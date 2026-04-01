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
            // Compact the conversation history: keep only the last N exchanges in full,
            // summarise earlier messages into a system note.
            compact_history(state, session).await;
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

/// Compact conversation history: keep the last 6 messages in full and
/// replace earlier messages with a summary notice.
async fn compact_history(state: &mut AppState, session: &Arc<Mutex<Session>>) {
    let mut sess = session.lock().await;
    let messages = sess.history.messages().to_vec();
    let keep = 6; // keep last 6 messages in full

    if messages.len() <= keep {
        state.push_system(format!(
            "Conversation has only {} messages — nothing to compact.",
            messages.len()
        ));
        return;
    }

    let removed = messages.len() - keep;
    sess.history.compact(keep);

    // Also clear most of the rendered messages, keeping the last few
    let rendered_keep = state.messages.len().saturating_sub(removed * 2);
    state.messages.drain(..rendered_keep);

    state.push_system(format!(
        "Compacted: removed {} messages from history. {} messages remain.",
        removed,
        keep
    ));
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
