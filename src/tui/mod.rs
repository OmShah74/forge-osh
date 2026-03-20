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
    event::{self, Event, KeyCode, KeyEventKind},
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
}

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
    pub total_lines: usize,      // computed during render
    pub visible_height: usize,   // computed during render
    pub streaming_text: String,
    pub provider_name: String,
    pub model_name: String,
    pub session_name: String,
    pub format_tokens: String,
    pub format_cost: String,
    pub trust_mode: bool,
    pub theme: Theme,
    pub running: bool,
    pub agent_busy: bool,
    pub key_save_pending: Option<(String, String)>,
    pub key_delete_pending: Option<String>,
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
            provider_name: session.provider_id.clone(),
            model_name: session.model_id.clone(),
            session_name: session.name.clone(),
            format_tokens: "0 tokens".to_string(),
            format_cost: "Free".to_string(),
            trust_mode: config.general.trust_mode,
            theme: Theme::from_name(&config.general.theme),
            running: true,
            agent_busy: false,
            key_save_pending: None,
            key_delete_pending: None,
        }
    }

    /// Scroll up by n lines, disabling auto-scroll
    pub fn scroll_up(&mut self, n: usize) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down by n lines
    pub fn scroll_down(&mut self, n: usize) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + n).min(max);
        // Re-enable auto-scroll if we've reached the bottom
        if self.scroll_offset >= max {
            self.auto_scroll = true;
        }
    }

    /// Maximum scroll offset
    pub fn max_scroll(&self) -> usize {
        self.total_lines.saturating_sub(self.visible_height)
    }

    /// Get effective scroll position
    pub fn effective_scroll(&self) -> usize {
        if self.auto_scroll {
            self.max_scroll()
        } else {
            self.scroll_offset.min(self.max_scroll())
        }
    }
}

/// Run the TUI event loop
pub async fn run_tui(
    config: Arc<Config>,
    provider_router: Arc<RwLock<ProviderRouter>>,
    tools: Arc<ToolRegistry>,
    session: Arc<Mutex<Session>>,
    key_store: Arc<Mutex<KeyStore>>,
) -> anyhow::Result<()> {
    // Set up terminal (no mouse capture — it causes input lag on Windows)
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create channels
    let (agent_event_tx, mut agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let (perm_req_tx, mut perm_req_rx) = mpsc::unbounded_channel::<PermissionRequest>();
    let (_perm_resp_tx, perm_resp_rx) = mpsc::unbounded_channel::<PermissionResponse>();

    // Create app state
    let mut state = {
        let sess = session.lock().await;
        AppState::new(&config, &sess)
    };

    // Welcome message
    state.messages.push(RenderedMessage {
        role: MessageRole::System,
        content: format!(
            "Welcome to forge-osh! Using {} ({}).",
            state.model_name, state.provider_name
        ),
    });

    let agent_loop = Arc::new(AgentLoop {
        provider_router: provider_router.clone(),
        tools: tools.clone(),
        session: session.clone(),
        config: config.clone(),
        event_tx: agent_event_tx.clone(),
        permission_tx: perm_req_tx,
        permission_rx: Arc::new(Mutex::new(perm_resp_rx)),
    });

    // Main event loop
    while state.running {
        // Draw
        terminal.draw(|frame| renderer::render(frame, &mut state))?;

        // Tick spinner
        if state.spinner.active {
            state.spinner.tick();
        }

        // Poll for events with a short timeout for animation
        let timeout = if state.spinner.active || state.agent_busy {
            Duration::from_millis(80)
        } else {
            Duration::from_millis(100)
        };

        // Handle agent events (non-blocking)
        while let Ok(event) = agent_event_rx.try_recv() {
            match event {
                AgentEvent::ThinkingStart => {
                    state.spinner.start(format!("{} is thinking...", state.model_name));
                    state.streaming_text.clear();
                }
                AgentEvent::Token(t) => {
                    state.spinner.stop();
                    state.streaming_text.push_str(&t);
                    state.auto_scroll = true;
                }
                AgentEvent::ToolStart { name, input } => {
                    state.spinner.stop();
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
                    state.spinner.start(format!("Running {name}..."));
                }
                AgentEvent::ToolEnd { name: _name, output, is_error } => {
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
                            "Warning: Context window {:.0}% full ({}/{} tokens)",
                            (used as f64 / limit as f64) * 100.0,
                            used,
                            limit
                        ),
                    });
                }
                AgentEvent::Done => {
                    state.spinner.stop();
                    state.agent_busy = false;
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
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("Error: {e}"),
                    });
                }
            }
        }

        // Handle permission requests
        while let Ok(req) = perm_req_rx.try_recv() {
            state.modal = Some(Modal::Confirmation {
                tool_name: req.tool_name,
                description: req.description,
                response_tx: req.response_tx,
            });
        }

        // Handle pending key operations
        if let Some((provider_id, api_key)) = state.key_save_pending.take() {
            let mut ks = key_store.lock().await;
            match ks.set(&provider_id, &api_key) {
                Ok(_) => {
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("API key saved for {provider_id}."),
                    });
                }
                Err(e) => {
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("Failed to save key: {e}"),
                    });
                }
            }
        }
        if let Some(provider_id) = state.key_delete_pending.take() {
            let mut ks = key_store.lock().await;
            match ks.delete(&provider_id) {
                Ok(_) => {
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("API key removed for {provider_id}."),
                    });
                }
                Err(e) => {
                    state.messages.push(RenderedMessage {
                        role: MessageRole::System,
                        content: format!("Failed to remove key: {e}"),
                    });
                }
            }
        }

        // Handle terminal events — drain ALL pending events before redrawing
        if event::poll(timeout)? {
            // Batch: read all available events, process them, then redraw once
            loop {
                let ev = event::read()?;
                if let Event::Key(key) = ev {
                    // On Windows, crossterm sends Press and Release events.
                    if key.kind != KeyEventKind::Press {
                        // Skip Release/Repeat, check for more events
                        if !event::poll(Duration::ZERO)? { break; }
                        continue;
                    }

                    // Handle modal-specific input
                    if state.modal.is_some() {
                        handle_modal_input(&mut state, key);
                        if !event::poll(Duration::ZERO)? { break; }
                        continue;
                    }

                    let action = input::map_key_normal(key);
                    match action {
                        Action::Submit => {
                            if !state.input.is_empty() && !state.agent_busy {
                                let text = state.input.submit();
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

                        // Scrolling
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

                        Action::Quit => {
                            if state.input.is_empty() {
                                state.running = false;
                            }
                        }
                        Action::Cancel => {
                            if state.agent_busy {
                                state.spinner.stop();
                                state.agent_busy = false;
                                state.messages.push(RenderedMessage {
                                    role: MessageRole::System,
                                    content: "Interrupted.".to_string(),
                                });
                            }
                        }
                        Action::ClearScreen => {
                            state.messages.clear();
                            state.scroll_offset = 0;
                        }
                        Action::ToggleTrustMode => {
                            state.trust_mode = !state.trust_mode;
                            state.messages.push(RenderedMessage {
                                role: MessageRole::System,
                                content: format!(
                                    "Trust mode: {}",
                                    if state.trust_mode { "ON" } else { "OFF" }
                                ),
                            });
                        }
                        Action::ShowHelp => {
                            state.modal = Some(Modal::Help);
                        }
                        Action::ShowTokenInfo => {
                            state.modal = Some(Modal::TokenInfo);
                        }
                        Action::OpenModelPicker => {
                            let router = provider_router.read().await;
                            let mut items = Vec::new();
                            for (pid, pname) in router.available_providers() {
                                let models = crate::config::models::models_for_provider(pid);
                                for m in models {
                                    items.push(picker::PickerItem::from_model_info(
                                        &m,
                                        true,
                                        pname,
                                    ));
                                }
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
                                Ok(_) => {
                                    state.messages.push(RenderedMessage {
                                        role: MessageRole::System,
                                        content: "Session saved.".to_string(),
                                    });
                                }
                                Err(e) => {
                                    state.messages.push(RenderedMessage {
                                        role: MessageRole::System,
                                        content: format!("Failed to save: {e}"),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // Non-key events (resize, etc) — just continue
                // Check if more events are queued
                if !event::poll(Duration::ZERO)? { break; }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn handle_modal_input(state: &mut AppState, key: crossterm::event::KeyEvent) {
    let modal = state.modal.take();

    match modal {
        Some(Modal::Confirmation {
            tool_name,
            description,
            response_tx,
        }) => {
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
                // Help closed
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
                        state.provider_name = item.provider_name.clone();
                        state.model_name = item.model_name.clone();
                        state.messages.push(RenderedMessage {
                            role: MessageRole::System,
                            content: format!(
                                "Switched to {} ({})",
                                item.model_name, item.provider_name
                            ),
                        });
                    }
                }
                Action::PickerCancel => {
                    if picker.filtering {
                        picker.stop_filter();
                        state.modal = Some(Modal::Picker(picker));
                    }
                    // else close
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
            // Helper: is this an Enter key? (Enter or Ctrl+M on Windows)
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
                    // Only insert if no CONTROL modifier (avoid inserting control chars)
                    if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        km.input_buffer.push(c);
                    }
                    state.modal = Some(Modal::KeyManager(km));
                } else {
                    state.modal = Some(Modal::KeyManager(km));
                }
            } else {
                if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                    // Close key manager
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
                    }
                    KeyCode::Char('d') | KeyCode::Delete => {
                        // Delete stored key
                        if let Some(entry) = km.providers.get_mut(km.selected) {
                            if entry.key_source == "stored" || entry.key_source == "env+stored" {
                                state.key_delete_pending = Some(entry.provider_id.clone());
                                // Update display
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
                    }
                    _ => {
                        state.modal = Some(Modal::KeyManager(km));
                    }
                }
            }
        }
        None => {}
    }
}
