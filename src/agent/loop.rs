use futures::FutureExt;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::agent::permissions::PermissionStore;
use crate::config::Config;
use crate::error::{ForgeError, Result};
use crate::graph::SharedGraph;
use crate::lsp::SharedLspManager;
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
    /// One chunk of extended-thinking / reasoning content. Emitted between
    /// `ThinkingStart` and `ThinkingEnd` if (and only if) the active
    /// provider streams thinking separately from the visible answer.
    ThinkingDelta {
        text: String,
    },
    /// All thinking content for the current assistant turn has been received.
    /// The next `Token` (if any) is the visible answer.
    ThinkingEnd,
    Token(String),
    ToolStart {
        /// Provider-issued tool-call id (stable across Start/End within a
        /// single turn). Empty string is reserved for legacy emitters.
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolEnd {
        /// Pairs with the `id` from the matching `ToolStart`.
        id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    /// Incremental stdout/stderr from a long-running tool (currently `bash`
    /// and `powershell`). Emitted between `ToolStart` and `ToolEnd` so IDEs
    /// can show live tail output instead of waiting for the buffered
    /// `output_excerpt` on `ToolEnd`. The TUI ignores this — it has no live
    /// rendering path for tool output today.
    ToolOutputDelta {
        /// Matches the `id` from `ToolStart`/`ToolEnd`.
        id: String,
        /// `"stdout"` or `"stderr"`.
        stream: String,
        text: String,
    },
    /// Computed unified diff for a file-mutation tool, emitted *before*
    /// the corresponding permission request so an IDE can open a native
    /// diff editor next to the prompt. Suppressed for non-file tools.
    DiffPreview {
        tool_call_id: String,
        path: String,
        unified_diff: String,
    },
    /// Usage for the most recent provider call only (NOT cumulative).
    /// `cost_usd` is the marginal cost of this turn step. Useful for IDEs
    /// that want to show "this turn cost $X" without subtracting snapshots.
    TurnUsage {
        input: u32,
        output: u32,
        cache_read: u32,
        cache_write: u32,
        cost_usd: f64,
    },
    ContextWarning {
        used: u32,
        limit: u32,
    },
    CompactionStart {
        message_count: usize,
        provider_name: String,
        model_id: String,
        automatic: bool,
    },
    /// Auto-compaction finished (success or graceful fallback). The TUI uses
    /// this to drain its rendered message log and refresh the context bar so
    /// the user SEES the window free up, not just the next turn.
    HistoryCompacted {
        kept: usize,
        removed: usize,
        summary_preview: String,
        succeeded: bool,
        automatic: bool,
        elapsed_ms: u64,
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
    /// The persistent task plan was created or updated (via the `update_plan`
    /// tool). The TUI uses this to render the live, ticking task checklist.
    /// Carries the full current plan so the panel can be re-rendered wholesale.
    PlanUpdated {
        plan: crate::session::TaskPlan,
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
    /// Shared LSP manager for automatic code intelligence checks.
    pub lsp: SharedLspManager,
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
    /// Optional channel for streaming `ToolOutputChunk`s produced by
    /// long-running tools (bash/powershell stdout/stderr lines).  When set,
    /// the construction site is expected to spawn a forwarder task that
    /// reads from the paired receiver and emits `AgentEvent::ToolOutputDelta`
    /// into `event_tx`.  The TUI leaves this `None` (no live rendering path);
    /// the JSON-RPC bridge wires it up.
    pub output_chunk_tx: Option<mpsc::UnboundedSender<crate::types::ToolOutputChunk>>,
}

#[derive(Debug)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub input_summary: String,
    pub description: String,
    pub level: PermissionLevel,
    /// Raw JSON args of the tool call. Used by the goal policy gate so
    /// `write_globs` can be enforced exactly against the actual `path`
    /// arguments instead of having to parse them out of `input_summary`.
    /// Existing consumers (the TUI confirmation modal) ignore this field.
    pub input: serde_json::Value,
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

fn latest_user_message_tokens(messages: &[Message]) -> u32 {
    match messages.last() {
        Some(Message::User(UserContent::Text(text))) => {
            crate::session::tokens::TokenCounter::count_text(text)
        }
        _ => 0,
    }
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

fn lsp_diagnostic_target(
    tc: &ToolCall,
    working_dir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let key = match tc.name.as_str() {
        "write_file" | "edit_file" | "create_file" => "path",
        "copy_file" | "move_file" => "destination",
        _ => return None,
    };
    let path = std::path::Path::new(tc.input.get(key)?.as_str()?);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    })
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
            diff_review: self.config.ui.diff_before_apply,
            file_cache: Some(self.file_cache.clone()),
            active_skill_scope,
            skill_registry: Some(self.skill_registry.clone()),
            // Cloned chunk sender for streaming tools. Per-call `tool_call_id`
            // is populated by `ToolExecutor::execute` immediately before the
            // tool runs (see `tools::executor`), so emitted deltas can be
            // tagged with the matching `ToolCallStart` id.
            output_chunk_tx: self.output_chunk_tx.clone(),
            tool_call_id: None,
            // The top-level loop is a single agent — no shared team blackboard.
            // Sub-agents spawned via spawn_team / the coordinator get their own.
            team_blackboard: None,
        }
    }

    /// Run one turn from a plain-text user message (back-compat entry point used
    /// by workers, skills, and the goal loop).
    pub async fn run(&self, user_message: String) -> Result<()> {
        self.run_user(crate::types::UserContent::Text(user_message)).await
    }

    /// Run one turn of the agent loop from (possibly multimodal) user content.
    #[instrument(skip_all)]
    pub async fn run_user(&self, user_content: crate::types::UserContent) -> Result<()> {
        let user_message = user_content.to_text();
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

        // 1. Add user message to history (preserving any attached images)
        {
            let mut session = self.session.lock().await;
            session.history.add_user_content(user_content);
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
                    let ctx_window = router.active_context_window();
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
                    let latest_user_tokens = latest_user_message_tokens(&messages_all);
                    let fresh_user_budget = ctx_window
                        .saturating_sub(self.config.agent.max_tokens)
                        .saturating_sub(2_048);
                    if latest_user_tokens > fresh_user_budget {
                        let _ = self.event_tx.send(AgentEvent::Error(format!(
                            "Latest user message is ~{latest_user_tokens} tokens, which cannot fit safely in the active {ctx_window}-token context window after reserving response/tool budget. It was not auto-compacted away. Please switch to a larger model or submit a smaller chunk."
                        )));
                        break;
                    }
                    let keep_last = if latest_user_tokens > 0 {
                        1
                    } else {
                        compaction::DEFAULT_KEEP_LAST
                    };

                    let _ = self.event_tx.send(AgentEvent::Token(format!(
                        "\n[context window full — auto-compacting {} message(s) via {provider_name} ({model_id})…]\n",
                        total_before
                    )));

                    let (to_summarize_slice, _) =
                        compaction::split_for_compaction(&messages_all, keep_last);
                    let to_summarize = to_summarize_slice.to_vec();
                    let compaction_started_at = Instant::now();
                    let _ = self.event_tx.send(AgentEvent::CompactionStart {
                        message_count: to_summarize.len(),
                        provider_name: provider_name.clone(),
                        model_id: model_id.clone(),
                        automatic: true,
                    });

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
                            let summary_word_count = summary.split_whitespace().count();
                            let removed = total_before.saturating_sub(keep_last);
                            let save_result = {
                                let mut session = self.session.lock().await;
                                session.history.summarize_old(summary.clone(), keep_last);
                                let live_estimate =
                                    crate::session::tokens::TokenCounter::count_messages(
                                        session.history.messages(),
                                    );
                                session.cost_tracker.last_prompt_tokens = live_estimate;
                                session.cost_tracker.last_output_tokens = 0;
                                if self.config.general.auto_save_sessions {
                                    session.save()
                                } else {
                                    Ok(())
                                }
                            };
                            let _ = self.event_tx.send(AgentEvent::HistoryCompacted {
                                kept: keep_last,
                                removed,
                                summary_preview: summary,
                                succeeded: true,
                                automatic: true,
                                elapsed_ms: compaction_started_at.elapsed().as_millis() as u64,
                            });
                            let _ = self.event_tx.send(AgentEvent::Token(format!(
                                "[context auto-compacted — {removed} message(s) replaced by an AI summary ({} words)]\n",
                                summary_word_count,
                            )));
                            if let Err(e) = save_result {
                                let _ = self.event_tx.send(AgentEvent::Error(format!(
                                    "Auto-compacted history could not be saved to disk: {e}"
                                )));
                            }
                        }
                        Err(e) => {
                            let removed = total_before.saturating_sub(keep_last);
                            let save_result = {
                                let mut session = self.session.lock().await;
                                session.history.compact(keep_last);
                                let live_estimate =
                                    crate::session::tokens::TokenCounter::count_messages(
                                        session.history.messages(),
                                    );
                                session.cost_tracker.last_prompt_tokens = live_estimate;
                                session.cost_tracker.last_output_tokens = 0;
                                if self.config.general.auto_save_sessions {
                                    session.save()
                                } else {
                                    Ok(())
                                }
                            };
                            let _ = self.event_tx.send(AgentEvent::HistoryCompacted {
                                kept: keep_last,
                                removed,
                                summary_preview: String::new(),
                                succeeded: false,
                                automatic: true,
                                elapsed_ms: compaction_started_at.elapsed().as_millis() as u64,
                            });
                            if let Err(save_err) = save_result {
                                let _ = self.event_tx.send(AgentEvent::Error(format!(
                                    "Auto-compact fallback history could not be saved to disk: {save_err}"
                                )));
                            }
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
                let _provider = router.active()?;

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

                // ── Live task plan ───────────────────────────────────────
                // Inject the persisted plan so the model always knows what is
                // done / in progress / pending and can keep ticking steps off
                // via `update_plan` instead of restarting the plan each turn.
                {
                    let plan = crate::session::TaskPlan::load(&session_id);
                    if !plan.is_empty() {
                        system.push_str("\n\n## Current Task Plan (persistent)\n");
                        system.push_str(&plan.to_prompt_block());
                        system.push_str(
                            "\nKeep this plan current: call `update_plan` to mark the step you are \
                             starting as `in_progress`, mark finished steps `completed`, and add or \
                             revise steps as the work evolves. Do not leave a completed step marked \
                             in_progress, and keep exactly one step in_progress at a time.",
                        );
                    }
                }

                let tools = if router.active_supports_tools() {
                    Some(self.tools.all_definitions())
                } else {
                    None
                };

                // ── MCP context block ────────────────────────────────────
                // When MCP servers are connected, the model gets tools like
                // `mcp__github__search_repositories`. Without context the
                // model treats them as generic API endpoints — it asks the
                // user for their GitHub username, then calls
                // search_repositories with `user:USERNAME` (literal
                // placeholder), and GitHub returns 422.
                //
                // Each MCP server is connected with the user's own
                // credentials (PAT, OAuth token, API key). The model needs
                // to know:
                //   1. these tools represent the user's authenticated
                //      identity — "my repos" should map to a list-mine tool
                //   2. prefer authenticated-user / "list_*_for_authenticated_user"
                //      tools over generic search when the user refers to
                //      their own data
                //   3. never substitute literal placeholders like USERNAME,
                //      EMAIL, ME — fail the call instead and ask the user.
                if let Some(defs) = &tools {
                    let mcp_servers: std::collections::BTreeMap<
                        String,
                        Vec<&crate::types::ToolDefinition>,
                    > = defs
                        .iter()
                        .filter_map(|d| {
                            d.name.strip_prefix("mcp__").and_then(|rest| {
                                rest.split_once("__").map(|(server, _)| (server, d))
                            })
                        })
                        .fold(std::collections::BTreeMap::new(), |mut acc, (server, d)| {
                            acc.entry(server.to_string()).or_default().push(d);
                            acc
                        });
                    if !mcp_servers.is_empty() {
                        system.push_str(
                            "\n\n## MCP Servers (active, authenticated)\n\
                             The following Model Context Protocol servers are \
                             currently connected. Each one runs as a child \
                             process configured with THE USER's own \
                             credentials (personal access token, OAuth \
                             token, API key, etc.). Every call to one of its \
                             tools acts on behalf of the user's account on \
                             that service — the server already knows who \
                             the user is from the credential.\n\n\
                             ### Rule 1: first-person words refer to the \
                             authenticated identity, never to literal field \
                             values\n\
                             When the user writes \"my\", \"me\", \"mine\", \
                             \"I\", \"myself\", \"my account\", they are \
                             referring to the credential-holder of the \
                             relevant MCP server. These words are NEVER \
                             usernames, owners, account ids, email \
                             addresses, or any other field value. You must \
                             not copy them into tool arguments.\n\n\
                             ### Rule 2: prefer the no-argument \
                             authenticated-user tool\n\
                             For every MCP server that exposes user-owned \
                             data, there is almost always a tool that lists \
                             or fetches the authenticated user's data \
                             without taking a username/owner argument. \
                             Read the tool descriptions in the tool list \
                             and pick that one when the user refers to \
                             their own data. Do NOT default to a `search_*` \
                             tool that requires a query string.\n\n\
                             ### Rule 3: concrete anti-pattern (the trap to \
                             avoid)\n\
                             User asks: \"List all my repos on github.\"\n\
                             ❌ WRONG: call `mcp__github__search_repositories` \
                             with `query: \"user:me\"`. The literal word \
                             \"me\" is not the user — it is a real GitHub \
                             account belonging to a different person, and \
                             the call will return THAT person's public \
                             repositories. The same trap exists with \
                             `user:my`, `owner:me`, `from:me` on any \
                             service. Search qualifiers built from English \
                             pronouns are almost always wrong.\n\
                             ✅ RIGHT: call \
                             `mcp__github__list_repositories_for_authenticated_user` \
                             (or whatever the equivalent no-argument tool \
                             is on that server) and pass no `user`/`owner` \
                             argument. The credential identifies the user \
                             for you.\n\
                             The same shape applies to every other MCP \
                             server: \"my issues\" → list-my-issues tool, \
                             \"my messages\" → list-my-messages tool, \
                             \"my projects\" → list-my-projects tool.\n\n\
                             ### Rule 4: never substitute literal \
                             placeholders\n\
                             Words like `USERNAME`, `OWNER`, `EMAIL`, \
                             `YOUR_TOKEN`, `YOUR_USERNAME` are placeholders \
                             from documentation — not values. Never paste \
                             them into a tool argument. If you genuinely \
                             need a value the user has not given you, \
                             either pick an authenticated-user tool that \
                             does not require it, or call `ask_user` to \
                             collect it. A 422 Validation error from the \
                             server almost always means you passed a \
                             placeholder.\n\n\
                             ### Rule 5: do not fall back to web search \
                             when an MCP server can answer\n\
                             If a connected MCP server covers the domain \
                             (github, slack, gmail, linear, notion, etc.), \
                             pick one of its tools instead of \
                             `web_search` or `web_fetch`. The MCP tool is \
                             authenticated, structured, and authoritative; \
                             web search returns generic public pages.\n\n\
                             ### Rule 6: fuzzy discovery before asking the user\n\
                             When the user names an object imprecisely \
                             (for example \"analyse my github for this repo: \
                             Neural Machine Translation\"), first use the \
                             authenticated MCP server to discover likely \
                             matches. For GitHub-like servers, list the \
                             authenticated user's repositories first, then \
                             compare the user's text against repository \
                             `name`, `full_name`, description, topics, and \
                             common normalized variants: lowercase, \
                             punctuation removed, hyphen/underscore/space \
                             substitutions, acronym words, and partial word \
                             order. If the list is too large or no clear \
                             local match appears, then use the server's \
                             repository/code search tools with multiple \
                             concrete query variants derived from the user's \
                             phrase. Only ask the user after the MCP-backed \
                             fuzzy search leaves several plausible matches \
                             with no defensible winner.\n\n\
                             ### Rule 7: once an MCP object is identified, \
                             stay on that server for follow-up operations\n\
                             After finding a repository, channel, page, \
                             ticket, document, or database through MCP, use \
                             that same MCP server for the requested analysis \
                             or mutation whenever it has the needed tool. Do \
                             not switch to generic local or web tools unless \
                             the MCP server lacks the capability or the user \
                             explicitly asks for local workspace work.\n\n\
                             **Connected servers:**\n",
                        );
                        for (server, server_tools) in &mcp_servers {
                            system.push_str(&format!(
                                "- `{}` — {} tool(s) registered as \
                                 `mcp__{}__*`. Read each tool's description \
                                 in the tool list below; pick the one whose \
                                 description matches what the user is asking \
                                 for.\n",
                                server,
                                server_tools.len(),
                                server,
                            ));
                        }
                    }
                }

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

            // 7. Record usage + emit a per-turn-step delta so IDEs can show
            //    "this turn cost $X" without subtracting cumulative snapshots.
            {
                let mut session = self.session.lock().await;
                let router = self.provider_router.read().await;
                let (input_cost, output_cost) = router.active_costs();
                session.record_usage(&response.usage, input_cost, output_cost);
                let u = &response.usage;
                let cache_read = u.cache_read_tokens.unwrap_or(0);
                let cache_write = u.cache_write_tokens.unwrap_or(0);
                // Cost-per-token math mirrors session::tokens (uncached only —
                // cached tokens have their own multipliers tracked centrally).
                let step_cost = (u.input_tokens as f64 / 1_000_000.0) * input_cost
                    + (u.output_tokens as f64 / 1_000_000.0) * output_cost;
                let _ = self.event_tx.send(AgentEvent::TurnUsage {
                    input: u.input_tokens,
                    output: u.output_tokens,
                    cache_read,
                    cache_write,
                    cost_usd: step_cost,
                });
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
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                });
                Self::maybe_emit_diff_preview(&self.event_tx, &tc, &ctx).await;

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
                let lsp = self.lsp.clone();

                let join_tc = tc.clone();
                futs.push((
                    idx,
                    join_tc,
                    tokio::spawn(async move {
                        let output = match AssertUnwindSafe(Self::execute_single(
                            executor.as_ref(),
                            &tc,
                            ctx.as_ref(),
                            tools.as_ref(),
                            &store_arc,
                            &cancel,
                            &hooks_cfg,
                            working_dir.clone(),
                            sid,
                            event_tx.clone(),
                            perm_tx,
                        ))
                        .catch_unwind()
                        .await
                        {
                            Ok(output) => output,
                            Err(payload) => ToolOutput::error(format!(
                                "Tool '{}' panicked: {}",
                                tc.name,
                                crate::tools::executor::panic_message(payload)
                            )),
                        };
                        let output = Self::append_lsp_post_write_diagnostics(
                            &lsp,
                            &tc,
                            output,
                            &working_dir,
                        )
                        .await;

                        let _ = event_tx.send(AgentEvent::ToolEnd {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            output: output.content.clone(),
                            is_error: output.is_error,
                        });

                        (idx, tc, output)
                    }),
                ));
            }

            for (idx, tc, f) in futs {
                match f.await {
                    Ok(triple) => results.push(triple),
                    Err(e) => {
                        let output =
                            ToolOutput::error(format!("Tool '{}' task failed: {e}", tc.name));
                        let _ = self.event_tx.send(AgentEvent::ToolEnd {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            output: output.content.clone(),
                            is_error: true,
                        });
                        results.push((idx, tc, output));
                    }
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
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
            Self::maybe_emit_diff_preview(&self.event_tx, &tc, &ctx).await;
            let output = match AssertUnwindSafe(Self::execute_single(
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
            ))
            .catch_unwind()
            .await
            {
                Ok(output) => output,
                Err(payload) => ToolOutput::error(format!(
                    "Tool '{}' panicked: {}",
                    tc.name,
                    crate::tools::executor::panic_message(payload)
                )),
            };
            let output =
                Self::append_lsp_post_write_diagnostics(&self.lsp, &tc, output, working_dir_pb)
                    .await;
            let _ = self.event_tx.send(AgentEvent::ToolEnd {
                id: tc.id.clone(),
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

    async fn append_lsp_post_write_diagnostics(
        lsp: &SharedLspManager,
        tc: &ToolCall,
        mut output: ToolOutput,
        working_dir: &std::path::Path,
    ) -> ToolOutput {
        if output.is_error {
            return output;
        }
        let Some(path) = lsp_diagnostic_target(tc, working_dir) else {
            return output;
        };
        if !path.exists() || lsp.language_for_path(&path).is_none() {
            return output;
        }

        let client =
            match tokio::time::timeout(Duration::from_secs(8), lsp.client_for_path(&path)).await {
                Ok(Ok(client)) => client,
                _ => return output,
            };

        let diagnostics = match client
            .diagnostics_for(&path, Duration::from_millis(1200))
            .await
        {
            Ok(diagnostics) => diagnostics,
            Err(_) => return output,
        };

        let rel = path
            .strip_prefix(working_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        if diagnostics.is_empty() {
            output.content.push_str(&format!(
                "\n\nLSP post-edit check ({}) for {}: no diagnostics reported.",
                client.spec.language, rel
            ));
            return output;
        }

        output.content.push_str(&format!(
            "\n\nLSP post-edit check ({}) for {}: {} diagnostic(s):\n",
            client.spec.language,
            rel,
            diagnostics.len()
        ));
        for diag in diagnostics.iter().take(12) {
            let label = match diag.severity {
                Some(1) => "error",
                Some(2) => "warning",
                Some(3) => "info",
                Some(4) => "hint",
                _ => "diag",
            };
            output.content.push_str(&format!(
                "  {}:{}: {}: {}\n",
                diag.range.start.line + 1,
                diag.range.start.character + 1,
                label,
                diag.message.lines().next().unwrap_or(&diag.message)
            ));
        }
        if diagnostics.len() > 12 {
            output.content.push_str(&format!(
                "  ... {} more diagnostic(s) omitted\n",
                diagnostics.len() - 12
            ));
        }
        output
    }

    async fn apply_special_tool_effects(&self, tc: &ToolCall, output: &ToolOutput) {
        // ── Live task planner ────────────────────────────────────────────
        // `update_plan` persists the plan to disk and returns the full plan in
        // its metadata. Re-emit it to the TUI so the checklist ticks off live.
        if tc.name == "update_plan" && !output.is_error {
            if let Some(plan_value) = output
                .metadata
                .as_ref()
                .and_then(|m| m.get("plan_update"))
                .cloned()
            {
                if let Ok(plan) = serde_json::from_value::<crate::session::TaskPlan>(plan_value) {
                    let _ = self.event_tx.send(AgentEvent::PlanUpdated { plan });
                }
            }
            return;
        }

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

    /// Compute and emit a `DiffPreview` event for file-mutation tools so
    /// IDEs can show a native diff editor alongside the permission prompt.
    /// Errors are swallowed — the permission flow still works without it.
    async fn maybe_emit_diff_preview(
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
        tc: &ToolCall,
        ctx: &ToolContext,
    ) {
        if !crate::tools::executor::is_file_mutation_tool(&tc.name) {
            return;
        }
        let preview =
            match crate::tools::fs::preview_file_tool_change(&tc.name, &tc.input, ctx).await {
                Some(p) => p,
                None => return,
            };
        // Derive `path` from the tool's input — every file-mutation tool
        // takes either "path" (write/edit/create/delete) or "source"/"destination"
        // (copy/move). The IDE just needs something to label the diff view with.
        let path = tc
            .input
            .get("path")
            .and_then(|v| v.as_str())
            .or_else(|| tc.input.get("destination").and_then(|v| v.as_str()))
            .or_else(|| tc.input.get("source").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let _ = event_tx.send(AgentEvent::DiffPreview {
            tool_call_id: tc.id.clone(),
            path,
            unified_diff: preview,
        });
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
        let input_summary = crate::agent::permissions::tool_input_summary(&tc.name, &tc.input);
        let input_value = tc.input.clone();
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
                        input_summary,
                        description: desc,
                        level,
                        input: input_value,
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
                    match event {
                        StreamEvent::Token(t) => {
                            did_stream_clone.store(true, Ordering::Release);
                            let _ = event_tx_clone.send(AgentEvent::Token(t));
                        }
                        StreamEvent::ThinkingDelta(t) => {
                            let _ = event_tx_clone.send(AgentEvent::ThinkingDelta { text: t });
                        }
                        StreamEvent::ThinkingDone => {
                            let _ = event_tx_clone.send(AgentEvent::ThinkingEnd);
                        }
                        _ => {}
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
