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
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::agent::{AgentEvent, AgentLoop, PermissionRequest};
use crate::config::Config;
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
}

/// Main application state for the TUI
pub struct AppState {
    pub messages: Vec<RenderedMessage>,
    pub input: InputState,
    pub modal: Option<Modal>,
    pub spinner: SpinnerState,
    pub scroll: usize,
    pub auto_scroll: bool,
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
}

impl AppState {
    pub fn new(config: &Config, session: &Session) -> Self {
        Self {
            messages: Vec::new(),
            input: InputState::new(),
            modal: None,
            spinner: SpinnerState::new(),
            scroll: 0,
            auto_scroll: true,
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
        }
    }
}

/// Run the TUI event loop
pub async fn run_tui(
    config: Arc<Config>,
    provider_router: Arc<RwLock<ProviderRouter>>,
    tools: Arc<ToolRegistry>,
    session: Arc<Mutex<Session>>,
) -> anyhow::Result<()> {
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create channels
    let (agent_event_tx, mut agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let (perm_req_tx, mut perm_req_rx) = mpsc::unbounded_channel::<PermissionRequest>();
    let (perm_resp_tx, perm_resp_rx) = mpsc::unbounded_channel::<PermissionResponse>();

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
        terminal.draw(|frame| renderer::render(frame, &state))?;

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
                    if state.auto_scroll {
                        state.scroll = usize::MAX;
                    }
                }
                AgentEvent::ToolStart { name, input } => {
                    state.spinner.stop();
                    // Finalize streaming text
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
                AgentEvent::ToolEnd { name, output, is_error } => {
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
                    // Finalize any remaining streaming text
                    if !state.streaming_text.is_empty() {
                        state.messages.push(RenderedMessage {
                            role: MessageRole::Assistant,
                            content: std::mem::take(&mut state.streaming_text),
                        });
                    }
                    // Update cost/token display
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

        // Handle terminal events
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                // Handle modal-specific input
                if state.modal.is_some() {
                    handle_modal_input(&mut state, key);
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

                            // Run agent in background
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
                    Action::PageUp => {
                        state.scroll = state.scroll.saturating_sub(10);
                        state.auto_scroll = false;
                    }
                    Action::PageDown => {
                        state.scroll = state.scroll.saturating_add(10);
                    }
                    Action::ScrollTop => {
                        state.scroll = 0;
                        state.auto_scroll = false;
                    }
                    Action::ScrollBottom => {
                        state.scroll = usize::MAX;
                        state.auto_scroll = true;
                    }
                    Action::Quit => {
                        if state.input.is_empty() {
                            state.running = false;
                        }
                    }
                    Action::Cancel => {
                        if state.agent_busy {
                            // TODO: Cancel ongoing request
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
                        // Build picker items from available models
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
                    // Put modal back — no action taken
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
                // Help closed, modal already taken
            } else {
                state.modal = Some(Modal::Help);
            }
        }
        Some(Modal::TokenInfo) => {
            if key.code == KeyCode::Esc {
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
                        // Note: Would need to update the actual provider router here
                    }
                }
                Action::PickerCancel => {
                    if picker.filtering {
                        picker.stop_filter();
                        state.modal = Some(Modal::Picker(picker));
                    }
                    // else modal is closed (already taken)
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
        None => {}
    }
}
