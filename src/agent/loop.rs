use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::agent::permissions::PermissionStore;
use crate::config::Config;
use crate::error::{ForgeError, Result};
use crate::graph::SharedGraph;
use crate::provider::router::ProviderRouter;
use crate::session::{FileStateCache, Session};
use crate::skills::{
    refresh_registry, ActiveSkillScope, SharedSkillRegistry, SkillExecutionMode, SkillHooks,
    SkillInvocationRecord,
};
use crate::tools::executor::ToolExecutor;
use crate::tools::ToolRegistry;
use crate::types::*;

use super::compaction;
use super::context::{ContextManager, ContextStatus};
use super::hooks::{self, HooksConfig, PreToolOutcome};
use super::system_prompt;

/// Events emitted by the agent loop to the TUI
#[derive(Debug, Clone)]
pub enum AgentEvent {
    ThinkingStart,
    Token(String),
    ToolStart {
        name: String,
        input: serde_json::Value,
    },
    ToolEnd {
        name: String,
        output: String,
        is_error: bool,
    },
    ContextWarning {
        used: u32,
        limit: u32,
    },
    /// Auto-compaction finished (success or graceful fallback). The TUI uses
    /// this to drain its rendered message log and refresh the context bar so
    /// the user SEES the window free up, not just the next turn.
    HistoryCompacted {
        kept: usize,
        removed: usize,
        summary_preview: String,
        succeeded: bool,
    },
    Done,
    Error(String),
    // -- Multithread worker events (only emitted when /multithread is ON) --
    WorkerSpawned {
        worker_id: String,
        description: String,
    },
    WorkerCompleted {
        worker_id: String,
        description: String,
        result: String,
        duration_ms: u64,
    },
    WorkerFailed {
        worker_id: String,
        description: String,
        error: String,
        duration_ms: u64,
    },
    WorkerToolStart {
        worker_id: String,
        name: String,
    },
    WorkerToolEnd {
        worker_id: String,
        name: String,
        is_error: bool,
    },
    /// The active skill scope has changed. `None` means scope cleared.
    /// TUI uses this to reflect the current scope in the status bar.
    SkillScopeChanged {
        name: Option<String>,
    },
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
    /// Session-scoped file-state cache, shared with tool execution contexts.
    pub file_cache: Arc<FileStateCache>,
    /// Permission rules loaded from disk. Refreshed at the start of every turn.
    pub permission_store: Arc<parking_lot::RwLock<PermissionStore>>,
    /// Cancellation signal honoured by provider streams and tool executors.
    /// Wrapped so the TUI can install a fresh token between turns without
    /// recreating the whole `AgentLoop` (CancellationToken is one-shot).
    /// `run()` snapshots `.read().clone()` at the start of each turn.
    pub cancel: Arc<parking_lot::RwLock<CancellationToken>>,
    /// Current permission mode (Default / Plan / AcceptEdits / Bypass).
    pub permission_mode: Arc<parking_lot::RwLock<PermissionMode>>,
    /// Current thinking config (Disabled / Enabled / Budget).
    pub thinking: Arc<parking_lot::RwLock<ThinkingConfig>>,
    pub skill_registry: SharedSkillRegistry,
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
    Transient,    // Network glitch, service temporarily unavailable
    RateLimit,    // 429 Too Many Requests
    Overloaded,   // 529 / "overloaded" responses
    Auth,         // 401/403 — do NOT retry
    NotRetryable, // Any other permanent error
}

fn categorize_error(err: &ForgeError) -> ErrorKind {
    match err {
        ForgeError::Api { status, message } => match status {
            429 => ErrorKind::RateLimit,
            529 => ErrorKind::Overloaded,
            500 | 502 | 503 | 504 => ErrorKind::Transient,
            401 | 403 => ErrorKind::Auth,
            _ => {
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

fn backoff_ms(attempt: u32, base_ms: u64, cap_ms: u64, kind: &ErrorKind) -> u64 {
    let base = match kind {
        ErrorKind::RateLimit => base_ms * 4,
        ErrorKind::Overloaded => base_ms * 2,
        _ => base_ms,
    };
    let delay = base * (2u64.pow(attempt.min(10)));
    delay.min(cap_ms)
}

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

fn normalize_messages(messages: &[Message]) -> Vec<Message> {
    normalize_messages_pub(messages)
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SkillInvocationMetadata {
    success: bool,
    mode: String,
    skill_name: String,
    applied_allowed_tools: Option<Vec<String>>,
    model_override: Option<String>,
    materialized_prompt: String,
    source: String,
    canonical_path: Option<String>,
    #[serde(default)]
    hooks: SkillHooksMetadata,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct SkillHooksMetadata {
    #[serde(rename = "PreToolUse", default)]
    pre_tool_use: Vec<crate::agent::hooks::HookEntry>,
    #[serde(rename = "PostToolUse", default)]
    post_tool_use: Vec<crate::agent::hooks::HookEntry>,
    #[serde(rename = "Stop", default)]
    stop: Vec<crate::agent::hooks::HookEntry>,
}

pub fn normalize_messages_pub(messages: &[Message]) -> Vec<Message> {
    let mut used_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for msg in messages {
        if let Message::Assistant(content) = msg {
            for tc in content.tool_calls() {
                used_ids.insert(tc.id.clone());
            }
        }
    }
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

pub struct ConsecutiveFailureTracker {
    failures: HashMap<(String, String), u32>,
    max_consecutive: u32,
}

impl ConsecutiveFailureTracker {
    pub fn new(max: u32) -> Self {
        Self {
            failures: HashMap::new(),
            max_consecutive: max,
        }
    }

    pub fn record(
        &mut self,
        tool_name: &str,
        input: &serde_json::Value,
        is_error: bool,
    ) -> Option<u32> {
        let file_path = input["path"].as_str().unwrap_or("").to_string();
        let key = (tool_name.to_string(), file_path);
        if is_error {
            let count = self.failures.entry(key).or_insert(0);
            *count += 1;
            if *count >= self.max_consecutive {
                let total = *count;
                return Some(total);
            }
        } else {
            self.failures.remove(&key);
        }
        None
    }

    pub fn reset(&mut self) {
        self.failures.clear();
    }
}

impl AgentLoop {
    /// Trigger cancellation of the current turn. Safe to call from any thread.
    pub fn cancel_current_turn(&self) {
        self.cancel.read().cancel();
    }

    /// Install a fresh cancellation token for the next turn. Must be called
    /// after a cancel + before issuing another `run()`. Returns a clone of
    /// the newly installed token so the TUI can retain a handle to it.
    pub fn reset_cancel(&self) -> CancellationToken {
        let fresh = CancellationToken::new();
        *self.cancel.write() = fresh.clone();
        fresh
    }

    /// Snapshot the active cancellation token. Every use of the token inside
    /// the loop/tool pipeline goes through this so a mid-turn swap cannot
    /// race — the snapshot is taken once per turn.
    fn cancel_token(&self) -> CancellationToken {
        self.cancel.read().clone()
    }

    fn build_tool_ctx(&self, working_dir: std::path::PathBuf, session_id: String) -> ToolContext {
        let mode = *self.permission_mode.read();
        let active_skill_scope = self
            .session
            .try_lock()
            .ok()
            .and_then(|session| session.active_skill_scope.clone());
        ToolContext {
            working_dir,
            home_dir: dirs::home_dir().unwrap_or_default(),
            session_id,
            trust_mode: mode == PermissionMode::Bypass || self.config.general.trust_mode,
            permission_mode: mode,
            file_cache: Some(self.file_cache.clone()),
            active_skill_scope,
            skill_registry: Some(self.skill_registry.clone()),
        }
    }

    /// Run one turn of the agent loop: process user message until completion
    #[instrument(skip_all, fields(user_msg_len = user_message.len()))]
    pub async fn run(&self, user_message: String) -> Result<()> {
        let hooks_config = HooksConfig::load();

        // Refresh stored permission rules each turn so `/permissions` edits take effect.
        {
            let fresh = PermissionStore::load();
            *self.permission_store.write() = fresh;
        }

        let working_dir_pb = {
            let s = self.session.lock().await;
            std::path::PathBuf::from(&s.working_dir)
        };
        if self.config.agent.skills_enabled {
            refresh_registry(&self.skill_registry, &working_dir_pb);
        }
        let session_id = {
            let s = self.session.lock().await;
            s.id.clone()
        };

        // UserPromptSubmit hook (may veto).
        if let Err(reason) = hooks::user_prompt_submit(
            &hooks_config,
            &user_message,
            working_dir_pb.clone(),
            Some(session_id.clone()),
        )
        .await
        {
            let _ = self.event_tx.send(AgentEvent::Error(format!(
                "UserPromptSubmit hook vetoed this turn: {reason}"
            )));
            return Ok(());
        }

        // 1. Add user message to history
        {
            let mut session = self.session.lock().await;
            session.history.add_user(user_message.clone());
        }

        let max_iterations = self.config.agent.max_tool_iterations;
        let mut iteration = 0;
        let mut failure_tracker = ConsecutiveFailureTracker::new(3);

        loop {
            iteration += 1;
            if iteration > max_iterations {
                let _ = self.event_tx.send(AgentEvent::Error(format!(
                    "Reached maximum tool iterations ({max_iterations}). Stopping."
                )));
                break;
            }

            if self.cancel_token().is_cancelled() {
                let _ = self
                    .event_tx
                    .send(AgentEvent::Error("Turn cancelled by user.".to_string()));
                break;
            }

            // 3. Check context budget — auto-compact when at 90%+
            {
                let (needs_compact, ctx_window) = {
                    let session = self.session.lock().await;
                    let router = self.provider_router.read().await;
                    let ctx_window = router
                        .active()
                        .map(|p| p.context_window())
                        .unwrap_or(128_000);
                    let ctx_mgr = ContextManager::new(ctx_window);
                    let needs = match ctx_mgr.check(&session.history) {
                        ContextStatus::NeedsSummarization { used, limit } => {
                            let _ = self
                                .event_tx
                                .send(AgentEvent::ContextWarning { used, limit });
                            true
                        }
                        ContextStatus::Warning { used, limit } => {
                            let _ = self
                                .event_tx
                                .send(AgentEvent::ContextWarning { used, limit });
                            false
                        }
                        _ => false,
                    };
                    (needs, ctx_window)
                };

                if needs_compact {
                    hooks::pre_compact(
                        &hooks_config,
                        working_dir_pb.clone(),
                        Some(session_id.clone()),
                    )
                    .await;

                    let keep_last = compaction::DEFAULT_KEEP_LAST;

                    // Always route compaction through the CURRENTLY-ACTIVE
                    // provider+model. Reading `session.model_id` (which is
                    // set at session creation and not updated on /model
                    // switch) caused "invalid model" errors when the user
                    // changed provider mid-conversation.
                    let (messages_all, invoked_skills, model_id, provider_name) = {
                        let session = self.session.lock().await;
                        let router = self.provider_router.read().await;
                        let pname = router
                            .active()
                            .map(|p| p.name().to_string())
                            .unwrap_or_default();
                        (
                            session.history.messages().to_vec(),
                            session.invoked_skills.clone(),
                            router.active_model_id().to_string(),
                            pname,
                        )
                    };
                    let total_before = messages_all.len();

                    let _ = self.event_tx.send(AgentEvent::Token(format!(
                        "\n[context window full — auto-compacting {} message(s) via {provider_name} ({model_id})…]\n",
                        total_before
                    )));

                    let (to_summarize_slice, _) =
                        compaction::split_for_compaction(&messages_all, keep_last);
                    let to_summarize = to_summarize_slice.to_vec();

                    let summary_result = {
                        let router = self.provider_router.read().await;
                        match router.active() {
                            Ok(provider) => {
                                compaction::summarize_messages(
                                    &to_summarize,
                                    &invoked_skills,
                                    provider,
                                    &model_id,
                                    ctx_window,
                                )
                                .await
                            }
                            Err(e) => Err(e),
                        }
                    };

                    match summary_result {
                        Ok(summary) => {
                            let preview: String = summary.chars().take(400).collect();
                            let summary_word_count = summary.split_whitespace().count();
                            let removed = total_before.saturating_sub(keep_last);
                            {
                                let mut session = self.session.lock().await;
                                session.history.summarize_old(summary, keep_last);
                            }
                            let _ = self.event_tx.send(AgentEvent::HistoryCompacted {
                                kept: keep_last,
                                removed,
                                summary_preview: preview,
                                succeeded: true,
                            });
                            let _ = self.event_tx.send(AgentEvent::Token(format!(
                                "[context auto-compacted — {removed} message(s) replaced by an AI summary ({} words)]\n",
                                summary_word_count,
                            )));
                        }
                        Err(e) => {
                            let removed = total_before.saturating_sub(keep_last);
                            {
                                let mut session = self.session.lock().await;
                                session.history.compact(keep_last);
                            }
                            let _ = self.event_tx.send(AgentEvent::HistoryCompacted {
                                kept: keep_last,
                                removed,
                                summary_preview: String::new(),
                                succeeded: false,
                            });
                            let _ = self.event_tx.send(AgentEvent::Error(format!(
                                "Auto-compact summarizer failed ({e}); fell back to plain truncation. \
                                 {removed} message(s) removed from context."
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

                let graph_info = self.graph.read().ok().and_then(|g| {
                    g.as_ref().map(|cg| {
                        format!(
                            "{} nodes, {} edges, {} files — built {}. \
                        Use graph_query tool for O(1) symbol lookup before reading files.",
                            cg.meta.total_nodes,
                            cg.meta.total_edges,
                            cg.meta.file_count,
                            cg.meta.age_description()
                        )
                    })
                });

                let skills_guard = self.skill_registry.read();
                let skills = if self.config.agent.skills_enabled {
                    Some(&*skills_guard)
                } else {
                    None
                };
                let mut system = system_prompt::build_system_prompt(
                    &std::path::PathBuf::from(&session.working_dir),
                    &self.config.general.system_prompt_extra,
                    graph_info.as_deref(),
                    skills,
                    self.config.agent.max_skill_listed_in_prompt,
                    self.config.agent.skills_enabled
                        && self.config.agent.include_skills_in_system_prompt,
                );

                let mode = *self.permission_mode.read();
                if mode != PermissionMode::Default {
                    system.push_str(&format!(
                        "\n\n## Permission Mode\n\
                         The current permission mode is `{}`. ",
                        mode.as_label()
                    ));
                    match mode {
                        PermissionMode::Plan => system.push_str(
                            "You may ONLY use ReadOnly tools (read_file, search_files, \
                             list_directory, git_status, git_diff, etc.). Any attempt \
                             to mutate state will be denied. Use `exit_plan_mode` when \
                             you have a complete plan to present to the user.",
                        ),
                        PermissionMode::AcceptEdits => system.push_str(
                            "File mutations (write_file / edit_file / create_file) \
                             auto-approve. Destructive, Shell, and Network tools still \
                             require explicit user approval.",
                        ),
                        PermissionMode::Bypass => {
                            system.push_str("All tools auto-approve. Proceed efficiently.")
                        }
                        _ => {}
                    }
                }

                let tools = if provider.supports_tools() {
                    Some(self.tools.all_definitions())
                } else {
                    None
                };

                let temperature = effort_temperature(session.effort_level);
                let normalized_messages = normalize_messages(session.history.messages());

                ChatRequest {
                    model: session
                        .active_skill_scope()
                        .and_then(|scope| scope.model_override.clone())
                        .unwrap_or_else(|| router.active_model_id().to_string()),
                    messages: normalized_messages,
                    tools,
                    max_tokens: self.config.agent.max_tokens,
                    temperature,
                    system: Some(system),
                    stop_sequences: Vec::new(),
                    thinking: *self.thinking.read(),
                }
            };

            let _ = self.event_tx.send(AgentEvent::ThinkingStart);

            let cancel = self.cancel_token();
            let call_result = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    let _ = self.event_tx.send(AgentEvent::Error("Turn cancelled by user.".to_string()));
                    break;
                }
                r = self.call_provider_with_retry(request) => r,
            };
            let (response, did_stream) = call_result?;

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

            if !did_stream {
                if let Some(text) = response.content.text() {
                    if !text.is_empty() {
                        let _ = self.event_tx.send(AgentEvent::Token(text.to_string()));
                    }
                }
            }

            {
                let mut session = self.session.lock().await;
                session.history.add_assistant(response.content.clone());
            }

            let tool_calls = response.content.tool_calls().to_vec();

            if tool_calls.is_empty() {
                let _ = self.event_tx.send(AgentEvent::Done);
                hooks::run_stop_hooks(
                    &hooks_config,
                    working_dir_pb.clone(),
                    Some(session_id.clone()),
                )
                .await;
                if let Some(scope) = self.session.lock().await.active_skill_scope.clone() {
                    let skill_hooks = scope.hooks.as_hooks_config();
                    hooks::run_stop_hooks(
                        &skill_hooks,
                        working_dir_pb.clone(),
                        Some(session_id.clone()),
                    )
                    .await;
                }
                let had_scope = {
                    let mut session = self.session.lock().await;
                    let had = session.active_skill_scope.is_some();
                    session.active_skill_scope = None;
                    had
                };
                if had_scope {
                    let _ = self
                        .event_tx
                        .send(AgentEvent::SkillScopeChanged { name: None });
                }
                break;
            }

            self.execute_tool_calls(
                &tool_calls,
                &hooks_config,
                &mut failure_tracker,
                &working_dir_pb,
                &session_id,
            )
            .await;
        }

        if self.config.general.auto_save_sessions {
            let session = self.session.lock().await;
            let _ = session.save();
        }

        Ok(())
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: &[ToolCall],
        hooks_config: &HooksConfig,
        failure_tracker: &mut ConsecutiveFailureTracker,
        working_dir_pb: &std::path::PathBuf,
        session_id: &str,
    ) {
        let executor = Arc::new(ToolExecutor::new(self.config.agent.max_output_per_tool));
        let ctx = Arc::new(self.build_tool_ctx(working_dir_pb.clone(), session_id.to_string()));

        let mut safe: Vec<(usize, ToolCall)> = Vec::new();
        let mut serial: Vec<(usize, ToolCall)> = Vec::new();
        for (i, tc) in tool_calls.iter().enumerate() {
            match self.tools.get(&tc.name) {
                Some(t) if t.is_concurrency_safe() => safe.push((i, tc.clone())),
                _ => serial.push((i, tc.clone())),
            }
        }

        let mut results: Vec<(usize, ToolCall, ToolOutput)> = Vec::with_capacity(tool_calls.len());

        if !safe.is_empty() {
            let mut futs = Vec::with_capacity(safe.len());
            for (idx, tc) in safe.into_iter() {
                let _ = self.event_tx.send(AgentEvent::ToolStart {
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                });

                let executor = executor.clone();
                let ctx = ctx.clone();
                let tools = self.tools.clone();
                let store_arc = self.permission_store.clone();
                let cancel = self.cancel_token();
                let event_tx = self.event_tx.clone();
                let perm_tx = self.permission_tx.clone();
                let hooks_cfg = hooks_config.clone();
                let working_dir = working_dir_pb.clone();
                let sid = session_id.to_string();

                futs.push(tokio::spawn(async move {
                    let output = Self::execute_single(
                        executor.as_ref(),
                        &tc,
                        ctx.as_ref(),
                        tools.as_ref(),
                        &store_arc,
                        &cancel,
                        &hooks_cfg,
                        working_dir,
                        sid,
                        event_tx.clone(),
                        perm_tx,
                    )
                    .await;

                    let _ = event_tx.send(AgentEvent::ToolEnd {
                        name: tc.name.clone(),
                        output: output.content.clone(),
                        is_error: output.is_error,
                    });

                    (idx, tc, output)
                }));
            }

            for f in futs {
                if let Ok(triple) = f.await {
                    results.push(triple);
                }
            }
        }

        for (idx, tc) in serial.into_iter() {
            if self.cancel_token().is_cancelled() {
                results.push((
                    idx,
                    tc,
                    ToolOutput::error("Cancelled before execution".to_string()),
                ));
                continue;
            }
            let _ = self.event_tx.send(AgentEvent::ToolStart {
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
            let output = Self::execute_single(
                executor.as_ref(),
                &tc,
                ctx.as_ref(),
                self.tools.as_ref(),
                &self.permission_store,
                &self.cancel_token(),
                hooks_config,
                working_dir_pb.clone(),
                session_id.to_string(),
                self.event_tx.clone(),
                self.permission_tx.clone(),
            )
            .await;
            let _ = self.event_tx.send(AgentEvent::ToolEnd {
                name: tc.name.clone(),
                output: output.content.clone(),
                is_error: output.is_error,
            });
            results.push((idx, tc, output));
        }

        results.sort_by_key(|(i, _, _)| *i);
        for (_, tc, output) in results {
            {
                let mut session = self.session.lock().await;
                session.history.add_tool_result(ToolResult {
                    tool_use_id: tc.id.clone(),
                    content: output.content.clone(),
                    is_error: output.is_error,
                });
            }

            self.apply_special_tool_effects(&tc, &output).await;

            if let Some(fail_count) = failure_tracker.record(&tc.name, &tc.input, output.is_error) {
                let file_path = tc.input["path"].as_str().unwrap_or("(unknown)");
                let nudge = format!(
                    "[SYSTEM] The tool '{}' has failed {} consecutive times on '{}'. \
                     STOP retrying the same approach. Instead:\n\
                     1. Use read_file to get the CURRENT file contents\n\
                     2. Use write_file to replace the ENTIRE file with the corrected version\n\
                     Do NOT attempt edit_file on this file again.",
                    tc.name, fail_count, file_path
                );
                let _ = self.event_tx.send(AgentEvent::Token(
                    format!("\n⚠️  Circuit breaker triggered: {} failed {} times on {}. Forcing fallback to write_file.\n",
                        tc.name, fail_count, file_path)
                ));
                let mut session = self.session.lock().await;
                session.history.add_user(nudge);
                failure_tracker.reset();
            }
        }
    }

    async fn apply_special_tool_effects(&self, tc: &ToolCall, output: &ToolOutput) {
        if tc.name != "invoke_skill" {
            return;
        }
        let Some(metadata_value) = output
            .metadata
            .as_ref()
            .and_then(|m| m.get("skill_invocation"))
            .cloned()
        else {
            return;
        };

        let Ok(meta) = serde_json::from_value::<SkillInvocationMetadata>(metadata_value) else {
            return;
        };
        if !meta.success {
            return;
        }

        let source = match meta.source.as_str() {
            "project" => crate::skills::SkillSource::Project,
            "user" => crate::skills::SkillSource::User,
            _ => crate::skills::SkillSource::Bundled,
        };

        let scope = ActiveSkillScope {
            skill_name: meta.skill_name.clone(),
            allowed_tools: meta.applied_allowed_tools.clone().unwrap_or_default(),
            model_override: meta.model_override.clone(),
            hooks: SkillHooks {
                pre_tool_use: meta.hooks.pre_tool_use.clone(),
                post_tool_use: meta.hooks.post_tool_use.clone(),
                stop: meta.hooks.stop.clone(),
            },
            execution_mode: if meta.mode == "fork" {
                SkillExecutionMode::Fork
            } else {
                SkillExecutionMode::Inline
            },
        };

        if meta.mode == "fork" {
            // Fork mode: run the skill in an isolated worker without blocking
            // the main loop. We push the invocation record immediately and
            // splice the result/failure back into history via a spawned task.
            {
                let mut session = self.session.lock().await;
                session.push_invoked_skill(SkillInvocationRecord {
                    skill_name: meta.skill_name.clone(),
                    source,
                    canonical_path: meta.canonical_path.as_ref().map(std::path::PathBuf::from),
                    materialized_prompt: meta.materialized_prompt.clone(),
                    invoked_at: chrono::Utc::now(),
                    worker_id: None,
                });
                session.active_skill_scope = None;
            }
            let working_dir = {
                let session = self.session.lock().await;
                session.working_dir.clone()
            };
            let worker = super::worker::Worker::new(
                format!("skill:{}", meta.skill_name),
                self.provider_router.clone(),
                self.tools.clone(),
                self.config.clone(),
                self.graph.clone(),
                working_dir,
            );
            let worker_id = worker.id.clone();
            let prompt = meta.materialized_prompt.clone();
            let session_arc = self.session.clone();
            let event_tx = self.event_tx.clone();
            let skill_name = meta.skill_name.clone();

            tokio::spawn(async move {
                let (notify_tx, mut notify_rx) = mpsc::unbounded_channel();
                worker.run(prompt, notify_tx, event_tx).await;
                if let Some(notification) = notify_rx.recv().await {
                    match notification.status {
                        super::worker::WorkerStatus::Completed { result, .. } => {
                            let mut session = session_arc.lock().await;
                            session
                                .history
                                .add_user(format!("[Skill Result: {skill_name}]\n{result}"));
                            if let Some(last) = session.invoked_skills.last_mut() {
                                last.worker_id = Some(worker_id);
                            }
                        }
                        super::worker::WorkerStatus::Failed { error, .. } => {
                            let mut session = session_arc.lock().await;
                            session
                                .history
                                .add_user(format!("[Skill Failure: {skill_name}]\n{error}"));
                        }
                        _ => {}
                    }
                }
            });
        } else {
            // Inline mode: the materialized prompt is already embedded in the
            // tool_result content (see InvokeSkillTool::execute). We only need
            // to install the scope and record the invocation — do NOT inject
            // another user turn, that would duplicate context and violate the
            // user/assistant turn alternation the providers expect.
            let skill_name = meta.skill_name.clone();
            {
                let mut session = self.session.lock().await;
                session.active_skill_scope = Some(scope);
                session.push_invoked_skill(SkillInvocationRecord {
                    skill_name: meta.skill_name.clone(),
                    source,
                    canonical_path: meta.canonical_path.as_ref().map(std::path::PathBuf::from),
                    materialized_prompt: meta.materialized_prompt.clone(),
                    invoked_at: chrono::Utc::now(),
                    worker_id: None,
                });
            }
            let _ = self.event_tx.send(AgentEvent::SkillScopeChanged {
                name: Some(skill_name),
            });
        }
    }

    #[instrument(skip_all, fields(tool = %tc.name, id = %tc.id))]
    #[allow(clippy::too_many_arguments)]
    async fn execute_single(
        executor: &ToolExecutor,
        tc: &ToolCall,
        ctx: &ToolContext,
        tools: &ToolRegistry,
        store: &Arc<parking_lot::RwLock<PermissionStore>>,
        cancel: &CancellationToken,
        hooks_config: &HooksConfig,
        working_dir: std::path::PathBuf,
        session_id: String,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        perm_tx: mpsc::UnboundedSender<PermissionRequest>,
    ) -> ToolOutput {
        let skill_hooks = ctx
            .active_skill_scope
            .as_ref()
            .map(|scope| scope.hooks.as_hooks_config());
        match hooks::pre_tool_use(
            hooks_config,
            &tc.name,
            &tc.input,
            working_dir.clone(),
            Some(session_id.clone()),
        )
        .await
        {
            PreToolOutcome::Proceed => {}
            PreToolOutcome::Veto { reason, hook } => {
                let msg = format!(
                    "PreToolUse hook vetoed '{}' (hook: `{}`): {}",
                    tc.name, hook, reason
                );
                let _ = event_tx.send(AgentEvent::Error(msg.clone()));
                return ToolOutput::error(msg);
            }
        }
        if let Some(skill_hooks) = &skill_hooks {
            match hooks::pre_tool_use(
                skill_hooks,
                &tc.name,
                &tc.input,
                working_dir.clone(),
                Some(session_id.clone()),
            )
            .await
            {
                PreToolOutcome::Proceed => {}
                PreToolOutcome::Veto { reason, hook } => {
                    let msg = format!(
                        "Skill PreToolUse hook vetoed '{}' (hook: `{}`): {}",
                        tc.name, hook, reason
                    );
                    let _ = event_tx.send(AgentEvent::Error(msg.clone()));
                    return ToolOutput::error(msg);
                }
            }
        }

        let trust_mode = ctx.trust_mode;
        let store_snapshot = store.read().clone();
        let output = executor
            .execute(
                tc,
                ctx,
                tools,
                &store_snapshot,
                cancel,
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

        hooks::post_tool_use(
            hooks_config,
            &tc.name,
            &tc.input,
            &output.content,
            output.is_error,
            working_dir,
            Some(session_id),
        )
        .await;
        if let Some(skill_hooks) = &skill_hooks {
            hooks::post_tool_use(
                skill_hooks,
                &tc.name,
                &tc.input,
                &output.content,
                output.is_error,
                ctx.working_dir.clone(),
                Some(ctx.session_id.clone()),
            )
            .await;
        }

        output
    }

    async fn call_provider_with_retry(&self, request: ChatRequest) -> Result<(ChatResponse, bool)> {
        const MAX_RETRIES: u32 = 10;
        const BASE_MS: u64 = 500;
        const CAP_MS: u64 = 120_000;

        let mut attempt = 0u32;

        loop {
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
                let cancel = self.cancel_token();
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        Err(ForgeError::Provider("cancelled by user".into()))
                    }
                    r = provider.chat(request.clone(), stream_tx) => r,
                }
            };

            let _ = forwarder.await;

            match chat_result {
                Ok(response) => return Ok((response, did_stream.load(Ordering::Acquire))),
                Err(err) => {
                    if self.cancel_token().is_cancelled() {
                        return Err(err);
                    }
                    let kind = categorize_error(&err);

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
                    let cancel_backoff = self.cancel_token();
                    tokio::select! {
                        biased;
                        _ = cancel_backoff.cancelled() => {
                            return Err(ForgeError::Provider("cancelled by user during backoff".into()));
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(delay_ms)) => {}
                    }

                    let _ = self.event_tx.send(AgentEvent::ThinkingStart);
                }
            }
        }
    }
}
