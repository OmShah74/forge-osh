pub mod diff;
pub mod help;
pub mod input;
pub mod picker;
pub mod renderer;
pub mod spinner;
pub mod themes;

use std::sync::Arc;
use crate::agent::Coordinator;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::agent::{AgentEvent, AgentLoop, PermissionRequest};
use crate::config::{self, Config};
use crate::config::keyring::KeyStore;
use crate::graph::{CodeGraph, GraphBuildMsg, SharedGraph};
use crate::provider::router::ProviderRouter;
use crate::session::Session;
use crate::tools::ToolRegistry;
use crate::types::PermissionResponse;

use input::{Action, InputState};
use picker::PickerState;
use spinner::SpinnerState;
use themes::Theme;

/// Messages displayed in the conversation view
#[derive(Debug, Clone)]
pub struct RenderedMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum MessageRole {
    User,
    Assistant,
    ToolCall { name: String },
    ToolResult { is_error: bool, tool_name: String },
    System,
    /// ASCII-art splash shown once at startup
    Splash,
}

/// OSH block-letter banner. Rendered line-by-line in render_conversation
/// so the box and letters can be coloured differently.
pub const OSH_SPLASH_LINES: &[&str] = &[
    "  ╔══════════════════════════════════════════════════════════╗",
    "  ║                                                          ║",
    "  ║     ######    ########   ##      ##                      ║",
    "  ║    ##    ##  ##      ##  ##      ##                      ║",
    "  ║   ##      ##  ##         ##      ##                      ║",
    "  ║   ##      ##   ########  ##########                      ║",
    "  ║   ##      ##         ##  ##      ##                      ║",
    "  ║    ##    ##  ##      ##  ##      ##                      ║",
    "  ║     ######    ########   ##      ##                      ║",
    "  ║                                                          ║",
    "  ╠══════════════════════════════════════════════════════════╣",
    "  ║   forge-osh  ─  universal ai coding agent                ║",
    "  ║   provider-agnostic  ─  rust-powered  ─  open source     ║",
    "  ╚══════════════════════════════════════════════════════════╝",
];

#[derive(Debug)]
pub enum Modal {
    Confirmation {
        tool_name: String,
        description: String,
        response_tx: tokio::sync::oneshot::Sender<PermissionResponse>,
    },
    Help,
    Picker(PickerState),
    TokenInfo,
    KeyManager(KeyManagerState),
    /// Custom model ID input (opened from model picker)
    CustomModelInput {
        provider_id: String,
        input_buffer: String,
    },
    /// Session browser — list, load, delete past sessions
    SessionBrowser(SessionBrowserState),
}

/// State for the API key manager modal
#[derive(Debug)]
pub struct KeyManagerState {
    pub providers: Vec<KeyManagerEntry>,
    pub selected: usize,
    pub editing: bool,
    pub input_buffer: String,
}

#[derive(Debug, Clone)]
pub struct KeyManagerEntry {
    pub provider_id: String,
    pub provider_name: String,
    pub has_key: bool,
    pub key_source: String, // "stored", "env", "none"
}

impl KeyManagerState {
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected < self.providers.len().saturating_sub(1) {
            self.selected += 1;
        }
    }

    pub fn selected_provider(&self) -> Option<&KeyManagerEntry> {
        self.providers.get(self.selected)
    }
}

/// State for the session browser modal
#[derive(Debug)]
pub struct SessionBrowserState {
    pub sessions: Vec<crate::session::checkpoint::SessionSummary>,
    pub selected: usize,
    /// When `Some(id)`, user pressed `d` and we show a confirmation prompt
    pub confirm_delete: Option<String>,
}

impl SessionBrowserState {
    pub fn new(sessions: Vec<crate::session::checkpoint::SessionSummary>) -> Self {
        Self { sessions, selected: 0, confirm_delete: None }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 { self.selected -= 1; }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.sessions.len() { self.selected += 1; }
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.sessions.get(self.selected).map(|s| s.id.as_str())
    }
}

/// Main application state for the TUI
pub struct AppState {
    pub messages: Vec<RenderedMessage>,
    pub input: InputState,
    pub modal: Option<Modal>,
    pub spinner: SpinnerState,
    /// Absolute index of the first visible line (from top). Anchored when
    /// auto_scroll is false so new content below never moves the viewport.
    pub scroll_top: usize,
    pub auto_scroll: bool,
    pub total_lines: usize,
    pub visible_height: usize,
    pub streaming_text: String,
    /// Guard against committing the same streaming text twice. Stores the
    /// hash of the last text that was committed as an Assistant message.
    /// Cleared on each ThinkingStart so a new turn can legitimately produce
    /// text that happens to match a previous turn.
    pub last_committed_hash: u64,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub session_name: String,
    pub format_tokens: String,
    pub format_cost: String,
    pub trust_mode: bool,
    pub theme: Theme,
    pub theme_name: String,
    pub running: bool,
    pub agent_busy: bool,
    pub agent_task: Option<tokio::task::JoinHandle<()>>,
    pub key_save_pending: Option<(String, String)>,
    pub key_delete_pending: Option<String>,
    pub model_switch_pending: Option<(String, String)>,
    pub vim_normal_mode: bool,
    pub fast_mode: bool,
    /// Pending session load (set by session browser, executed in main loop)
    pub session_load_pending: Option<String>,
    /// Pending session delete (set by session browser, executed in main loop)
    pub session_delete_pending: Option<String>,
    /// Context window usage 0–100 %
    pub context_pct: u8,
    /// Context window size in tokens (set from provider info)
    pub context_limit: u32,
    /// Messages added while the user was scrolled away from the bottom
    pub unread_count: usize,
    /// Per-tool call count for /stats
    pub tool_stats: std::collections::HashMap<String, usize>,

    // ── Multithread Coordinator-Worker Architecture ─────────────────────
    /// When true, user prompts are routed through the Coordinator which
    /// spawns parallel Worker tasks. When false (default), everything uses
    /// the standard monolithic AgentLoop::run().
    pub multithread_mode: bool,
    /// The coordinator instance (always created, but only active when
    /// multithread_mode is true).
    pub coordinator: Option<Coordinator>,

    // ── forge-graph ────────────────────────────────────────────────────────
    /// Shared semantic code graph (shared with AgentLoop and GraphQueryTool)
    pub shared_graph: SharedGraph,
    /// Progress messages received from the background graph build thread
    pub graph_build_rx: Option<std::sync::mpsc::Receiver<GraphBuildMsg>>,
}

impl AppState {
    pub fn new(config: &Config, session: &Session, shared_graph: SharedGraph) -> Self {
        Self {
            messages: Vec::new(),
            input: InputState::new(),
            modal: None,
            spinner: SpinnerState::new(),
            scroll_top: 0,
            auto_scroll: true,
            total_lines: 0,
            visible_height: 0,
            streaming_text: String::new(),
            last_committed_hash: 0,
            provider_id: session.provider_id.clone(),
            provider_name: session.provider_id.clone(),
            model_id: session.model_id.clone(),
            model_name: session.model_id.clone(),
            session_name: session.name.clone(),
            format_tokens: "0 tokens".to_string(),
            format_cost: "$0.00".to_string(),
            trust_mode: config.general.trust_mode,
            theme: Theme::from_name(&config.general.theme),
            theme_name: config.general.theme.clone(),
            running: true,
            agent_busy: false,
            agent_task: None,
            key_save_pending: None,
            key_delete_pending: None,
            model_switch_pending: None,
            vim_normal_mode: false,
            fast_mode: false,
            session_load_pending: None,
            session_delete_pending: None,
            context_pct: 0,
            context_limit: 128_000,
            unread_count: 0,
            tool_stats: std::collections::HashMap::new(),
            multithread_mode: false,
            coordinator: None,
            shared_graph,
            graph_build_rx: None,
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        if self.auto_scroll {
            // First scroll up from bottom: anchor starting at current bottom.
            self.scroll_top = self.max_scroll().saturating_sub(n);
        } else {
            self.scroll_top = self.scroll_top.saturating_sub(n);
        }
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self, n: usize) {
        if self.auto_scroll {
            return; // already at bottom, nothing to do
        }
        self.scroll_top = self.scroll_top.saturating_add(n);
        if self.scroll_top >= self.max_scroll() {
            self.auto_scroll = true;
            self.unread_count = 0;
        }
    }

    pub fn max_scroll(&self) -> usize {
        self.total_lines.saturating_sub(self.visible_height)
    }

    pub fn effective_scroll(&self) -> usize {
        if self.auto_scroll {
            // Always show the bottom (follows new content automatically).
            self.max_scroll()
        } else {
            // Anchored absolute position — clamp in case content shrank.
            self.scroll_top.min(self.max_scroll())
        }
    }

    /// Push a system message
    pub fn push_system(&mut self, msg: impl Into<String>) {
        self.messages.push(RenderedMessage {
            role: MessageRole::System,
            content: msg.into(),
        });
        // Do NOT reset scroll here — let user keep their scroll position
    }
}

// ---------------------------------------------------------------------------
// Slash command handling
// ---------------------------------------------------------------------------

/// Handle a /command string typed at the prompt.
/// Returns true if the command was recognised (so the caller does NOT forward it to the agent).
async fn handle_slash_command(
    text: &str,
    state: &mut AppState,
    session: &Arc<Mutex<Session>>,
    provider_router: &Arc<RwLock<ProviderRouter>>,
    key_store: &Arc<Mutex<KeyStore>>,
    _config: &Arc<Config>,
) -> bool {
    // Delegate /forge-graph before reaching the big match
    if text.trim_start().starts_with("/forge-graph") {
        cmd_forge_graph(state, session, text.trim()).await;
        return true;
    }
    // Split into command name and optional argument
    let (cmd, arg) = {
        let t = text.trim();
        if let Some(space) = t.find(' ') {
            (&t[..space], t[space + 1..].trim())
        } else {
            (t, "")
        }
    };

    match cmd {
        "/help" | "/?" => {
            state.modal = Some(Modal::Help);
        }

        "/clear" | "/cls" => {
            state.messages.clear();
            state.streaming_text.clear();
            state.scroll_top = 0;
            state.auto_scroll = true;
            state.push_system("Conversation display cleared. History is still in memory.");
        }

        "/quit" | "/exit" | "/q" => {
            state.running = false;
        }

        "/cost" | "/tokens" => {
            state.modal = Some(Modal::TokenInfo);
        }

        "/model" => {
            let pid = state.provider_id.clone();
            let pname = state.provider_name.clone();
            if arg.is_empty() {
                // Open picker
                let models = crate::config::models::models_for_provider(&pid);
                let mut items: Vec<picker::PickerItem> = models
                    .iter()
                    .map(|m| picker::PickerItem::from_model_info(m, true, &pname))
                    .collect();
                // Always add "Add custom model" entry at the bottom
                items.push(picker::PickerItem {
                    provider_id: pid.clone(),
                    provider_name: pname.clone(),
                    model_id: "__add_custom__".to_string(),
                    model_name: "+ Add custom model...".to_string(),
                    context_window: 0,
                    cost_display: "enter any model ID".to_string(),
                    connected: true,
                });
                state.modal = Some(Modal::Picker(PickerState::new(items)));
            } else if arg == "list" {
                // List available models for current provider
                let models = crate::config::models::models_for_provider(&pid);
                if models.is_empty() {
                    state.push_system(format!("No models found for provider '{pname}'."));
                } else {
                    let mut lines = vec![format!("Models for provider '{pname}':")];
                    for m in &models {
                        lines.push(format!("  {} — {}", m.id, m.name));
                    }
                    state.push_system(lines.join("\n"));
                }
            } else {
                // Direct model switch: /model <model-id>
                // First try catalog match, then fall back to using the ID directly (custom model)
                let models = crate::config::models::models_for_provider(&pid);
                let found = models.iter().find(|m| m.id == arg || m.name.to_lowercase().contains(&arg.to_lowercase()));
                if let Some(m) = found {
                    let model_id = m.id.clone();
                    let model_name = m.name.clone();
                    state.model_switch_pending = Some((pid.clone(), model_id.clone()));
                    state.model_id = model_id.clone();
                    state.model_name = model_name.clone();
                    state.push_system(format!("Switched to model: {} ({})", model_name, model_id));
                } else {
                    // Not in catalog — use as custom model ID directly
                    let model_id = arg.to_string();
                    state.model_switch_pending = Some((pid.clone(), model_id.clone()));
                    state.model_id = model_id.clone();
                    state.model_name = model_id.clone();
                    state.push_system(format!("Switched to custom model: {model_id} (not in catalog — make sure the ID is correct)"));
                }
            }
        }

        "/provider" => {
            let router = provider_router.read().await;
            let mut items = Vec::new();
            for (pid, pname) in router.available_providers() {
                let models = crate::config::models::models_for_provider(pid);
                let item = if let Some(m) = models.first() {
                    picker::PickerItem::from_model_info(m, true, pname)
                } else {
                    picker::PickerItem {
                        provider_id: pid.to_string(),
                        provider_name: pname.to_string(),
                        model_id: String::new(),
                        model_name: "(default)".to_string(),
                        context_window: 0,
                        cost_display: String::new(),
                        connected: true,
                    }
                };
                items.push(item);
            }
            if items.is_empty() {
                state.push_system("No providers configured. Use /keys to add an API key.");
            } else {
                state.modal = Some(Modal::Picker(PickerState::new(items)));
            }
        }

        "/keys" | "/key" | "/apikey" => {
            let ks = key_store.lock().await;
            let mut entries = Vec::new();
            for pid in config::cloud_provider_ids() {
                let env_var = crate::config::keyring::provider_env_var(pid);
                let has_env = std::env::var(&env_var).is_ok();
                let has_stored = ks.list_providers().contains(&pid.to_string());
                let has_key = has_env || has_stored;
                let source = if has_env && has_stored {
                    "env+stored".to_string()
                } else if has_env {
                    "env".to_string()
                } else if has_stored {
                    "stored".to_string()
                } else {
                    "none".to_string()
                };
                entries.push(KeyManagerEntry {
                    provider_id: pid.to_string(),
                    provider_name: pid.to_string(),
                    has_key,
                    key_source: source,
                });
            }
            state.modal = Some(Modal::KeyManager(KeyManagerState {
                providers: entries,
                selected: 0,
                editing: false,
                input_buffer: String::new(),
            }));
        }

        "/theme" => {
            if arg.is_empty() {
                // Cycle to next theme
                let next = Theme::next_theme_name(&state.theme_name);
                state.theme = Theme::from_name(next);
                state.theme_name = next.to_string();
                state.push_system(format!("Theme: {next}"));
            } else {
                // Set specific theme
                state.theme = Theme::from_name(arg);
                state.theme_name = arg.to_string();
                state.push_system(format!("Theme: {arg}"));
            }
        }

        "/trust" => {
            state.trust_mode = !state.trust_mode;
            state.push_system(format!(
                "Trust mode: {}",
                if state.trust_mode { "ON  (all tool permissions auto-approved)" } else { "OFF  (tool permissions will be prompted)" }
            ));
        }

        "/compact" => {
            // LLM-based compact: summarize old messages using the active provider.
            let keep = if !arg.is_empty() {
                arg.parse().unwrap_or(crate::agent::compaction::DEFAULT_KEEP_LAST)
            } else {
                crate::agent::compaction::DEFAULT_KEEP_LAST
            };
            compact_history_llm(state, session, provider_router, keep).await;
        }

        "/undo" => {
            let msg = crate::agent::file_history::undo_last().await;
            state.push_system(msg);
        }

        "/new" => {
            // Start a fresh conversation in this session (clear history + display).
            {
                let mut sess = session.lock().await;
                sess.history.clear();
            }
            state.messages.retain(|m| matches!(m.role, MessageRole::Splash));
            state.push_system("New conversation started. History cleared.");
        }

        "/save" => {
            let sess = session.lock().await;
            match sess.save() {
                Ok(_) => state.push_system("Session saved."),
                Err(e) => state.push_system(format!("Failed to save session: {e}")),
            }
        }

        "/session" | "/info" => {
            let sess = session.lock().await;
            let msg_count = sess.history.messages().len();
            state.push_system(format!(
                "Session: {}  |  ID: {}  |  Messages: {}  |  Model: {}  |  Tokens: {}  |  Cost: {}",
                sess.name,
                &sess.id[..8],
                msg_count,
                sess.model_id,
                state.format_tokens,
                state.format_cost,
            ));
        }

        "/sessions" | "/history" => {
            match crate::session::checkpoint::Checkpoint::list() {
                Ok(sessions) if sessions.is_empty() => {
                    state.push_system("No saved sessions found. Use /save to save the current session.");
                }
                Ok(sessions) => {
                    state.modal = Some(Modal::SessionBrowser(SessionBrowserState::new(sessions)));
                }
                Err(e) => {
                    state.push_system(format!("Failed to list sessions: {e}"));
                }
            }
        }

        // ── /commit — AI-generated commit message ──────────────────────────
        "/commit" => {
            cmd_commit(state, session).await;
        }

        // ── /diff — show git diff ──────────────────────────────────────────
        "/diff" => {
            let working_dir = {
                let sess = session.lock().await;
                sess.working_dir.clone()
            };
            let staged = if arg == "staged" || arg == "--staged" { "--staged" } else { "" };
            let args: Vec<&str> = if staged.is_empty() {
                vec!["diff", "--stat"]
            } else {
                vec!["diff", "--staged", "--stat"]
            };
            match std::process::Command::new("git").args(&args).current_dir(&working_dir).output() {
                Ok(out) => {
                    let diff = String::from_utf8_lossy(&out.stdout).to_string();
                    if diff.trim().is_empty() {
                        state.push_system("No changes in working tree.");
                    } else {
                        state.push_system(format!("Git diff (stat):\n{}", diff.trim()));
                    }
                }
                Err(e) => state.push_system(format!("git diff failed: {e}")),
            }
        }

        // ── /export — export conversation to Markdown ──────────────────────
        "/export" => {
            export_conversation(state, session, arg).await;
        }

        // ── /status — system status overview ──────────────────────────────
        "/status" => {
            let sess = session.lock().await;
            let router = provider_router.read().await;
            let ctx_window = router.active().map(|p| p.context_window()).unwrap_or(128_000);
            let used_tokens: u64 = sess.cost_tracker.total_input_tokens + sess.cost_tracker.total_output_tokens;
            let ctx_pct = (used_tokens as f64 / ctx_window as f64 * 100.0).min(100.0);
            let tools_loaded = crate::tools::ToolRegistry::with_builtins().tool_names().len();
            let permissions = crate::agent::permissions::PermissionStore::load();

            state.push_system(format!(
                "forge-osh Status\n\
                ├─ Provider:     {} ({})\n\
                ├─ Model:        {}\n\
                ├─ Context:      {}/{} tokens ({:.1}% used)\n\
                ├─ Cost:         {}\n\
                ├─ Messages:     {}\n\
                ├─ Tools:        {} loaded\n\
                ├─ Trust mode:   {}\n\
                ├─ Permission rules: {}\n\
                └─ Session:      {} ({})",
                state.provider_name, state.provider_id,
                state.model_name,
                used_tokens, ctx_window, ctx_pct,
                state.format_cost,
                sess.history.message_count(),
                tools_loaded,
                if state.trust_mode { "ON" } else { "OFF" },
                permissions.rules.len(),
                sess.name, &sess.id[..8],
            ));
        }

        // ── /doctor — environment diagnostics ─────────────────────────────
        "/doctor" => {
            cmd_doctor(state, session, provider_router).await;
        }

        // ── /add-dir — add a directory to the session scope ────────────────
        "/add-dir" => {
            if arg.is_empty() {
                state.push_system("Usage: /add-dir <path>  — adds a directory to the session working context");
            } else {
                let path = std::path::PathBuf::from(arg);
                let abs = if path.is_absolute() {
                    path.clone()
                } else {
                    let sess = session.lock().await;
                    std::path::PathBuf::from(&sess.working_dir).join(&path)
                };
                if abs.is_dir() {
                    state.push_system(format!(
                        "Added directory to scope: {}\n\
                        The agent will now consider files in this directory when responding.",
                        abs.display()
                    ));
                    // Store in session state so system prompt can use it
                    let mut sess = session.lock().await;
                    sess.working_dir = abs.to_string_lossy().to_string();
                } else {
                    state.push_system(format!("Directory not found: {}", abs.display()));
                }
            }
        }

        // ── /resume — resume a past session ───────────────────────────────
        "/resume" => {
            cmd_resume(state, session).await;
        }

        // ── /permissions — view/edit permission rules ──────────────────────
        "/permissions" => {
            cmd_permissions(state, arg).await;
        }

        // ── /effort — set response effort level ───────────────────────────
        "/effort" => {
            if arg.is_empty() {
                let current = session.lock().await.effort_level;
                state.push_system(format!(
                    "Current effort: {current}/5. Usage: /effort <1-5>  — 1=minimal, 3=balanced (default), 5=maximum"
                ));
            } else {
                match arg.parse::<u8>() {
                    Ok(n) if (1..=5).contains(&n) => {
                        session.lock().await.effort_level = n;
                        let desc = match n {
                            1 => "minimal — temperature 0.0, most deterministic",
                            2 => "low — temperature 0.3",
                            3 => "balanced — temperature 0.7 (default)",
                            4 => "high — temperature 1.0",
                            5 => "maximum — temperature 1.2, most creative",
                            _ => "balanced",
                        };
                        state.push_system(format!("Effort level set to {n}/5 ({desc})."));
                    }
                    _ => state.push_system("Invalid effort level. Use a number 1-5."),
                }
            }
        }

        // ── /copy — copy last response to clipboard ────────────────────────
        "/copy" => {
            // Find last assistant message
            let last_response = state.messages.iter().rev()
                .find(|m| matches!(m.role, MessageRole::Assistant))
                .map(|m| m.content.clone());

            match last_response {
                Some(text) => {
                    // Try to copy to clipboard using platform tools
                    let copied = try_copy_to_clipboard(&text);
                    if copied {
                        state.push_system(format!(
                            "Copied last response to clipboard ({} chars).", text.len()
                        ));
                    } else {
                        state.push_system(format!(
                            "Clipboard not available. Last response ({} chars):\n\n{}",
                            text.len(),
                            if text.len() > 500 { &text[..500] } else { &text }
                        ));
                    }
                }
                None => state.push_system("No assistant response to copy yet."),
            }
        }

        "/vim" => {
            state.vim_normal_mode = !state.vim_normal_mode;
            state.push_system(format!(
                "Vim mode: {}  ({})",
                if state.vim_normal_mode { "ON" } else { "OFF" },
                if state.vim_normal_mode {
                    "j/k scroll, d/u half-page, g/G top/bottom, i/a to insert"
                } else {
                    "normal input mode"
                }
            ));
        }

        "/multithread" | "/mt" => {
            if arg == "status" || arg == "info" {
                // Show multithread status
                if state.multithread_mode {
                    if let Some(ref coord) = state.coordinator {
                        let workers = coord.list_workers();
                        if workers.is_empty() {
                            state.push_system("Multithread mode: ON  |  No active workers.");
                        } else {
                            let mut lines = vec![format!("Multithread mode: ON  |  {} active worker(s):", workers.len())];
                            for (id, desc) in &workers {
                                lines.push(format!("  • {id} — {desc}"));
                            }
                            state.push_system(lines.join("\n"));
                        }
                    } else {
                        state.push_system("Multithread mode: ON  (coordinator not initialized)");
                    }
                } else {
                    state.push_system("Multithread mode: OFF  (use /multithread to enable)");
                }
            } else if arg == "stop" {
                // Stop all workers
                if let Some(ref mut coord) = state.coordinator {
                    coord.stop_all();
                    state.push_system("All workers stopped.");
                } else {
                    state.push_system("No coordinator active.");
                }
            } else {
                // Toggle on/off
                state.multithread_mode = !state.multithread_mode;
                state.push_system(format!(
                    "Multithread mode: {}\n{}",
                    if state.multithread_mode { "ON" } else { "OFF" },
                    if state.multithread_mode {
                        "Prompts prefixed with @worker will spawn parallel workers.\n\
                        Use /multithread status to see active workers.\n\
                        Use /multithread stop to cancel all workers."
                    } else {
                        "Standard monolithic execution restored."
                    }
                ));
            }
        }

        "/fast" => {
            state.fast_mode = !state.fast_mode;
            state.push_system(format!(
                "Fast mode: {}  ({})",
                if state.fast_mode { "ON" } else { "OFF" },
                if state.fast_mode {
                    "tool results collapsed, streaming optimized"
                } else {
                    "full output display"
                }
            ));
        }

        // ── /init — generate a CLAUDE.md for the current project ─────────────
        "/init" => {
            cmd_init(state, session).await;
        }

        // ── /find — search project files for text ─────────────────────────
        "/find" => {
            if arg.is_empty() {
                state.push_system("Usage: /find <text>  — search all project files for matching text");
            } else {
                cmd_find(state, session, arg).await;
            }
        }

        // ── /config — view or change configuration ────────────────────────
        "/config" => {
            cmd_config(state, arg);
        }

        // ── /stats — detailed session statistics ──────────────────────────
        "/stats" => {
            cmd_stats(state, session).await;
        }

        _ => {
            // Unknown command
            state.push_system(format!(
                "Unknown command: {}  (type /help for a list of commands)",
                cmd
            ));
        }
    }

    true // command was handled
}

// ---------------------------------------------------------------------------
// /commit implementation
// ---------------------------------------------------------------------------
async fn cmd_commit(state: &mut AppState, session: &Arc<Mutex<Session>>) {
    let working_dir = {
        let sess = session.lock().await;
        sess.working_dir.clone()
    };

    // Check if there are staged changes
    let staged_diff = match std::process::Command::new("git")
        .args(["diff", "--staged", "--stat"])
        .current_dir(&working_dir)
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
        Err(e) => {
            state.push_system(format!("/commit: failed to run git: {e}"));
            return;
        }
    };

    if staged_diff.trim().is_empty() {
        // Check if there are any changes at all
        let status = match std::process::Command::new("git")
            .args(["status", "--short"])
            .current_dir(&working_dir)
            .output()
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
            Err(e) => {
                state.push_system(format!("/commit: failed to run git status: {e}"));
                return;
            }
        };

        if status.trim().is_empty() {
            state.push_system("/commit: Nothing to commit — working tree is clean.");
        } else {
            state.push_system(format!(
                "/commit: No staged changes. Stage files first with:\n  git add <file>\n  git add -A\n\nUnstaged changes:\n{}",
                status.trim()
            ));
        }
        return;
    }

    // Get full diff for context
    let full_diff = std::process::Command::new("git")
        .args(["diff", "--staged"])
        .current_dir(&working_dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // Show what's staged
    state.push_system(format!(
        "Staged changes:\n{}\n\nSending to agent to generate commit message...",
        staged_diff.trim()
    ));

    // Build a commit message from the diff using the agent
    // We construct a special internal message
    let prompt = format!(
        "Based on the following git diff, write a concise, informative commit message. \
        Follow conventional commits format if appropriate (feat/fix/refactor/docs/chore etc). \
        Return ONLY the commit message, nothing else.\n\n\
        Staged diff:\n```\n{}\n```",
        if full_diff.len() > 4000 { &full_diff[..4000] } else { &full_diff }
    );

    state.push_system(format!("Suggested: use the agent to generate the commit message by sending:\n> {}", &prompt[..prompt.len().min(200)]));
    state.push_system(
        "Type your commit message (or ask the agent to write one by describing the changes). \
        Then run: bash git commit -m \"<message>\""
    );
}

// ---------------------------------------------------------------------------
// /export implementation
// ---------------------------------------------------------------------------
async fn export_conversation(state: &mut AppState, session: &Arc<Mutex<Session>>, filename: &str) {
    let sess = session.lock().await;
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");

    let outfile = if filename.is_empty() {
        format!("forge-export-{}.md", timestamp)
    } else if filename.ends_with(".md") {
        filename.to_string()
    } else {
        format!("{}.md", filename)
    };

    let mut lines = vec![
        format!("# forge-osh Session Export"),
        format!("**Session:** {}  ", sess.name),
        format!("**Model:** {}  ", sess.model_id),
        format!("**Provider:** {}  ", sess.provider_id),
        format!("**Exported:** {}  ", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
        format!("**Messages:** {}  ", sess.history.message_count()),
        String::new(),
        "---".to_string(),
        String::new(),
    ];

    for msg in &state.messages {
        match &msg.role {
            MessageRole::User => {
                lines.push(format!("### 👤 User\n{}\n", msg.content));
            }
            MessageRole::Assistant => {
                lines.push(format!("### 🤖 Assistant\n{}\n", msg.content));
            }
            MessageRole::ToolCall { name } => {
                lines.push(format!("### ⚙️ Tool: `{}`\n```json\n{}\n```\n", name, msg.content));
            }
            MessageRole::ToolResult { is_error, tool_name } => {
                let label = if *is_error { "❌ Error" } else { "✅ Result" };
                let name_part = if tool_name.is_empty() { String::new() } else { format!(" ({})", tool_name) };
                lines.push(format!("### {}{}\n```\n{}\n```\n", label, name_part, msg.content));
            }
            MessageRole::System => {
                lines.push(format!("*System: {}*\n", msg.content));
            }
            MessageRole::Splash => {} // skip
        }
    }

    let content = lines.join("\n");
    let export_path = std::path::PathBuf::from(&sess.working_dir).join(&outfile);

    drop(sess); // release lock before writing

    match std::fs::write(&export_path, &content) {
        Ok(_) => state.push_system(format!(
            "Conversation exported to: {} ({} chars)", outfile, content.len()
        )),
        Err(e) => state.push_system(format!("Export failed: {e}")),
    }
}

// ---------------------------------------------------------------------------
// /doctor implementation
// ---------------------------------------------------------------------------
async fn cmd_doctor(
    state: &mut AppState,
    session: &Arc<Mutex<Session>>,
    provider_router: &Arc<RwLock<ProviderRouter>>,
) {
    let mut report = vec!["forge-osh Doctor — Environment Diagnostics".to_string(), String::new()];

    // Working directory
    let working_dir = {
        let sess = session.lock().await;
        sess.working_dir.clone()
    };
    let wd_exists = std::path::Path::new(&working_dir).exists();
    report.push(format!("Working Directory: {} {}",
        working_dir,
        if wd_exists { "✓" } else { "✗ NOT FOUND" }
    ));

    // Git availability
    match std::process::Command::new("git").arg("--version").output() {
        Ok(out) => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            report.push(format!("Git: {} ✓", ver));
        }
        Err(_) => report.push("Git: NOT FOUND ✗ (git commands will fail)".to_string()),
    }

    // Shell
    let (shell, _) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    match std::process::Command::new(shell).arg(if cfg!(target_os="windows") { "/C echo test" } else { "-c" }).arg("echo test").output() {
        Ok(_) => report.push(format!("Shell ({}): ✓", shell)),
        Err(e) => report.push(format!("Shell ({}): ✗ {}", shell, e)),
    }

    // Provider connectivity check
    let (provider_id, model_id, available_providers) = {
        let router = provider_router.read().await;
        (
            router.active_provider_id().to_string(),
            router.active_model_id().to_string(),
            router.available_providers()
                .iter()
                .map(|(id, _)| id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        )
    };
    report.push(format!("Active provider: {} / {}", provider_id, model_id));
    report.push(format!("Available providers: {}", available_providers));

    // API key check
    let cfg_dir = crate::config::config_dir();
    let ks = crate::config::keyring::KeyStore::new(&cfg_dir);
    let has_key = ks.list_providers().contains(&provider_id.to_string());
    let env_key = std::env::var(crate::config::keyring::provider_env_var(&provider_id)).is_ok();
    report.push(format!("API key for '{}': {}",
        provider_id,
        if has_key || env_key { "✓ found" } else { "✗ NOT SET (use /keys or set env var)" }
    ));

    // CLAUDE.md memory files
    let mem_path = std::path::PathBuf::from(&working_dir).join("CLAUDE.md");
    report.push(format!("CLAUDE.md (project): {}",
        if mem_path.exists() { format!("✓ found ({})", mem_path.display()) }
        else { "not found (optional)".to_string() }
    ));

    // Hooks config
    let hooks_path = crate::config::config_dir().join("hooks.json");
    report.push(format!("hooks.json: {}",
        if hooks_path.exists() { format!("✓ found ({})", hooks_path.display()) }
        else { "not configured (optional)".to_string() }
    ));

    // Permission rules
    let permissions = crate::agent::permissions::PermissionStore::load();
    report.push(format!("Permission rules: {} stored", permissions.rules.len()));

    // Config dir
    let cfg_dir = crate::config::config_dir();
    report.push(format!("Config directory: {} {}",
        cfg_dir.display(),
        if cfg_dir.exists() { "✓" } else { "✗ NOT FOUND" }
    ));

    report.push(String::new());
    report.push(format!("Platform: {} ({})", std::env::consts::OS, std::env::consts::ARCH));
    report.push(format!("forge-osh v1.0.1  — Batch 1"));

    state.push_system(report.join("\n"));
}

// ---------------------------------------------------------------------------
// /resume implementation
// ---------------------------------------------------------------------------
async fn cmd_resume(state: &mut AppState, _session: &Arc<Mutex<Session>>) {
    let sessions_dir = crate::config::sessions_dir();
    if !sessions_dir.exists() {
        state.push_system("No saved sessions found. Sessions are saved automatically on exit.");
        return;
    }

    let mut session_files: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let mtime = entry.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                session_files.push((path, mtime));
            }
        }
    }

    if session_files.is_empty() {
        state.push_system("No saved sessions found.");
        return;
    }

    session_files.sort_by(|a, b| b.1.cmp(&a.1)); // newest first

    let mut lines = vec!["Saved sessions (most recent first):".to_string()];
    for (i, (path, mtime)) in session_files.iter().take(10).enumerate() {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let datetime: chrono::DateTime<chrono::Local> = (*mtime).into();
        lines.push(format!("  {}. {}  ({})", i + 1, name, datetime.format("%Y-%m-%d %H:%M")));
    }
    lines.push(String::new());
    lines.push("To resume: forge-osh --session <session-id>  or start a new session and use /compact to manage context.".to_string());

    state.push_system(lines.join("\n"));
}

// ---------------------------------------------------------------------------
// /permissions implementation
// ---------------------------------------------------------------------------
async fn cmd_permissions(state: &mut AppState, arg: &str) {
    let mut store = crate::agent::permissions::PermissionStore::load();

    if arg.is_empty() {
        // Display current rules
        state.push_system(store.display());
        state.push_system(
            "Usage:\n  /permissions add bash(git *)     — always allow git commands\n  \
            /permissions deny bash(rm -rf *)  — always deny rm -rf\n  \
            /permissions remove <index>        — remove rule by index\n  \
            /permissions clear                 — remove all rules"
        );
        return;
    }

    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
    match parts[0] {
        "add" | "allow" => {
            if let Some(rule_str) = parts.get(1) {
                if let Some((tool, pattern)) = parse_permission_rule(rule_str) {
                    store.add_allow(&tool, &pattern);
                    state.push_system(format!("Added allow rule: {}({})", tool, pattern));
                } else {
                    state.push_system("Invalid rule format. Use: tool_name(pattern)  e.g. bash(git *)");
                }
            }
        }
        "deny" => {
            if let Some(rule_str) = parts.get(1) {
                if let Some((tool, pattern)) = parse_permission_rule(rule_str) {
                    store.add_deny(&tool, &pattern);
                    state.push_system(format!("Added deny rule: {}({})", tool, pattern));
                } else {
                    state.push_system("Invalid rule format. Use: tool_name(pattern)  e.g. bash(rm -rf *)");
                }
            }
        }
        "remove" | "rm" | "delete" => {
            if let Some(idx_str) = parts.get(1) {
                if let Ok(idx) = idx_str.trim().parse::<usize>() {
                    store.remove(idx);
                    state.push_system(format!("Removed rule at index {idx}."));
                } else {
                    state.push_system("Usage: /permissions remove <index>");
                }
            }
        }
        "clear" => {
            store.rules.clear();
            store.save();
            state.push_system("All permission rules cleared.");
        }
        _ => {
            state.push_system("Unknown subcommand. Use: /permissions [add|deny|remove|clear]");
        }
    }
}

fn parse_permission_rule(s: &str) -> Option<(String, String)> {
    // Format: tool_name(pattern)  e.g.  bash(git *)
    if let Some(open) = s.find('(') {
        if let Some(close) = s.rfind(')') {
            if close > open {
                let tool = s[..open].trim().to_string();
                let pattern = s[open + 1..close].to_string();
                if !tool.is_empty() && !pattern.is_empty() {
                    return Some((tool, pattern));
                }
            }
        }
    }
    // Also support: bash git *  (space-separated, pattern is everything after tool)
    let parts: Vec<&str> = s.splitn(2, ' ').collect();
    if parts.len() == 2 {
        return Some((parts[0].to_string(), parts[1].to_string()));
    }
    None
}

// ---------------------------------------------------------------------------
// Clipboard helper
// ---------------------------------------------------------------------------
fn try_copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        // Use PowerShell to copy to clipboard on Windows
        if let Ok(mut child) = std::process::Command::new("powershell")
            .args(["-Command", &format!("Set-Clipboard -Value '{}'", text.replace('\'', "''"))])
            .spawn()
        {
            let _ = child.wait();
            return true;
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.take() {
                let _ = std::io::BufWriter::new(stdin).write_all(text.as_bytes());
            }
            let _ = child.wait();
            return true;
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Try xclip or xsel on Linux
        use std::io::Write;
        for cmd in &["xclip", "xsel"] {
            let args: &[&str] = if *cmd == "xclip" {
                &["-selection", "clipboard"]
            } else {
                &["--clipboard", "--input"]
            };
            if let Ok(mut child) = std::process::Command::new(cmd)
                .args(args)
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(stdin) = child.stdin.take() {
                    let _ = std::io::BufWriter::new(stdin).write_all(text.as_bytes());
                }
                let _ = child.wait();
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tab completion helper
// ---------------------------------------------------------------------------

/// Complete or list slash commands when the user presses Tab.
fn tab_complete_slash(state: &mut AppState) {
    let text = state.input.text.clone();
    if !text.starts_with('/') || text.contains(' ') {
        return;
    }
    const ALL_COMMANDS: &[&str] = &[
        "/help", "/clear", "/quit", "/exit", "/cost", "/model", "/provider",
        "/keys", "/theme", "/trust", "/vim", "/fast", "/compact", "/undo",
        "/new", "/save", "/session", "/sessions", "/history", "/commit", "/diff", "/export",
        "/status", "/doctor", "/add-dir", "/resume", "/permissions",
        "/effort", "/copy", "/init", "/find", "/config", "/stats",
        "/forge-graph", "/multithread",
    ];
    let prefix = text.as_str();
    let matches: Vec<&str> = ALL_COMMANDS.iter().copied()
        .filter(|c| c.starts_with(prefix) && c.len() > prefix.len())
        .collect();

    match matches.len() {
        0 => {} // no match — do nothing
        1 => {
            // Single match: complete with trailing space
            state.input.text = format!("{} ", matches[0]);
            state.input.cursor = state.input.text.len();
        }
        _ => {
            // Multiple matches: extend to common prefix, then show options
            let common = common_prefix(&matches);
            if common.len() > prefix.len() {
                state.input.text = common.clone();
                state.input.cursor = common.len();
            }
            state.push_system(format!("Commands: {}", matches.join("  ")));
        }
    }
}

fn common_prefix(strings: &[&str]) -> String {
    if strings.is_empty() { return String::new(); }
    let first = strings[0];
    let mut len = first.len();
    for s in &strings[1..] {
        len = len.min(s.len());
        for (i, (a, b)) in first.chars().zip(s.chars()).enumerate() {
            if a != b { len = len.min(i); break; }
        }
    }
    first[..len].to_string()
}

// ---------------------------------------------------------------------------
// /init — generate CLAUDE.md for the project
// ---------------------------------------------------------------------------

async fn cmd_init(state: &mut AppState, session: &Arc<Mutex<Session>>) {
    let working_dir = { let sess = session.lock().await; sess.working_dir.clone() };
    let root = std::path::Path::new(&working_dir);
    let out_path = root.join("CLAUDE.md");

    if out_path.exists() {
        state.push_system(format!(
            "CLAUDE.md already exists at {}. Delete it first to regenerate.",
            out_path.display()
        ));
        return;
    }

    let mut parts: Vec<String> = Vec::new();
    parts.push("# CLAUDE.md\n\nThis file provides guidance to forge-osh when working with this project.\n".into());

    // Detect project type and build commands
    let (lang, build_cmds): (&str, &[&str]) =
        if root.join("Cargo.toml").exists()      { ("Rust",               &["cargo build", "cargo test", "cargo clippy", "cargo fmt"]) }
        else if root.join("package.json").exists()  { ("JavaScript/TypeScript", &["npm install", "npm run build", "npm test"]) }
        else if root.join("pyproject.toml").exists() || root.join("setup.py").exists()
                                                    { ("Python",            &["pip install -e .", "pytest", "ruff check ."]) }
        else if root.join("go.mod").exists()        { ("Go",               &["go build ./...", "go test ./...", "go vet ./..."]) }
        else if root.join("pom.xml").exists()       { ("Java/Maven",       &["mvn compile", "mvn test"]) }
        else if root.join("build.gradle").exists()  { ("Java/Gradle",      &["gradle build", "gradle test"]) }
        else                                        { ("",                  &[]) };

    // Try to read project name from manifest
    let project_name = if root.join("Cargo.toml").exists() {
        std::fs::read_to_string(root.join("Cargo.toml")).ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("name")).and_then(|l| l.split('"').nth(1)).map(str::to_string))
    } else if root.join("package.json").exists() {
        std::fs::read_to_string(root.join("package.json")).ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["name"].as_str().map(str::to_string))
    } else {
        None
    };

    let name_part = project_name.map(|n| format!("{} — ", n)).unwrap_or_default();
    let lang_part = if lang.is_empty() { "a project".to_string() } else { format!("a {lang} project") };
    parts.push(format!("## Project Overview\n\n{name_part}{lang_part}.\n\nDescribe what this project does here.\n"));

    if !build_cmds.is_empty() {
        parts.push(format!("## Build & Development Commands\n\n```bash\n{}\n```\n", build_cmds.join("\n")));
    }

    parts.push("## Architecture\n\nDescribe the key modules, directories, and how they interact.\n".into());
    parts.push("## Key Design Decisions\n\nDocument important architectural choices and their rationale.\n".into());
    parts.push("## Notes for forge-osh\n\nAdd any project-specific instructions here (e.g. test commands, coding style, off-limits files).\n".into());

    let content = parts.join("\n");
    match std::fs::write(&out_path, &content) {
        Ok(_) => state.push_system(format!(
            "Created CLAUDE.md at {}  (detected: {})\nEdit it to add project-specific instructions for the agent.",
            out_path.display(), if lang.is_empty() { "generic project" } else { lang }
        )),
        Err(e) => state.push_system(format!("Failed to create CLAUDE.md: {e}")),
    }
}

// ---------------------------------------------------------------------------
// /find — full-text search across project files
// ---------------------------------------------------------------------------

async fn cmd_find(state: &mut AppState, session: &Arc<Mutex<Session>>, pattern: &str) {
    let working_dir = { let sess = session.lock().await; sess.working_dir.clone() };
    let pattern_lc = pattern.to_lowercase();

    const BINARY_EXTS: &[&str] = &[
        "png","jpg","jpeg","gif","webp","bmp","ico","tiff","svg",
        "exe","dll","so","dylib","wasm","pdf","zip","tar","gz",
        "7z","rar","mp3","mp4","avi","mov","ttf","otf","woff",
    ];

    let mut results: Vec<String> = Vec::new();
    let mut file_count = 0usize;
    let mut match_count = 0usize;
    const MAX_MATCHES: usize = 60;

    use ignore::WalkBuilder;
    let walker = WalkBuilder::new(&working_dir).hidden(false).git_ignore(true).build();

    for entry in walker.filter_map(|e| e.ok()) {
        if match_count >= MAX_MATCHES { break; }
        let path = entry.path();
        if !path.is_file() { continue; }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if BINARY_EXTS.contains(&ext.as_str()) { continue; }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let relative = path.strip_prefix(&working_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());

        let mut file_hits: Vec<String> = Vec::new();
        for (i, line) in content.lines().enumerate() {
            if match_count >= MAX_MATCHES { break; }
            if line.to_lowercase().contains(&pattern_lc) {
                let trimmed = line.trim();
                let preview = if trimmed.len() > 120 { &trimmed[..120] } else { trimmed };
                file_hits.push(format!("  L{}: {}", i + 1, preview));
                match_count += 1;
            }
        }
        if !file_hits.is_empty() {
            file_count += 1;
            results.push(format!("{}:", relative));
            results.extend(file_hits);
        }
    }

    if results.is_empty() {
        state.push_system(format!("No matches found for: '{pattern}'"));
    } else {
        let mut out = format!(
            "Found {match_count} match(es) in {file_count} file(s) for '{pattern}':\n{}",
            results.join("\n")
        );
        if match_count >= MAX_MATCHES {
            out.push_str(&format!("\n... (showing first {MAX_MATCHES} matches — narrow your search)"));
        }
        state.push_system(out);
    }
}

// ---------------------------------------------------------------------------
// /config — view or change settings inline
// ---------------------------------------------------------------------------

fn cmd_config(state: &mut AppState, arg: &str) {
    if arg.is_empty() || arg == "show" {
        state.push_system(format!(
            "forge-osh Configuration\n\
            ├─ theme:        {}\n\
            ├─ trust_mode:   {}\n\
            ├─ vim_mode:     {}\n\
            ├─ fast_mode:    {}\n\
            ├─ context:      {}% used ({} limit)\n\
            └─ config file:  {}\n\n\
            To change: /config set <key> <value>\n\
            Keys: theme (dark/light/dracula/nord/solarized), trust (on/off)",
            state.theme_name,
            if state.trust_mode { "on" } else { "off" },
            if state.vim_normal_mode { "on" } else { "off" },
            if state.fast_mode { "on" } else { "off" },
            state.context_pct,
            state.context_limit,
            crate::config::config_dir().join("config.toml").display()
        ));
        return;
    }
    let parts: Vec<&str> = arg.splitn(3, ' ').collect();
    match parts.as_slice() {
        ["set", key, value] => match *key {
            "theme" => {
                state.theme = crate::tui::themes::Theme::from_name(value);
                state.theme_name = value.to_string();
                state.push_system(format!("Theme set to: {value}"));
            }
            "trust" | "trust_mode" => {
                let on = matches!(*value, "on" | "true" | "1" | "yes");
                state.trust_mode = on;
                state.push_system(format!("Trust mode: {}", if on { "ON" } else { "OFF" }));
            }
            "vim" | "vim_mode" => {
                let on = matches!(*value, "on" | "true" | "1" | "yes");
                state.vim_normal_mode = on;
                state.push_system(format!("Vim mode: {}", if on { "ON" } else { "OFF" }));
            }
            _ => state.push_system(format!(
                "Unknown config key '{key}'. Settable: theme, trust, vim"
            )),
        },
        _ => state.push_system(
            "Usage:\n  /config               — show all settings\n\
            /config set theme <name>    — dark/light/dracula/nord/solarized\n\
            /config set trust on|off    — toggle trust mode\n\
            /config set vim on|off      — toggle vim normal mode"
        ),
    }
}

// ---------------------------------------------------------------------------
// /stats — detailed session statistics
// ---------------------------------------------------------------------------

async fn cmd_stats(state: &mut AppState, session: &Arc<Mutex<Session>>) {
    let (user_msgs, assistant_msgs, tool_calls, total) = {
        let sess = session.lock().await;
        let msgs = sess.history.messages();
        let user   = msgs.iter().filter(|m| matches!(m, crate::types::Message::User(_))).count();
        let asst   = msgs.iter().filter(|m| matches!(m, crate::types::Message::Assistant(_))).count();
        let tools  = msgs.iter().filter(|m| matches!(m, crate::types::Message::Tool(_))).count();
        (user, asst, tools, msgs.len())
    };

    let rendered_msgs = state.messages.len();
    let tool_summary = if state.tool_stats.is_empty() {
        "  (none yet)".to_string()
    } else {
        let mut entries: Vec<(&String, &usize)> = state.tool_stats.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        entries.iter().take(12)
            .map(|(name, count)| format!("  {:<25} {}", name, count))
            .collect::<Vec<_>>()
            .join("\n")
    };

    state.push_system(format!(
        "Session Statistics\n\
        ─────────────────────────────────\n\
        History messages:\n\
        ├─ User:         {user_msgs}\n\
        ├─ Assistant:    {assistant_msgs}\n\
        ├─ Tool results: {tool_calls}\n\
        └─ Total:        {total}\n\
        \n\
        Display messages: {rendered_msgs}\n\
        Context:          {}% used\n\
        Tokens:           {}\n\
        Cost:             {}\n\
        \n\
        Tool calls this session:\n\
        {tool_summary}",
        state.context_pct,
        state.format_tokens,
        state.format_cost,
    ));
}

// ---------------------------------------------------------------------------
// /forge-graph — build or query the semantic context graph
// ---------------------------------------------------------------------------

async fn cmd_forge_graph(state: &mut AppState, session: &Arc<Mutex<Session>>, text: &str) {
    // Parse subcommand: /forge-graph [status|query <name>|rebuild|clear]
    let arg = text.trim_start_matches("/forge-graph").trim();

    match arg {
        // ── status ───────────────────────────────────────────────────────────
        "status" | "info" => {
            let msg = {
                let guard = state.shared_graph.read().unwrap();
                match guard.as_ref() {
                    None => "No forge-graph loaded.\nRun /forge-graph to build one for this project.".to_string(),
                    Some(g) => format!(
                        "forge-graph status\n\
                        ├─ Root:    {}\n\
                        ├─ Built:   {}\n\
                        ├─ Nodes:   {}\n\
                        ├─ Edges:   {}\n\
                        └─ Files:   {}",
                        g.meta.root_path,
                        g.meta.age_description(),
                        g.meta.total_nodes,
                        g.meta.total_edges,
                        g.meta.file_count,
                    ),
                }
            };
            state.push_system(msg);
        }

        // ── clear ─────────────────────────────────────────────────────────────
        "clear" => {
            let working_dir = { let s = session.lock().await; s.working_dir.clone() };
            let root = std::path::PathBuf::from(&working_dir);
            let exe_dir = CodeGraph::artifact_dir();
            let artifact = CodeGraph::artifact_path(&root, &exe_dir);
            if artifact.exists() {
                match std::fs::remove_file(&artifact) {
                    Ok(_) => state.push_system(format!("Removed artifact: {}", artifact.display())),
                    Err(e) => state.push_system(format!("Failed to remove artifact: {e}")),
                }
            }
            if let Ok(mut g) = state.shared_graph.write() {
                *g = None;
            }
            state.push_system("forge-graph cleared. Run /forge-graph to rebuild.");
        }

        // ── query <name> ──────────────────────────────────────────────────────
        s if s.starts_with("query ") => {
            let name = s.trim_start_matches("query ").trim().to_string();
            let msg = {
                let guard = state.shared_graph.read().unwrap();
                match guard.as_ref() {
                    None => "No graph loaded. Run /forge-graph first.".to_string(),
                    Some(g) => {
                        use crate::graph::query::GraphQuery;
                        let q = GraphQuery::new(g);
                        let results = q.fuzzy_search(&name, 15);
                        q.format_search(&results)
                    }
                }
            };
            state.push_system(msg);
        }

        // ── build (default: "" or "rebuild") ─────────────────────────────────
        "" | "rebuild" => {
            if state.graph_build_rx.is_some() {
                state.push_system("Graph build already in progress. Please wait.");
                return;
            }

            let working_dir = { let s = session.lock().await; s.working_dir.clone() };
            let root = std::path::PathBuf::from(&working_dir);
            let exe_dir = CodeGraph::artifact_dir();
            let artifact = CodeGraph::artifact_path(&root, &exe_dir);

            // If existing artifact and not "rebuild", load it
            if arg.is_empty() {
                let guard = state.shared_graph.read().unwrap();
                if guard.is_some() {
                    drop(guard);
                    state.push_system(
                        "forge-graph is already loaded. Use /forge-graph rebuild to force a full rebuild, \
                        or /forge-graph status to see details."
                    );
                    return;
                }
            }

            state.push_system(format!(
                "Building forge-graph for: {}\nArtifact: {}\nThis may take 10–120 seconds for large codebases...",
                root.display(), artifact.display()
            ));

            let (tx, rx) = std::sync::mpsc::channel::<GraphBuildMsg>();
            let shared_graph = state.shared_graph.clone();

            std::thread::spawn(move || {
                use crate::graph::builder::GraphBuilder;
                match GraphBuilder::build(&root, &tx) {
                    Ok(graph) => {
                        // Finalize and save artifact
                        let save_result = graph.save(&artifact);
                        let msg = match save_result {
                            Ok(_) => format!(
                                "forge-graph complete!\n\
                                Nodes: {}  Edges: {}  Files: {}\n\
                                Artifact: {}",
                                graph.meta.total_nodes, graph.meta.total_edges,
                                graph.meta.file_count, artifact.display()
                            ),
                            Err(e) => format!(
                                "forge-graph built in memory ({} nodes, {} edges) but save failed: {e}\n\
                                Graph is available for this session only.",
                                graph.meta.total_nodes, graph.meta.total_edges
                            ),
                        };
                        // Update shared graph
                        if let Ok(mut g) = shared_graph.write() {
                            *g = Some(graph);
                        }
                        let _ = tx.send(GraphBuildMsg::Done {
                            graph: CodeGraph::new(crate::graph::GraphMeta {
                                version: crate::graph::GRAPH_VERSION,
                                root_path: String::new(),
                                built_at: 0,
                                total_nodes: 0,
                                total_edges: 0,
                                file_count: 0,
                            }),
                            artifact_path: artifact,
                        });
                        // Use a progress message as the done signal (Done graph is dummy)
                        let _ = tx.send(GraphBuildMsg::Progress(format!("DONE:{msg}")));
                    }
                    Err(e) => {
                        let _ = tx.send(GraphBuildMsg::Error(e.to_string()));
                    }
                }
            });

            state.graph_build_rx = Some(rx);
        }

        _ => {
            state.push_system(format!(
                "Unknown /forge-graph subcommand: '{arg}'\n\
                Usage:\n\
                  /forge-graph           — build graph for current project\n\
                  /forge-graph rebuild   — force full rebuild\n\
                  /forge-graph status    — show graph info\n\
                  /forge-graph query <name> — search graph for a symbol\n\
                  /forge-graph clear     — remove artifact and unload"
            ));
        }
    }
}

/// LLM-based compact: summarize old messages with the active provider,
/// then replace them with the summary so the context window is freed.
async fn compact_history_llm(
    state: &mut AppState,
    session: &Arc<Mutex<Session>>,
    provider_router: &Arc<RwLock<ProviderRouter>>,
    keep: usize,
) {
    use crate::agent::compaction;

    let (messages, model_id, total) = {
        let sess = session.lock().await;
        let msgs = sess.history.messages().to_vec();
        let total = msgs.len();
        let model_id = sess.model_id.clone();
        (msgs, model_id, total)
    };

    if total <= keep {
        state.push_system(format!(
            "Conversation has {total} messages. Nothing to compact (keeping last {keep})."
        ));
        return;
    }

    let (to_summarize, _) = compaction::split_for_compaction(&messages, keep);
    let to_summarize = to_summarize.to_vec();

    state.push_system("Compacting with AI summary — please wait...");

    let summary_result = {
        let router = provider_router.read().await;
        match router.active() {
            Ok(provider) => {
                compaction::summarize_messages(&to_summarize, provider, &model_id).await
            }
            Err(e) => Err(e),
        }
    };

    match summary_result {
        Ok(summary) => {
            let removed = total - keep;
            {
                let mut sess = session.lock().await;
                sess.history.summarize_old(summary.clone(), keep);
            }

            // Trim the rendered view: keep only the most-recent rendered messages
            let rendered_keep = state.messages.len().saturating_sub(removed.saturating_mul(2));
            state.messages.drain(..rendered_keep);

            state.push_system(format!(
                "Compacted: {removed} messages summarized by AI. {keep} messages kept in full.\n\
                Summary: {}",
                &summary[..summary.len().min(300)]
            ));
        }
        Err(e) => {
            // Fall back to simple truncation if LLM call fails
            let removed = total - keep;
            {
                let mut sess = session.lock().await;
                sess.history.compact(keep);
            }
            let rendered_keep = state.messages.len().saturating_sub(removed.saturating_mul(2));
            state.messages.drain(..rendered_keep);
            state.push_system(format!(
                "AI summary failed ({e}); fell back to simple truncation. \
                Removed {removed} messages, {keep} remain."
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming text commit with deduplication
// ---------------------------------------------------------------------------

/// Commit the current streaming_text as an Assistant message, but only if it
/// has not already been committed (detected via hash). This prevents the same
/// text from appearing twice in the conversation display.
fn commit_streaming_text(state: &mut AppState) {
    if state.streaming_text.is_empty() {
        return;
    }
    // Compute a simple hash of the text to detect duplicates
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    state.streaming_text.hash(&mut hasher);
    let hash = hasher.finish();

    if hash == state.last_committed_hash && state.last_committed_hash != 0 {
        // Duplicate detected — discard without committing
        state.streaming_text.clear();
        return;
    }
    state.last_committed_hash = hash;

    if !state.auto_scroll {
        state.unread_count += 1;
    }
    state.messages.push(RenderedMessage {
        role: MessageRole::Assistant,
        content: std::mem::take(&mut state.streaming_text),
    });
}

// ---------------------------------------------------------------------------
// Main TUI event loop
// ---------------------------------------------------------------------------

pub async fn run_tui(
    config: Arc<Config>,
    provider_router: Arc<RwLock<ProviderRouter>>,
    tools: Arc<ToolRegistry>,
    session: Arc<Mutex<Session>>,
    key_store: Arc<Mutex<KeyStore>>,
    shared_graph: SharedGraph,
) -> anyhow::Result<()> {
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Channels between TUI and agent loop
    let (agent_event_tx, mut agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let (perm_req_tx, mut perm_req_rx) = mpsc::unbounded_channel::<PermissionRequest>();
    let (_perm_resp_tx, perm_resp_rx) = mpsc::unbounded_channel::<PermissionResponse>();

    // Build initial app state
    let mut state = {
        let sess = session.lock().await;
        AppState::new(&config, &sess, shared_graph.clone())
    };

    // Resolve proper display names from provider router and model catalog
    {
        let router = provider_router.read().await;
        let pid = router.active_provider_id();
        state.provider_id = pid.to_string();
        state.model_id = router.active_model_id().to_string();

        // Provider display name
        for (id, name) in router.available_providers() {
            if id == pid {
                state.provider_name = name.to_string();
                break;
            }
        }

        // Model display name from built-in catalog
        for m in &crate::config::models::models_for_provider(pid) {
            if m.id == state.model_id {
                state.model_name = m.name.clone();
                break;
            }
        }
        // If still empty, fall back to model_id
        if state.model_name.is_empty() {
            state.model_name = state.model_id.clone();
        }
    }

    // Load persistent input history from previous sessions.
    {
        let history_path = config::config_dir().join("input_history.json");
        let loaded = input::InputState::load_history(&history_path);
        if !loaded.is_empty() {
            state.input.history = loaded;
        }
    }

    // Always show the OSH splash banner first.
    state.messages.push(RenderedMessage {
        role: MessageRole::Splash,
        content: String::new(), // content is taken from OSH_SPLASH_LINES
    });

    // Restore persistent chat history from the loaded session into the display.
    // This makes resumed sessions show their full prior conversation immediately.
    {
        let sess = session.lock().await;
        let msg_count = sess.history.message_count();
        if msg_count > 0 {
            restore_history_to_display(&mut state, &sess);
            state.messages.push(RenderedMessage {
                role: MessageRole::System,
                content: format!(
                    "Resumed session '{}' — {} messages restored.  Model: {}  Provider: {}  Type /help for commands.",
                    sess.name, msg_count, state.model_name, state.provider_name
                ),
            });
        } else {
            // Count available providers and build the startup status line.
            // This is displayed to every new user so they can immediately see
            // whether they need to add API keys (Ctrl+K) before starting.
            let (provider_count, provider_list) = {
                let router = provider_router.read().await;
                let available: Vec<String> = router
                    .available_providers()
                    .iter()
                    .map(|(id, _)| id.to_string())
                    .collect();
                (available.len(), available.join(", "))
            };

            let cfg_dir = crate::config::config_dir();

            if provider_count == 0 {
                // No API keys configured — guide the user to set one up.
                state.messages.push(RenderedMessage {
                    role: MessageRole::System,
                    content: format!(
                        "No providers configured yet.\n\
                        Press Ctrl+K to add an API key, or set an env var (e.g. ANTHROPIC_API_KEY).\n\
                        Keys are stored in: {}\n\
                        Type /help for all commands.",
                        cfg_dir.display()
                    ),
                });
            } else {
                state.messages.push(RenderedMessage {
                    role: MessageRole::System,
                    content: format!(
                        "Model: {}  |  Provider: {}  |  {} provider(s) ready: [{}]  |  Type /help for commands.",
                        state.model_name, state.provider_name, provider_count, provider_list
                    ),
                });
            }
        }
    }

    let agent_loop = Arc::new(AgentLoop {
        provider_router: provider_router.clone(),
        tools: tools.clone(),
        session: session.clone(),
        config: config.clone(),
        event_tx: agent_event_tx.clone(),
        permission_tx: perm_req_tx,
        permission_rx: Arc::new(Mutex::new(perm_resp_rx)),
        graph: shared_graph.clone(),
    });

    // -----------------------------------------------------------------------
    // Main event loop
    // -----------------------------------------------------------------------
    while state.running {
        // Draw frame
        terminal.draw(|frame| renderer::render(frame, &mut state))?;

        // Tick spinner animation
        if state.spinner.active {
            state.spinner.tick();
        }

        // Poll interval: faster when agent is working (for smooth streaming)
        let timeout = if state.spinner.active || state.agent_busy {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(50)
        };

        // ---- Drain agent events (non-blocking) ----
        while let Ok(event) = agent_event_rx.try_recv() {
            match event {
                AgentEvent::ThinkingStart => {
                    state.spinner.start(format!("{} is thinking...", state.model_name));
                    state.streaming_text.clear();
                    // Reset dedup hash so the new turn can produce legitimate text
                    state.last_committed_hash = 0;
                    // Do NOT reset scroll here — the user may have scrolled up to read
                    // earlier content while the agent is working. Scroll is only reset
                    // on explicit user Submit (see Action::Submit handler).
                }
                AgentEvent::Token(t) => {
                    // Stop the "thinking" spinner on first token so the streaming
                    // text is visible immediately.
                    if state.spinner.active {
                        state.spinner.stop();
                    }
                    state.streaming_text.push_str(&t);
                    // Do NOT force auto_scroll here — user may have scrolled up
                }
                AgentEvent::ToolStart { name, input } => {
                    state.spinner.stop();
                    // Commit any streaming text before tool execution
                    commit_streaming_text(&mut state);
                    state.messages.push(RenderedMessage {
                        role: MessageRole::ToolCall { name: name.clone() },
                        content: serde_json::to_string_pretty(&input).unwrap_or_default(),
                    });
                    // Track tool usage stats
                    *state.tool_stats.entry(name.clone()).or_insert(0) += 1;
                    state.spinner.start(format!("Running {}...", name));
                }
                AgentEvent::ToolEnd { name, output, is_error } => {
                    state.spinner.stop();
                    if !state.auto_scroll { state.unread_count += 1; }
                    state.messages.push(RenderedMessage {
                        role: MessageRole::ToolResult { is_error, tool_name: name },
                        content: output,
                    });
                }
                AgentEvent::ContextWarning { used, limit } => {
                    // Update context percentage for the progress bar
                    state.context_pct = ((used as f64 / limit as f64 * 100.0) as u8).min(100);
                    state.context_limit = limit;
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!(
                            "Context window {:.0}% full ({}/{} tokens). Use /compact to free space.",
                            used as f64 / limit as f64 * 100.0,
                            used,
                            limit
                        ),
                    });
                }
                AgentEvent::Done => {
                    state.spinner.stop();
                    state.agent_busy = false;
                    // Commit remaining streaming text (guarded against duplicates)
                    commit_streaming_text(&mut state);
                    let sess = session.lock().await;
                    state.format_cost = sess.format_cost();
                    state.format_tokens = sess.format_tokens();
                    // Update context bar from actual token usage
                    if state.context_limit > 0 {
                        let used = sess.cost_tracker.total_input_tokens
                            + sess.cost_tracker.total_output_tokens;
                        state.context_pct = ((used as f64 / state.context_limit as f64 * 100.0) as u8).min(100);
                    }
                }
                AgentEvent::Error(e) => {
                    state.spinner.stop();
                    state.agent_busy = false;
                    // Commit any partial streaming text before showing error
                    commit_streaming_text(&mut state);
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("Error: {e}"),
                    });
                }
                // -- Multithread worker events -----------------------------------
                AgentEvent::WorkerSpawned { worker_id, description } => {
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("⚡ Worker spawned: {description} [{worker_id}]"),
                    });
                }
                AgentEvent::WorkerCompleted { worker_id, description, result, duration_ms } => {
                    let duration_s = duration_ms as f64 / 1000.0;
                    let preview = if result.len() > 500 {
                        format!("{}…", &result[..500])
                    } else {
                        result
                    };
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!(
                            "✅ Worker completed: {description} [{worker_id}] ({duration_s:.1}s)\n\
                            Result: {preview}"
                        ),
                    });
                }
                AgentEvent::WorkerFailed { worker_id, description, error, duration_ms } => {
                    let duration_s = duration_ms as f64 / 1000.0;
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!(
                            "❌ Worker failed: {description} [{worker_id}] ({duration_s:.1}s)\n\
                            Error: {error}"
                        ),
                    });
                }
                AgentEvent::WorkerToolStart { worker_id, name } => {
                    // Brief notification — don't clutter the display
                    if !state.fast_mode {
                        state.messages.push(RenderedMessage {
                            role: MessageRole::System,
                            content: format!("  [{worker_id}] running {name}..."),
                        });
                    }
                }
                AgentEvent::WorkerToolEnd { worker_id, name, is_error } => {
                    if is_error && !state.fast_mode {
                        state.messages.push(RenderedMessage {
                            role: MessageRole::System,
                            content: format!("  [{worker_id}] {name} returned error"),
                        });
                    }
                }
            }
        }

        // ---- Drain graph build progress messages ----
        if state.graph_build_rx.is_some() {
            let mut done = false;
            // Collect messages without holding borrow on state.graph_build_rx
            let msgs: Vec<GraphBuildMsg> = {
                let rx = state.graph_build_rx.as_ref().unwrap();
                let mut collected = Vec::new();
                while let Ok(m) = rx.try_recv() {
                    collected.push(m);
                }
                collected
            };
            for msg in msgs {
                match msg {
                    GraphBuildMsg::Progress(s) => {
                        if let Some(rest) = s.strip_prefix("DONE:") {
                            state.push_system(rest.to_string());
                            done = true;
                        } else {
                            state.push_system(format!("[forge-graph] {s}"));
                        }
                    }
                    GraphBuildMsg::Done { .. } => { done = true; }
                    GraphBuildMsg::Error(e) => {
                        state.push_system(format!("forge-graph error: {e}"));
                        done = true;
                    }
                }
            }
            if done { state.graph_build_rx = None; }
        }

        // ---- Drain coordinator worker notifications ----
        if state.multithread_mode {
            if let Some(ref mut coord) = state.coordinator {
                let worker_events = coord.drain_notifications();
                for ev in worker_events {
                    let _ = agent_event_tx.send(ev);
                }
            }
        }

        // ---- Drain permission requests ----
        while let Ok(req) = perm_req_rx.try_recv() {
            state.modal = Some(Modal::Confirmation {
                tool_name: req.tool_name,
                description: req.description,
                response_tx: req.response_tx,
            });
        }

        // ---- Process pending key operations ----
        if let Some((provider_id, api_key)) = state.key_save_pending.take() {
            let save_ok = {
                let mut ks = key_store.lock().await;
                ks.set(&provider_id, &api_key)
            };
            match save_ok {
                Ok(_) => {
                    // Hotreload: instantiate the provider immediately so the
                    // user can switch to it without restarting the app.
                    let reload_result = {
                        let mut router = provider_router.write().await;
                        router.reload_provider(&provider_id, api_key, &config)
                    };
                    match reload_result {
                        Ok(_) => state.push_system(format!(
                            "API key saved. Provider '{provider_id}' is now available — use Ctrl+P to switch."
                        )),
                        Err(e) => state.push_system(format!(
                            "Key saved to disk, but provider init failed: {e}"
                        )),
                    }
                }
                Err(e) => state.push_system(format!("Failed to save key: {e}")),
            }
        }
        if let Some(provider_id) = state.key_delete_pending.take() {
            let del_ok = {
                let mut ks = key_store.lock().await;
                ks.delete(&provider_id)
            };
            match del_ok {
                Ok(_) => {
                    // Remove provider from router immediately
                    {
                        let mut router = provider_router.write().await;
                        router.remove_provider(&provider_id);
                    }
                    state.push_system(format!("API key removed for {provider_id}."));
                }
                Err(e) => state.push_system(format!("Failed to remove key: {e}")),
            }
        }

        // ---- Process pending session delete ----
        if let Some(id) = state.session_delete_pending.take() {
            match crate::session::checkpoint::Checkpoint::delete(&id) {
                Ok(_) => state.push_system(format!("Session {} deleted.", &id[..8.min(id.len())])),
                Err(e) => state.push_system(format!("Failed to delete session: {e}")),
            }
        }

        // ---- Process pending session load ----
        if let Some(id) = state.session_load_pending.take() {
            match crate::session::checkpoint::Checkpoint::load(&id) {
                Ok(mut new_session) => {
                    // Keep current working dir so tools stay in context
                    let current_wd = {
                        let sess = session.lock().await;
                        sess.working_dir.clone()
                    };
                    new_session.working_dir = current_wd;

                    let sess_name = new_session.name.clone();
                    let sess_provider = new_session.provider_id.clone();
                    let sess_model = new_session.model_id.clone();

                    // Replace session contents
                    {
                        let mut sess = session.lock().await;
                        *sess = new_session;
                    }

                    // Rebuild TUI display from loaded history
                    state.messages.retain(|m| matches!(m.role, MessageRole::Splash));
                    state.streaming_text.clear();
                    state.scroll_top = 0;
                    state.auto_scroll = true;
                    state.unread_count = 0;
                    state.tool_stats.clear();
                    state.context_pct = 0;
                    state.format_tokens = "0 tokens".to_string();
                    state.format_cost = "$0.00".to_string();
                    state.session_name = sess_name.clone();
                    state.provider_id = sess_provider.clone();
                    state.model_id = sess_model.clone();
                    // Update display names
                    {
                        let router = provider_router.read().await;
                        for (id, name) in router.available_providers() {
                            if id == sess_provider {
                                state.provider_name = name.to_string();
                                break;
                            }
                        }
                        for m in &crate::config::models::models_for_provider(&sess_provider) {
                            if m.id == sess_model {
                                state.model_name = m.name.clone();
                                break;
                            }
                        }
                        if state.model_name.is_empty() {
                            state.model_name = sess_model.clone();
                        }
                    }
                    {
                        let sess = session.lock().await;
                        restore_history_to_display(&mut state, &sess);
                    }
                    state.push_system(format!("Loaded session: {sess_name}"));
                }
                Err(e) => state.push_system(format!("Failed to load session: {e}")),
            }
        }

        // ---- Process pending model/provider switch ----
        if let Some((pid, mid)) = state.model_switch_pending.take() {
            let mut router = provider_router.write().await;
            match router.set_active(&pid, &mid) {
                Ok(_) => {
                    // Update display names after switch
                    state.provider_id = pid.clone();
                    state.model_id = mid.clone();
                    for (id, name) in router.available_providers() {
                        if id == pid {
                            state.provider_name = name.to_string();
                            break;
                        }
                    }
                    for m in &crate::config::models::models_for_provider(&pid) {
                        if m.id == mid {
                            state.model_name = m.name.clone();
                            break;
                        }
                    }
                    if state.model_name.is_empty() {
                        state.model_name = mid.clone();
                    }
                }
                Err(e) => state.push_system(format!("Failed to switch: {e}")),
            }
        }

        // ---- Poll terminal events ----
        if event::poll(timeout)? {
            // Batch: read all pending events then redraw once
            loop {
                let ev = event::read()?;

                match ev {
                    Event::Key(key) => {
                        // Skip key-release events (Windows sends both Press and Release)
                        if key.kind != KeyEventKind::Press {
                            if !event::poll(Duration::ZERO)? { break; }
                            continue;
                        }

                        // Modal input has priority over everything else
                        if state.modal.is_some() {
                            handle_modal_input(&mut state, key);
                            if !event::poll(Duration::ZERO)? { break; }
                            continue;
                        }

                        // Vim normal mode — hjkl scroll instead of text input
                        if state.vim_normal_mode {
                            match (key.modifiers, key.code) {
                                (KeyModifiers::NONE, KeyCode::Char('j')) => { state.scroll_down(3); }
                                (KeyModifiers::NONE, KeyCode::Char('k')) => { state.scroll_up(3); }
                                (KeyModifiers::NONE, KeyCode::Char('d')) => {
                                    let h = state.visible_height / 2;
                                    state.scroll_down(h);
                                }
                                (KeyModifiers::NONE, KeyCode::Char('u')) => {
                                    let h = state.visible_height / 2;
                                    state.scroll_up(h);
                                }
                                (KeyModifiers::NONE, KeyCode::Char('G')) => {
                                    state.auto_scroll = true;
                                }
                                (KeyModifiers::NONE, KeyCode::Char('g')) => {
                                    state.scroll_top = 0;
                                    state.auto_scroll = false;
                                }
                                (KeyModifiers::NONE, KeyCode::Char('i'))
                                | (KeyModifiers::NONE, KeyCode::Char('a')) => {
                                    state.vim_normal_mode = false;
                                }
                                (KeyModifiers::NONE, KeyCode::Esc) => {} // already in normal mode
                                (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                                    if state.agent_busy {
                                        if let Some(task) = state.agent_task.take() {
                                            task.abort();
                                        }
                                        state.spinner.stop();
                                        state.agent_busy = false;
                                        commit_streaming_text(&mut state);
                                        state.push_system("Execution cancelled by user.");
                                    }
                                }
                                _ => {}
                            }
                            if !event::poll(Duration::ZERO)? { break; }
                            continue;
                        }

                        let action = input::map_key_normal(key);
                        match action {
                            // ---- Submit / slash commands ----
                            Action::Submit => {
                                if !state.input.is_empty() && !state.agent_busy {
                                    let text = state.input.submit();

                                    if text.trim_start().starts_with('/') {
                                        // Slash command — handle locally, do NOT send to agent
                                        handle_slash_command(
                                            &text,
                                            &mut state,
                                            &session,
                                            &provider_router,
                                            &key_store,
                                            &config,
                                        ).await;
                                    } else if state.multithread_mode && text.starts_with("@worker ") {
                                        // Multithread mode: @worker prefix spawns a parallel worker
                                        let task_prompt = text.strip_prefix("@worker ").unwrap_or(&text).to_string();
                                        state.messages.push(RenderedMessage {
                                            role: MessageRole::User,
                                            content: text.clone(),
                                        });
                                        // Lazily initialize coordinator if needed
                                        if state.coordinator.is_none() {
                                            state.coordinator = Some(Coordinator::new(
                                                provider_router.clone(),
                                                tools.clone(),
                                                config.clone(),
                                                shared_graph.clone(),
                                                session.clone(),
                                                agent_event_tx.clone(),
                                            ));
                                        }
                                        if let Some(ref mut coord) = state.coordinator {
                                            let desc = if task_prompt.len() > 60 {
                                                format!("{}…", &task_prompt[..60])
                                            } else {
                                                task_prompt.clone()
                                            };
                                            coord.spawn_worker(desc, task_prompt);
                                        }
                                        state.scroll_top = 0;
                                        state.auto_scroll = true;
                                    } else {
                                        // Normal user message — send to agent loop
                                        state.messages.push(RenderedMessage {
                                            role: MessageRole::User,
                                            content: text.clone(),
                                        });
                                        state.agent_busy = true;
                                        state.scroll_top = 0;
                                        state.auto_scroll = true;

                                        let loop_clone = agent_loop.clone();
                                        state.agent_task = Some(tokio::spawn(async move {
                                            let _ = loop_clone.run(text).await;
                                        }));
                                    }
                                }
                            }

                            // ---- Text editing ----
                            Action::InsertChar(c) => state.input.insert_char(c),
                            Action::Backspace => state.input.backspace(),
                            Action::Delete => state.input.delete(),
                            Action::CursorLeft => state.input.cursor_left(),
                            Action::CursorRight => state.input.cursor_right(),
                            Action::CursorHome => state.input.cursor_home(),
                            Action::CursorEnd => state.input.cursor_end(),
                            Action::DeleteToStart => state.input.delete_to_start(),
                            Action::DeleteWord => state.input.delete_word(),
                            Action::HistoryUp => state.input.history_up(),
                            Action::HistoryDown => state.input.history_down(),
                            Action::NewLine => state.input.insert_char('\n'),
                            Action::TabComplete => {
                                tab_complete_slash(&mut state);
                            }

                            // ---- Scrolling ----
                            Action::ScrollUp => state.scroll_up(3),
                            Action::ScrollDown => state.scroll_down(3),
                            Action::PageUp => state.scroll_up(10),
                            Action::PageDown => state.scroll_down(10),
                            Action::ScrollTop => {
                                state.scroll_top = 0;
                                state.auto_scroll = false;
                            }
                            Action::ScrollBottom => {
                                state.auto_scroll = true;
                                state.unread_count = 0;
                            }

                            // ---- Global ----
                            Action::Quit => {
                                if state.input.is_empty() {
                                    state.running = false;
                                }
                            }
                            Action::Cancel => {
                                if state.agent_busy {
                                    if let Some(task) = state.agent_task.take() {
                                        task.abort();
                                    }
                                    state.spinner.stop();
                                    state.agent_busy = false;
                                    commit_streaming_text(&mut state);
                                    state.push_system("Execution cancelled by user.");
                                }
                            }
                            Action::ClearScreen => {
                                state.messages.clear();
                                state.streaming_text.clear();
                                state.scroll_top = 0;
                                state.auto_scroll = true;
                                state.push_system("Screen cleared.");
                            }

                            // ---- Ctrl shortcuts (all preserved) ----
                            Action::ToggleTrustMode => {
                                state.trust_mode = !state.trust_mode;
                                state.push_system(format!(
                                    "Trust mode: {}",
                                    if state.trust_mode { "ON" } else { "OFF" }
                                ));
                            }
                            Action::CycleTheme => {
                                let next = Theme::next_theme_name(&state.theme_name);
                                state.theme = Theme::from_name(next);
                                state.theme_name = next.to_string();
                                state.push_system(format!("Theme: {next}"));
                            }
                            Action::ShowHelp => {
                                state.modal = Some(Modal::Help);
                            }
                            Action::ShowTokenInfo => {
                                state.modal = Some(Modal::TokenInfo);
                            }
                            Action::OpenModelPicker => {
                                let pid = state.provider_id.clone();
                                let pname = state.provider_name.clone();
                                let models = crate::config::models::models_for_provider(&pid);
                                let mut items: Vec<picker::PickerItem> = models
                                    .iter()
                                    .map(|m| picker::PickerItem::from_model_info(m, true, &pname))
                                    .collect();
                                // Always add "Add custom model" entry at the bottom
                                items.push(picker::PickerItem {
                                    provider_id: pid.clone(),
                                    provider_name: pname.clone(),
                                    model_id: "__add_custom__".to_string(),
                                    model_name: "+ Add custom model...".to_string(),
                                    context_window: 0,
                                    cost_display: "enter any model ID".to_string(),
                                    connected: true,
                                });
                                state.modal = Some(Modal::Picker(PickerState::new(items)));
                            }
                            Action::OpenProviderPicker => {
                                let router = provider_router.read().await;
                                let mut items = Vec::new();
                                for (pid, pname) in router.available_providers() {
                                    let models = crate::config::models::models_for_provider(pid);
                                    let item = if let Some(m) = models.first() {
                                        picker::PickerItem::from_model_info(m, true, pname)
                                    } else {
                                        picker::PickerItem {
                                            provider_id: pid.to_string(),
                                            provider_name: pname.to_string(),
                                            model_id: String::new(),
                                            model_name: "(default)".to_string(),
                                            context_window: 0,
                                            cost_display: String::new(),
                                            connected: true,
                                        }
                                    };
                                    items.push(item);
                                }
                                state.modal = Some(Modal::Picker(PickerState::new(items)));
                            }
                            Action::OpenKeyManager => {
                                let ks = key_store.lock().await;
                                let mut entries = Vec::new();
                                for pid in config::cloud_provider_ids() {
                                    let env_var = crate::config::keyring::provider_env_var(pid);
                                    let has_env = std::env::var(&env_var).is_ok();
                                    let has_stored = ks.list_providers().contains(&pid.to_string());
                                    let has_key = has_env || has_stored;
                                    let source = if has_env && has_stored {
                                        "env+stored".to_string()
                                    } else if has_env {
                                        "env".to_string()
                                    } else if has_stored {
                                        "stored".to_string()
                                    } else {
                                        "none".to_string()
                                    };
                                    entries.push(KeyManagerEntry {
                                        provider_id: pid.to_string(),
                                        provider_name: pid.to_string(),
                                        has_key,
                                        key_source: source,
                                    });
                                }
                                state.modal = Some(Modal::KeyManager(KeyManagerState {
                                    providers: entries,
                                    selected: 0,
                                    editing: false,
                                    input_buffer: String::new(),
                                }));
                            }
                            Action::SaveSession => {
                                let sess = session.lock().await;
                                match sess.save() {
                                    Ok(_) => state.push_system("Session saved."),
                                    Err(e) => state.push_system(format!("Save failed: {e}")),
                                }
                            }
                            Action::None => {
                                // Escape with no modifiers enters vim normal mode
                                if key.modifiers == KeyModifiers::NONE && key.code == KeyCode::Esc {
                                    state.vim_normal_mode = true;
                                }
                            }
                            _ => {}
                        }
                    }

                    // Mouse scroll
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp => state.scroll_up(3),
                        MouseEventKind::ScrollDown => state.scroll_down(3),
                        _ => {}
                    },

                    // Terminal resize or focus events — just redraw
                    _ => {}
                }

                if !event::poll(Duration::ZERO)? { break; }
            }
        }
    }

    // Auto-save session on clean exit so progress is never lost.
    {
        let sess = session.lock().await;
        let _ = sess.save();
    }

    // Persist input history for next session.
    {
        let history_path = config::config_dir().join("input_history.json");
        state.input.save_history(&history_path);
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Restore persisted history into the rendered display
// ---------------------------------------------------------------------------

/// Convert the messages stored in `session.history` into `RenderedMessage`
/// entries so the TUI shows the full prior conversation on resume.
fn restore_history_to_display(state: &mut AppState, session: &Session) {
    use crate::types::Message;

    for msg in session.history.messages() {
        match msg {
            Message::User(crate::types::UserContent::Text(text)) => {
                state.messages.push(RenderedMessage {
                    role: MessageRole::User,
                    content: text.clone(),
                });
            }
            Message::Assistant(content) => {
                // Show text response if any
                if let Some(text) = content.text() {
                    if !text.is_empty() {
                        state.messages.push(RenderedMessage {
                            role: MessageRole::Assistant,
                            content: text.to_string(),
                        });
                    }
                }
                // Show tool calls compactly
                for tc in content.tool_calls() {
                    state.messages.push(RenderedMessage {
                        role: MessageRole::ToolCall { name: tc.name.clone() },
                        content: serde_json::to_string_pretty(&tc.input).unwrap_or_default(),
                    });
                }
            }
            Message::Tool(result) => {
                state.messages.push(RenderedMessage {
                    role: MessageRole::ToolResult { is_error: result.is_error, tool_name: String::new() },
                    content: result.content.clone(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Modal input handler
// ---------------------------------------------------------------------------

fn handle_modal_input(state: &mut AppState, key: crossterm::event::KeyEvent) {
    let modal = state.modal.take();

    match modal {
        Some(Modal::Confirmation { tool_name, description, response_tx }) => {
            let action = input::map_key_confirm(key);
            match action {
                Action::Confirm => {
                    let _ = response_tx.send(PermissionResponse::Allow);
                }
                Action::Deny => {
                    let _ = response_tx.send(PermissionResponse::Deny);
                }
                Action::AlwaysAllow => {
                    let _ = response_tx.send(PermissionResponse::AlwaysAllow);
                }
                Action::EnableTrustMode => {
                    state.trust_mode = true;
                    let _ = response_tx.send(PermissionResponse::TrustMode);
                }
                _ => {
                    // Not a recognised confirmation key — keep the modal open
                    state.modal = Some(Modal::Confirmation {
                        tool_name,
                        description,
                        response_tx,
                    });
                }
            }
        }

        Some(Modal::Help) => {
            if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                // Closed
            } else {
                state.modal = Some(Modal::Help);
            }
        }

        Some(Modal::TokenInfo) => {
            if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                // Closed
            } else {
                state.modal = Some(Modal::TokenInfo);
            }
        }

        Some(Modal::Picker(mut picker)) => {
            let action = input::map_key_picker(key, picker.filtering);
            match action {
                Action::PickerUp => {
                    picker.move_up();
                    state.modal = Some(Modal::Picker(picker));
                }
                Action::PickerDown => {
                    picker.move_down();
                    state.modal = Some(Modal::Picker(picker));
                }
                Action::PickerSelect => {
                    if let Some(item) = picker.selected_item() {
                        let provider_id = item.provider_id.clone();
                        let provider_name = item.provider_name.clone();
                        let model_id = item.model_id.clone();
                        let model_name = item.model_name.clone();

                        if model_id == "__add_custom__" {
                            // Open custom model input dialog
                            state.modal = Some(Modal::CustomModelInput {
                                provider_id,
                                input_buffer: String::new(),
                            });
                        } else {
                            state.model_switch_pending = Some((provider_id.clone(), model_id.clone()));
                            state.messages.push(RenderedMessage {
                                role: MessageRole::System,
                                content: format!(
                                    "Switching to {} ({})",
                                    model_name, provider_name
                                ),
                            });
                            // Optimistic update — will be confirmed by the pending switch handler
                            state.provider_id = provider_id;
                            state.provider_name = provider_name;
                            state.model_id = model_id;
                            state.model_name = model_name;
                        }
                    }
                }
                Action::PickerCancel => {
                    if picker.filtering {
                        picker.stop_filter();
                        state.modal = Some(Modal::Picker(picker));
                    }
                    // else close (modal was taken, not re-set)
                }
                Action::Cancel => {
                    // Ctrl+C always closes
                }
                Action::PickerFilter => {
                    picker.start_filter();
                    state.modal = Some(Modal::Picker(picker));
                }
                Action::PickerFilterChar(c) => {
                    picker.add_filter_char(c);
                    state.modal = Some(Modal::Picker(picker));
                }
                Action::PickerFilterBackspace => {
                    picker.remove_filter_char();
                    state.modal = Some(Modal::Picker(picker));
                }
                _ => {
                    state.modal = Some(Modal::Picker(picker));
                }
            }
        }

        Some(Modal::KeyManager(mut km)) => {
            let is_enter = key.code == KeyCode::Enter
                || (key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::CONTROL));

            if km.editing {
                if key.code == KeyCode::Esc {
                    km.editing = false;
                    km.input_buffer.clear();
                    state.modal = Some(Modal::KeyManager(km));
                } else if is_enter {
                    if !km.input_buffer.is_empty() {
                        if let Some(entry) = km.providers.get_mut(km.selected) {
                            entry.has_key = true;
                            entry.key_source = "stored".to_string();
                            state.key_save_pending = Some((
                                entry.provider_id.clone(),
                                km.input_buffer.clone(),
                            ));
                        }
                    }
                    km.editing = false;
                    km.input_buffer.clear();
                    state.modal = Some(Modal::KeyManager(km));
                } else if key.code == KeyCode::Backspace {
                    km.input_buffer.pop();
                    state.modal = Some(Modal::KeyManager(km));
                } else if let KeyCode::Char(c) = key.code {
                    if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        km.input_buffer.push(c);
                    }
                    state.modal = Some(Modal::KeyManager(km));
                } else {
                    state.modal = Some(Modal::KeyManager(km));
                }
            } else {
                if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                    // Closed
                } else if key.code == KeyCode::Up {
                    km.move_up();
                    state.modal = Some(Modal::KeyManager(km));
                } else if key.code == KeyCode::Down {
                    km.move_down();
                    state.modal = Some(Modal::KeyManager(km));
                } else if is_enter || key.code == KeyCode::Char('e') {
                    km.editing = true;
                    km.input_buffer.clear();
                    state.modal = Some(Modal::KeyManager(km));
                } else if key.code == KeyCode::Char('d') || key.code == KeyCode::Delete {
                    let did_delete = if let Some(entry) = km.providers.get_mut(km.selected) {
                        if entry.key_source == "stored" || entry.key_source == "env+stored" {
                            state.key_delete_pending = Some(entry.provider_id.clone());
                            let has_env = std::env::var(
                                crate::config::keyring::provider_env_var(&entry.provider_id)
                            ).is_ok();
                            entry.has_key = has_env;
                            entry.key_source = if has_env {
                                "env".to_string()
                            } else {
                                "none".to_string()
                            };
                            true
                        } else if entry.key_source == "env" {
                            let env_var = crate::config::keyring::provider_env_var(&entry.provider_id);
                            state.push_system(format!(
                                "Cannot delete key for '{}' — it is set via environment variable ({}).\n\
                                Unset the env var to remove it.",
                                entry.provider_id, env_var
                            ));
                            false
                        } else {
                            state.push_system(format!(
                                "No stored key for '{}' to delete.", entry.provider_id
                            ));
                            false
                        }
                    } else {
                        false
                    };
                    // Close modal after deletion so user can see the confirmation message.
                    // Keep modal open for error cases (env-only or nothing to delete).
                    if !did_delete {
                        state.modal = Some(Modal::KeyManager(km));
                    }
                    // If did_delete: modal stays closed; key_delete_pending fires next frame.
                } else {
                    state.modal = Some(Modal::KeyManager(km));
                }
            }
        }

        Some(Modal::CustomModelInput { provider_id, mut input_buffer }) => {
            if key.code == KeyCode::Esc {
                // Close without switching
            } else if key.code == KeyCode::Enter
                || (key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                if !input_buffer.is_empty() {
                    let mid = input_buffer.trim().to_string();
                    state.model_switch_pending = Some((provider_id.clone(), mid.clone()));
                    state.provider_id = provider_id.clone();
                    state.model_id = mid.clone();
                    state.model_name = mid.clone();
                    state.push_system(format!(
                        "Switching to custom model: {mid}\n\
                        (Make sure the model ID is correct for provider '{provider_id}')"
                    ));
                }
                // Close modal whether empty or not
            } else if key.code == KeyCode::Backspace {
                input_buffer.pop();
                state.modal = Some(Modal::CustomModelInput { provider_id, input_buffer });
            } else if let KeyCode::Char(c) = key.code {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    input_buffer.push(c);
                }
                state.modal = Some(Modal::CustomModelInput { provider_id, input_buffer });
            } else {
                state.modal = Some(Modal::CustomModelInput { provider_id, input_buffer });
            }
        }

        Some(Modal::SessionBrowser(mut browser)) => {
            let is_enter = key.code == KeyCode::Enter
                || (key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::CONTROL));

            if let Some(ref _confirm_id) = browser.confirm_delete {
                // Waiting for second `d` or Esc to confirm/cancel delete
                if key.code == KeyCode::Char('d') {
                    let id = browser.confirm_delete.take().unwrap();
                    browser.sessions.retain(|s| s.id != id);
                    if browser.selected >= browser.sessions.len() && !browser.sessions.is_empty() {
                        browser.selected = browser.sessions.len() - 1;
                    }
                    state.session_delete_pending = Some(id);
                    if browser.sessions.is_empty() {
                        // No sessions left — close
                    } else {
                        state.modal = Some(Modal::SessionBrowser(browser));
                    }
                } else {
                    browser.confirm_delete = None;
                    state.modal = Some(Modal::SessionBrowser(browser));
                }
            } else if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                // Close
            } else if key.code == KeyCode::Up {
                browser.move_up();
                state.modal = Some(Modal::SessionBrowser(browser));
            } else if key.code == KeyCode::Down {
                browser.move_down();
                state.modal = Some(Modal::SessionBrowser(browser));
            } else if is_enter {
                if let Some(id) = browser.selected_id() {
                    state.session_load_pending = Some(id.to_string());
                }
                // Close modal — session loads in main loop
            } else if key.code == KeyCode::Char('d') || key.code == KeyCode::Delete {
                if let Some(id) = browser.selected_id() {
                    browser.confirm_delete = Some(id.to_string());
                }
                state.modal = Some(Modal::SessionBrowser(browser));
            } else {
                state.modal = Some(Modal::SessionBrowser(browser));
            }
        }

        None => {}
    }
}
