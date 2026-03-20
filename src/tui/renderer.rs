use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::themes::Theme;
use super::{AppState, Modal, RenderedMessage, MessageRole};

/// Render the entire TUI
pub fn render(frame: &mut Frame, state: &AppState) {
    let theme = &state.theme;

    // Main layout: header | conversation | input | status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // header
            Constraint::Min(5),    // conversation
            Constraint::Length(3), // input
            Constraint::Length(1), // status bar
        ])
        .split(frame.area());

    render_header(frame, chunks[0], state, theme);
    render_conversation(frame, chunks[1], state, theme);
    render_input(frame, chunks[2], state, theme);
    render_status_bar(frame, chunks[3], state, theme);

    // Render modal overlays
    if let Some(modal) = &state.modal {
        match modal {
            Modal::Confirmation { tool_name, description, .. } => {
                render_confirmation(frame, tool_name, description, theme);
            }
            Modal::Help => {
                render_help(frame, theme);
            }
            Modal::Picker(picker) => {
                render_picker(frame, picker, theme);
            }
            Modal::TokenInfo => {
                render_token_info(frame, state, theme);
            }
        }
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let header_text = format!(
        " forge-osh | {} ({}) | session: {} | {} | {}",
        state.model_name,
        state.provider_name,
        state.session_name,
        state.format_tokens,
        state.format_cost,
    );

    let trust_indicator = if state.trust_mode { " [TRUST]" } else { "" };

    let header = Paragraph::new(format!("{header_text}{trust_indicator}"))
        .style(Style::default().fg(theme.header_fg).bg(theme.header_bg));

    frame.render_widget(header, area);
}

fn render_conversation(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    for msg in &state.messages {
        match &msg.role {
            MessageRole::User => {
                lines.push(Line::from(vec![
                    Span::styled("You", Style::default().fg(theme.user_msg_fg).add_modifier(Modifier::BOLD)),
                    Span::raw(": "),
                ]));
                for text_line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {text_line}"),
                        Style::default().fg(theme.user_msg_fg),
                    )));
                }
                lines.push(Line::from(""));
            }
            MessageRole::Assistant => {
                lines.push(Line::from(vec![
                    Span::styled("forge", Style::default().fg(theme.assistant_msg_fg).add_modifier(Modifier::BOLD)),
                    Span::raw(": "),
                ]));
                for text_line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {text_line}"),
                        Style::default().fg(theme.assistant_msg_fg),
                    )));
                }
                lines.push(Line::from(""));
            }
            MessageRole::ToolCall { name } => {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  [Tool: {name}]"),
                        Style::default().fg(theme.tool_name_fg),
                    ),
                ]));
                // Show truncated content
                let preview: String = msg.content.lines().take(5).collect::<Vec<_>>().join("\n");
                for text_line in preview.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("    {text_line}"),
                        Style::default().fg(theme.muted_fg),
                    )));
                }
                if msg.content.lines().count() > 5 {
                    lines.push(Line::from(Span::styled(
                        "    ... (truncated)",
                        Style::default().fg(theme.muted_fg),
                    )));
                }
                lines.push(Line::from(""));
            }
            MessageRole::ToolResult { is_error } => {
                let color = if *is_error { theme.error_fg } else { theme.added_fg };
                let status = if *is_error { "ERROR" } else { "OK" };
                lines.push(Line::from(Span::styled(
                    format!("  [Result: {status}]"),
                    Style::default().fg(color),
                )));
                let preview: String = msg.content.lines().take(3).collect::<Vec<_>>().join("\n");
                for text_line in preview.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("    {text_line}"),
                        Style::default().fg(theme.muted_fg),
                    )));
                }
                lines.push(Line::from(""));
            }
            MessageRole::System => {
                lines.push(Line::from(Span::styled(
                    format!("  {}", msg.content),
                    Style::default().fg(theme.warning_fg),
                )));
                lines.push(Line::from(""));
            }
        }
    }

    // Spinner
    if state.spinner.active {
        lines.push(Line::from(Span::styled(
            format!("  {} {}", state.spinner.current_frame(), state.spinner.message),
            Style::default().fg(theme.spinner_fg),
        )));
    }

    // Streaming text
    if !state.streaming_text.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("forge", Style::default().fg(theme.assistant_msg_fg).add_modifier(Modifier::BOLD)),
            Span::raw(": "),
        ]));
        for text_line in state.streaming_text.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {text_line}"),
                Style::default().fg(theme.assistant_msg_fg),
            )));
        }
    }

    let total_lines = lines.len() as u16;
    let visible_height = area.height;
    let scroll = if total_lines > visible_height {
        state.scroll.min((total_lines - visible_height) as usize) as u16
    } else {
        0
    };

    let conversation = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(conversation, area);
}

fn render_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let input_text = if state.input.text.is_empty() {
        "Type your message... (Ctrl+D to exit, F1 for help)"
    } else {
        &state.input.text
    };

    let style = if state.input.text.is_empty() {
        Style::default().fg(theme.muted_fg)
    } else {
        Style::default().fg(theme.fg)
    };

    let input = Paragraph::new(input_text)
        .style(style)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.border_fg))
                .title(Span::styled(" > ", Style::default().fg(theme.prompt_fg))),
        );

    frame.render_widget(input, area);

    // Set cursor position
    let cursor_x = area.x + 3 + state.input.cursor as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x.min(area.x + area.width - 1), cursor_y));
}

fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let trust = if state.trust_mode {
        "[Trust: ON]"
    } else {
        "[Trust: OFF]"
    };

    let status = format!(
        " Ctrl+C Cancel | Ctrl+M Model | Ctrl+P Provider | Ctrl+T Trust {trust} | F1 Help"
    );

    let bar = Paragraph::new(status)
        .style(Style::default().fg(theme.status_fg).bg(theme.status_bg));

    frame.render_widget(bar, area);
}

fn render_confirmation(frame: &mut Frame, tool_name: &str, description: &str, theme: &Theme) {
    let area = centered_rect(60, 40, frame.area());
    frame.render_widget(Clear, area);

    let text = format!(
        "CONFIRMATION REQUIRED\n\nTool: {tool_name}\n\n{description}\n\n[Y] Yes  [N] No  [A] Always  [T] Trust mode"
    );

    let dialog = Paragraph::new(text)
        .style(Style::default().fg(theme.warning_fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.warning_fg))
                .title(" Confirm "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(dialog, area);
}

fn render_help(frame: &mut Frame, theme: &Theme) {
    let area = centered_rect(80, 80, frame.area());
    frame.render_widget(Clear, area);

    let help = Paragraph::new(super::help::help_text())
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(" Help (Esc to close) "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(help, area);
}

fn render_picker(frame: &mut Frame, picker: &super::picker::PickerState, theme: &Theme) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = picker
        .filtered_items()
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == picker.selected {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            let connected = if item.connected { "*" } else { " " };
            let text = format!(
                " {connected} {:<20} {:<30} {}",
                item.provider_name, item.model_name, item.cost_display
            );
            ListItem::new(text).style(style)
        })
        .collect();

    let title = if picker.filtering {
        format!(" Select Model (filter: {}) ", picker.filter)
    } else {
        " Select Model (/ to filter, Enter to select, Esc to cancel) ".to_string()
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_fg))
            .title(title),
    );

    frame.render_widget(list, area);
}

fn render_token_info(frame: &mut Frame, state: &AppState, theme: &Theme) {
    let area = centered_rect(50, 30, frame.area());
    frame.render_widget(Clear, area);

    let text = format!(
        "Token Usage & Cost\n\nProvider: {}\nModel: {}\nTokens: {}\nCost: {}\n\nPress Esc to close",
        state.provider_name,
        state.model_name,
        state.format_tokens,
        state.format_cost,
    );

    let info = Paragraph::new(text)
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(" Token Info "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(info, area);
}

/// Helper to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
