use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::Config;
use crate::error::{ForgeError, Result};
use crate::graph::SharedGraph;
use crate::provider::router::ProviderRouter;
use crate::session::Session;
use crate::tools::executor::ToolExecutor;
use crate::tools::ToolRegistry;
use crate::types::*;

use super::compaction;
use super::context::{ContextManager, ContextStatus};
use super::hooks::{self, HooksConfig};
use super::system_prompt;

/// Events emitted by the agent loop to the TUI
#[derive(Debug, Clone)]
pub enum AgentEvent {
    ThinkingStart,
    Token(String),
    ToolStart { name: String, input: serde_json::Value },
    ToolEnd { name: String, output: String, is_error: bool },
    ContextWarning { used: u32, limit: u32 },
    Done,
    Error(String),
    // -- Multithread worker events (only emitted when /multithread is ON) --
    WorkerSpawned { worker_id: String, description: String },
    WorkerCompleted { worker_id: String, description: String, result: String, duration_ms: u64 },
    WorkerFailed { worker_id: String, description: String, error: String, duration_ms: u64 },
    WorkerToolStart { worker_id: String, name: String },
    WorkerToolEnd { worker_id: String, name: String, is_error: bool },
}

/// The core agentic loop
pub struct AgentLoop {
    pub provider_router: Arc<RwLock<ProviderRouter>>,
    pub tools: Arc<ToolRegistry>,
    pub session: Arc<Mutex<Session>>,
    pub config: Arc<Config>,
    pub event_tx: mpsc::UnboundedSender<AgentEvent>,
    pub permission_tx: mpsc::UnboundedSender<PermissionRequest>,
    pub permission_rx: Arc<Mutex<mpsc::UnboundedReceiver<PermissionResponse>>>,
    /// Shared semantic code graph (None when /forge-graph has not been built yet)
    pub graph: SharedGraph,
}

#[derive(Debug)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub description: String,
    pub level: PermissionLevel,
    pub response_tx: tokio::sync::oneshot::Sender<PermissionResponse>,
}

// ---------------------------------------------------------------------------
// Error categorization for retry logic
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum ErrorKind {
    Transient,      // Network glitch, service temporarily unavailable
    RateLimit,      // 429 Too Many Requests
    Overloaded,     // 529 / "overloaded" responses
    Auth,           // 401/403 — do NOT retry
    NotRetryable,   // Any other permanent error
}

fn categorize_error(err: &ForgeError) -> ErrorKind {
    match err {
        ForgeError::Api { status, message } => match status {
            429 => ErrorKind::RateLimit,
            529 => ErrorKind::Overloaded,
            500 | 502 | 503 | 504 => ErrorKind::Transient,
            401 | 403 => ErrorKind::Auth,
            _ => {
                // Check message content for additional hints
                let msg_lower = message.to_lowercase();
                if msg_lower.contains("overloaded") || msg_lower.contains("capacity") {
                    ErrorKind::Overloaded
                } else if msg_lower.contains("rate") || msg_lower.contains("limit") {
                    ErrorKind::RateLimit
                } else if msg_lower.contains("timeout") || msg_lower.contains("connection") {
                    ErrorKind::Transient
                } else {
                    ErrorKind::NotRetryable
                }
            }
        },
        ForgeError::Http(e) => {
            if e.is_timeout() || e.is_connect() {
                ErrorKind::Transient
            } else if e.status().map(|s| s.as_u16()) == Some(429) {
                ErrorKind::RateLimit
            } else {
                ErrorKind::NotRetryable
            }
        }
        ForgeError::Io(_) => ErrorKind::Transient,
        _ => ErrorKind::NotRetryable,
    }
}

/// Calculate exponential backoff delay in milliseconds
fn backoff_ms(attempt: u32, base_ms: u64, cap_ms: u64, kind: &ErrorKind) -> u64 {
    let base = match kind {
        ErrorKind::RateLimit => base_ms * 4, // rate limits need longer delay
        ErrorKind::Overloaded => base_ms * 2,
        _ => base_ms,
    };
    let delay = base * (2u64.pow(attempt.min(10)));
    delay.min(cap_ms)
}

/// Map effort level (1–5) to a temperature override.
/// Lower effort = more deterministic; higher effort = more creative.
fn effort_temperature(effort: u8) -> f32 {
    match effort {
        1 => 0.0,
        2 => 0.3,
        3 => 0.7,
        4 => 1.0,
        5 => 1.2,
        _ => 0.7,
    }
}

/// Remove orphaned tool_use / tool_result message pairs from the history.
///
/// The Anthropic API rejects conversations where a tool_result appears without
/// a corresponding tool_use block (or vice versa). This can happen if the
/// conversation was compacted or truncated mid-exchange.
fn normalize_messages(messages: &[Message]) -> Vec<Message> {
    normalize_messages_pub(messages)
}

/// Public version of normalize_messages — used by Worker to strip orphaned
/// tool_use / tool_result pairs from its own isolated history.
pub fn normalize_messages_pub(messages: &[Message]) -> Vec<Message> {
    // Collect all tool_use IDs present in assistant messages
    let mut used_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for msg in messages {
        if let Message::Assistant(content) = msg {
            for tc in content.tool_calls() {
                used_ids.insert(tc.id.clone());
            }
        }
    }

    // Filter out tool results whose tool_use_id has no matching assistant block
    messages
        .iter()
        .filter(|msg| {
            if let Message::Tool(result) = msg {
                used_ids.contains(&result.tool_use_id)
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

impl AgentLoop {
    /// Run one turn of the agent loop: process user message until completion
    pub async fn run(&self, user_message: String) -> Result<()> {
        // Load hooks config once at the start
        let hooks_config = HooksConfig::load();

        // 1. Add user message to history
        {
            let mut session = self.session.lock().await;
            session.history.add_user(user_message.clone());
        }

        // 2. Enter the loop
        let max_iterations = self.config.agent.max_tool_iterations;
        let mut iteration = 0;

        loop {
            iteration += 1;
            if iteration > max_iterations {
                let _ = self.event_tx.send(AgentEvent::Error(format!(
                    "Reached maximum tool iterations ({max_iterations}). Stopping."
                )));
                break;
            }

            // 3. Check context budget — auto-compact when at 90%+
            //
            // Behaviour matches Claude Code: at 80% we send a ContextWarning
            // event (TUI shows it in the header bar), at 90% we auto-compact
            // the entire conversation using the SAME model the user is
            // talking to. The full conversation is sent to the summarizer —
            // no per-message truncation. After compaction the conversation
            // is replaced by a single summary message (keep_last = 0).
            {
                let (needs_compact, ctx_window) = {
                    let session = self.session.lock().await;
                    let router = self.provider_router.read().await;
                    let ctx_window = router.active().map(|p| p.context_window()).unwrap_or(128_000);
                    let ctx_mgr = ContextManager::new(ctx_window);
                    let needs = match ctx_mgr.check(&session.history) {
                        ContextStatus::NeedsSummarization { used, limit } => {
                            let _ = self.event_tx.send(AgentEvent::ContextWarning { used, limit });
                            true
                        }
                        ContextStatus::Warning { used, limit } => {
                            let _ = self.event_tx.send(AgentEvent::ContextWarning { used, limit });
                            false
                        }
                        _ => false,
                    };
                    (needs, ctx_window)
                };

                if needs_compact {
                    // No hard minimum — user-configured policy. Default 0
                    // means "replace the whole thing with a summary".
                    let keep_last = compaction::DEFAULT_KEEP_LAST;

                    // Let the user know BEFORE we hit the wall.
                    let _ = self.event_tx.send(AgentEvent::Token(
                        "\n[context window full — auto-compacting conversation with the current model…]\n".to_string(),
                    ));

                    // Feed the FULL conversation (not just the prefix) to
                    // the summarizer so nothing is silently dropped. We
                    // then replace the prefix with the summary, keeping
                    // the last `keep_last` messages verbatim.
                    let (messages_all, model_id) = {
                        let session = self.session.lock().await;
                        let router = self.provider_router.read().await;
                        (
                            session.history.messages().to_vec(),
                            router.active_model_id().to_string(),
                        )
                    };

                    let (to_summarize_slice, _) =
                        compaction::split_for_compaction(&messages_all, keep_last);
                    let to_summarize = to_summarize_slice.to_vec();

                    let summary_result = {
                        let router = self.provider_router.read().await;
                        if let Ok(provider) = router.active() {
                            compaction::summarize_messages(
                                &to_summarize,
                                provider,
                                &model_id,
                                ctx_window,
                            )
                            .await
                        } else {
                            Ok("(auto-compact: provider unavailable)".to_string())
                        }
                    };

                    match summary_result {
                        Ok(summary) => {
                            let mut session = self.session.lock().await;
                            session.history.summarize_old(summary, keep_last);
                            let _ = self.event_tx.send(AgentEvent::Token(
                                "[context auto-compacted — conversation replaced with AI summary]\n"
                                    .to_string(),
                            ));
                        }
                        Err(e) => {
                            // Fall back to hard truncation so the next
                            // request doesn't exceed the window.
                            let mut session = self.session.lock().await;
                            session.history.compact(keep_last);
                            let _ = self.event_tx.send(AgentEvent::Error(format!(
                                "Auto-compact failed ({e}); fell back to truncation."
                            )));
                        }
                    }
                }
            }

            // 4. Build the request
            let request = {
                let session = self.session.lock().await;
                let router = self.provider_router.read().await;
                let provider = router.active()?;

                // Build graph info string (brief lock, released before await)
                let graph_info = self.graph.read()
                    .ok()
                    .and_then(|g| g.as_ref().map(|cg| format!(
                        "{} nodes, {} edges, {} files — built {}. \
                        Use graph_query tool for O(1) symbol lookup before reading files.",
                        cg.meta.total_nodes, cg.meta.total_edges,
                        cg.meta.file_count, cg.meta.age_description()
                    )));

                let system = system_prompt::build_system_prompt(
                    &std::path::PathBuf::from(&session.working_dir),
                    &self.config.general.system_prompt_extra,
                    graph_info.as_deref(),
                );

                let tools = if provider.supports_tools() {
                    Some(self.tools.all_definitions())
                } else {
                    None
                };

                // Effort-based temperature override
                let temperature = effort_temperature(session.effort_level);

                // Normalize messages to strip orphaned tool pairs
                let normalized_messages = normalize_messages(session.history.messages());

                ChatRequest {
                    model: router.active_model_id().to_string(),
                    messages: normalized_messages,
                    tools,
                    max_tokens: self.config.agent.max_tokens,
                    temperature,
                    system: Some(system),
                    stop_sequences: Vec::new(),
                }
            };

            // 5. Send ThinkingStart
            let _ = self.event_tx.send(AgentEvent::ThinkingStart);

            // 6. Stream tokens with retry logic (exponential backoff)
            let (response, did_stream) = self.call_provider_with_retry(request).await?;

            // 7. Record usage
            {
                let mut session = self.session.lock().await;
                let router = self.provider_router.read().await;
                if let Ok(provider) = router.active() {
                    session.record_usage(
                        &response.usage,
                        provider.input_cost_per_million(),
                        provider.output_cost_per_million(),
                    );
                }
            }

            // 8. If the provider didn't stream any text tokens (e.g. non-streaming
            //    provider or tool-only response), send the full response text
            //    so the TUI can display it. Guard against double-emission.
            if !did_stream {
                if let Some(text) = response.content.text() {
                    if !text.is_empty() {
                        let _ = self.event_tx.send(AgentEvent::Token(text.to_string()));
                    }
                }
            }

            // 9. Add assistant response to history
            {
                let mut session = self.session.lock().await;
                session.history.add_assistant(response.content.clone());
            }

            // 10. Check if we need to execute tools
            let tool_calls = response.content.tool_calls().to_vec();

            if tool_calls.is_empty() {
                // No tool calls — we're done
                let _ = self.event_tx.send(AgentEvent::Done);

                // Run Stop hooks
                let working_dir = {
                    let session = self.session.lock().await;
                    std::path::PathBuf::from(&session.working_dir)
                };
                hooks::run_stop_hooks(&hooks_config, working_dir).await;

                break;
            }

            // 11. Execute each tool call
            let executor = ToolExecutor::new(self.config.agent.max_output_per_tool);
            let working_dir = {
                let session = self.session.lock().await;
                std::path::PathBuf::from(&session.working_dir)
            };
            let ctx = ToolContext {
                working_dir: working_dir.clone(),
                home_dir: dirs::home_dir().unwrap_or_default(),
                session_id: {
                    let session = self.session.lock().await;
                    session.id.clone()
                },
                trust_mode: self.config.general.trust_mode,
            };

            for tc in &tool_calls {
                let _ = self.event_tx.send(AgentEvent::ToolStart {
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                });

                // Run PreToolUse hooks
                hooks::pre_tool_use(&hooks_config, &tc.name, &tc.input, working_dir.clone()).await;

                let perm_tx = self.permission_tx.clone();
                let trust_mode = self.config.general.trust_mode;

                let output = executor
                    .execute(
                        tc,
                        &ctx,
                        &self.tools,
                        |name, desc, level| async move {
                            if trust_mode {
                                return PermissionResponse::Allow;
                            }
                            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                            let req = PermissionRequest {
                                tool_name: name,
                                description: desc,
                                level,
                                response_tx: resp_tx,
                            };
                            let _ = perm_tx.send(req);
                            resp_rx.await.unwrap_or(PermissionResponse::Deny)
                        },
                    )
                    .await;

                // Run PostToolUse hooks
                hooks::post_tool_use(
                    &hooks_config,
                    &tc.name,
                    &tc.input,
                    &output.content,
                    output.is_error,
                    working_dir.clone(),
                ).await;

                let _ = self.event_tx.send(AgentEvent::ToolEnd {
                    name: tc.name.clone(),
                    output: output.content.clone(),
                    is_error: output.is_error,
                });

                // Add tool result to history
                {
                    let mut session = self.session.lock().await;
                    session.history.add_tool_result(ToolResult {
                        tool_use_id: tc.id.clone(),
                        content: output.content,
                        is_error: output.is_error,
                    });
                }
            }

            // Loop back — the LLM will see tool results and continue
        }

        // Auto-save session
        if self.config.general.auto_save_sessions {
            let session = self.session.lock().await;
            let _ = session.save();
        }

        Ok(())
    }

    /// Make an API call with exponential backoff retry logic.
    /// Retries up to 10 times for transient/rate-limit errors.
    /// Returns (response, did_stream_tokens).
    async fn call_provider_with_retry(&self, request: ChatRequest) -> Result<(ChatResponse, bool)> {
        const MAX_RETRIES: u32 = 10;
        const BASE_MS: u64 = 500;
        const CAP_MS: u64 = 120_000; // 2 minutes max delay

        let mut attempt = 0u32;

        loop {
            // Stream tokens concurrently while waiting for the full response
            let (stream_tx, stream_rx) = mpsc::unbounded_channel::<StreamEvent>();
            let event_tx_clone = self.event_tx.clone();
            let did_stream = Arc::new(AtomicBool::new(false));
            let did_stream_clone = did_stream.clone();

            let forwarder = tokio::spawn(async move {
                let mut rx = stream_rx;
                while let Some(event) = rx.recv().await {
                    if let StreamEvent::Token(t) = event {
                        did_stream_clone.store(true, Ordering::Release);
                        let _ = event_tx_clone.send(AgentEvent::Token(t));
                    }
                }
            });

            let chat_result = {
                let router = self.provider_router.read().await;
                let provider = router.active()?;
                provider.chat(request.clone(), stream_tx).await
                // router read guard + stream_tx dropped here
            };

            let _ = forwarder.await;

            match chat_result {
                Ok(response) => return Ok((response, did_stream.load(Ordering::Acquire))),
                Err(err) => {
                    let kind = categorize_error(&err);

                    // Non-retryable errors fail immediately
                    if kind == ErrorKind::Auth || kind == ErrorKind::NotRetryable {
                        let _ = self.event_tx.send(AgentEvent::Error(err.to_string()));
                        return Err(err);
                    }

                    attempt += 1;
                    if attempt > MAX_RETRIES {
                        let _ = self.event_tx.send(AgentEvent::Error(format!(
                            "Failed after {MAX_RETRIES} retries: {err}"
                        )));
                        return Err(err);
                    }

                    let delay_ms = backoff_ms(attempt, BASE_MS, CAP_MS, &kind);
                    let retry_msg = match kind {
                        ErrorKind::RateLimit => format!(
                            "Rate limited. Retrying in {:.1}s (attempt {attempt}/{MAX_RETRIES})...",
                            delay_ms as f64 / 1000.0
                        ),
                        ErrorKind::Overloaded => format!(
                            "API overloaded. Retrying in {:.1}s (attempt {attempt}/{MAX_RETRIES})...",
                            delay_ms as f64 / 1000.0
                        ),
                        _ => format!(
                            "Transient error. Retrying in {:.1}s (attempt {attempt}/{MAX_RETRIES})...",
                            delay_ms as f64 / 1000.0
                        ),
                    };

                    let _ = self.event_tx.send(AgentEvent::Error(retry_msg));
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

                    // Re-send ThinkingStart after retry
                    let _ = self.event_tx.send(AgentEvent::ThinkingStart);
                }
            }
        }
    }
}
