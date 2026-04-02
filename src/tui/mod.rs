pub mod diff;
pub mod help;
pub mod input;
pub mod picker;
pub mod renderer;
pub mod spinner;
pub mod themes;

use std::sync::Arc;
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
    ToolResult { is_error: bool },
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

/// Main application state for the TUI
pub struct AppState {
    pub messages: Vec<RenderedMessage>,
    pub input: InputState,
    pub modal: Option<Modal>,
    pub spinner: SpinnerState,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub total_lines: usize,
    pub visible_height: usize,
    pub streaming_text: String,
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
    pub key_save_pending: Option<(String, String)>,
    pub key_delete_pending: Option<String>,
    pub model_switch_pending: Option<(String, String)>,
}

impl AppState {
    pub fn new(config: &Config, session: &Session) -> Self {
        Self {
            messages: Vec::new(),
            input: InputState::new(),
            modal: None,
            spinner: SpinnerState::new(),
            scroll_offset: 0,
            auto_scroll: true,
            total_lines: 0,
            visible_height: 0,
            streaming_text: String::new(),
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
            key_save_pending: None,
            key_delete_pending: None,
            model_switch_pending: None,
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + n).min(max);
        if self.scroll_offset >= max {
            self.auto_scroll = true;
        }
    }

    pub fn max_scroll(&self) -> usize {
        self.total_lines.saturating_sub(self.visible_height)
    }

    pub fn effective_scroll(&self) -> usize {
        if self.auto_scroll {
            self.max_scroll()
        } else {
            self.scroll_offset.min(self.max_scroll())
        }
    }

    /// Push a system message
    pub fn push_system(&mut self, msg: impl Into<String>) {
        self.messages.push(RenderedMessage {
            role: MessageRole::System,
            content: msg.into(),
        });
        self.auto_scroll = true;
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
            state.scroll_offset = 0;
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
            let models = crate::config::models::models_for_provider(&pid);
            let items: Vec<picker::PickerItem> = models
                .iter()
                .map(|m| picker::PickerItem::from_model_info(m, true, &pname))
                .collect();
            if items.is_empty() {
                state.push_system(format!("No models found for provider '{pname}'."));
            } else {
                state.modal = Some(Modal::Picker(PickerState::new(items)));
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
            compact_history_llm(state, session, provider_router).await;
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
            MessageRole::ToolResult { is_error } => {
                let label = if *is_error { "❌ Error" } else { "✅ Result" };
                lines.push(format!("### {}\n```\n{}\n```\n", label, msg.content));
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

/// LLM-based compact: summarize old messages with the active provider,
/// then replace them with the summary so the context window is freed.
async fn compact_history_llm(
    state: &mut AppState,
    session: &Arc<Mutex<Session>>,
    provider_router: &Arc<RwLock<ProviderRouter>>,
) {
    use crate::agent::compaction;

    let keep = compaction::DEFAULT_KEEP_LAST;

    let (messages, model_id, total) = {
        let sess = session.lock().await;
        let msgs = sess.history.messages().to_vec();
        let total = msgs.len();
        let model_id = sess.model_id.clone();
        (msgs, model_id, total)
    };

    if total <= keep {
        state.push_system(format!(
            "Conversation has only {total} messages — nothing to compact (threshold: {keep})."
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
// Main TUI event loop
// ---------------------------------------------------------------------------

pub async fn run_tui(
    config: Arc<Config>,
    provider_router: Arc<RwLock<ProviderRouter>>,
    tools: Arc<ToolRegistry>,
    session: Arc<Mutex<Session>>,
    key_store: Arc<Mutex<KeyStore>>,
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
        AppState::new(&config, &sess)
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
            state.messages.push(RenderedMessage {
                role: MessageRole::System,
                content: format!(
                    "Model: {}  |  Provider: {}  |  Type /help for commands.",
                    state.model_name, state.provider_name
                ),
            });
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
            Duration::from_millis(30)
        } else {
            Duration::from_millis(80)
        };

        // ---- Drain agent events (non-blocking) ----
        while let Ok(event) = agent_event_rx.try_recv() {
            match event {
                AgentEvent::ThinkingStart => {
                    state.spinner.start(format!("{} is thinking...", state.model_name));
                    state.streaming_text.clear();
                }
                AgentEvent::Token(t) => {
                    // Stop the "thinking" spinner on first token so the streaming
                    // text is visible immediately.
                    state.spinner.stop();
                    state.streaming_text.push_str(&t);
                    state.auto_scroll = true;
                }
                AgentEvent::ToolStart { name, input } => {
                    state.spinner.stop();
                    // Commit any streaming text before tool execution
                    if !state.streaming_text.is_empty() {
                        state.messages.push(RenderedMessage {
                            role: MessageRole::Assistant,
                            content: std::mem::take(&mut state.streaming_text),
                        });
                    }
                    state.messages.push(RenderedMessage {
                        role: MessageRole::ToolCall { name: name.clone() },
                        content: serde_json::to_string_pretty(&input).unwrap_or_default(),
                    });
                    state.spinner.start(format!("Running {}...", name));
                }
                AgentEvent::ToolEnd { name: _, output, is_error } => {
                    state.spinner.stop();
                    state.messages.push(RenderedMessage {
                        role: MessageRole::ToolResult { is_error },
                        content: output,
                    });
                }
                AgentEvent::ContextWarning { used, limit } => {
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
                    // Commit remaining streaming text
                    if !state.streaming_text.is_empty() {
                        state.messages.push(RenderedMessage {
                            role: MessageRole::Assistant,
                            content: std::mem::take(&mut state.streaming_text),
                        });
                    }
                    let sess = session.lock().await;
                    state.format_cost = sess.format_cost();
                    state.format_tokens = sess.format_tokens();
                }
                AgentEvent::Error(e) => {
                    state.spinner.stop();
                    state.agent_busy = false;
                    if !state.streaming_text.is_empty() {
                        state.messages.push(RenderedMessage {
                            role: MessageRole::Assistant,
                            content: std::mem::take(&mut state.streaming_text),
                        });
                    }
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("Error: {e}"),
                    });
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
            let mut ks = key_store.lock().await;
            match ks.set(&provider_id, &api_key) {
                Ok(_) => state.push_system(format!("API key saved for {provider_id}.")),
                Err(e) => state.push_system(format!("Failed to save key: {e}")),
            }
        }
        if let Some(provider_id) = state.key_delete_pending.take() {
            let mut ks = key_store.lock().await;
            match ks.delete(&provider_id) {
                Ok(_) => state.push_system(format!("API key removed for {provider_id}.")),
                Err(e) => state.push_system(format!("Failed to remove key: {e}")),
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
                                    } else {
                                        // Normal user message — send to agent loop
                                        state.messages.push(RenderedMessage {
                                            role: MessageRole::User,
                                            content: text.clone(),
                                        });
                                        state.agent_busy = true;
                                        state.auto_scroll = true;

                                        let loop_clone = agent_loop.clone();
                                        tokio::spawn(async move {
                                            let _ = loop_clone.run(text).await;
                                        });
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

                            // ---- Scrolling ----
                            Action::ScrollUp => state.scroll_up(3),
                            Action::ScrollDown => state.scroll_down(3),
                            Action::PageUp => state.scroll_up(10),
                            Action::PageDown => state.scroll_down(10),
                            Action::ScrollTop => {
                                state.scroll_offset = 0;
                                state.auto_scroll = false;
                            }
                            Action::ScrollBottom => {
                                state.auto_scroll = true;
                            }

                            // ---- Global ----
                            Action::Quit => {
                                if state.input.is_empty() {
                                    state.running = false;
                                }
                            }
                            Action::Cancel => {
                                if state.agent_busy {
                                    state.spinner.stop();
                                    state.agent_busy = false;
                                    if !state.streaming_text.is_empty() {
                                        state.messages.push(RenderedMessage {
                                            role: MessageRole::Assistant,
                                            content: std::mem::take(&mut state.streaming_text),
                                        });
                                    }
                                    state.push_system("Interrupted.");
                                }
                            }
                            Action::ClearScreen => {
                                state.messages.clear();
                                state.streaming_text.clear();
                                state.scroll_offset = 0;
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
                                let items: Vec<picker::PickerItem> = models
                                    .iter()
                                    .map(|m| picker::PickerItem::from_model_info(m, true, &pname))
                                    .collect();
                                if items.is_empty() {
                                    state.push_system(format!(
                                        "No models found for provider '{pname}' (id: {pid})."
                                    ));
                                } else {
                                    state.modal = Some(Modal::Picker(PickerState::new(items)));
                                }
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
                    role: MessageRole::ToolResult { is_error: result.is_error },
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
                    if let Some(entry) = km.providers.get_mut(km.selected) {
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
                        }
                    }
                    state.modal = Some(Modal::KeyManager(km));
                } else {
                    state.modal = Some(Modal::KeyManager(km));
                }
            }
        }

        None => {}
    }
}
