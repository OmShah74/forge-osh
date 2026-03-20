use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use super::themes::Theme;
use super::{AppState, KeyManagerState, Modal, MessageRole};

/// Render the entire TUI
pub fn render(frame: &mut Frame, state: &mut AppState) {
    let theme = &state.theme.clone();

    // Main layout: header | conversation | input | status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header
            Constraint::Min(5),    // conversation
            Constraint::Length(4), // input
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
            Modal::KeyManager(km) => {
                render_key_manager(frame, km, theme);
            }
        }
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let header_text = format!(
        " forge-osh | {} ({}) | {} | {} | {}",
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

fn render_conversation(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    for msg in &state.messages {
        match &msg.role {
            MessageRole::User => {
                lines.push(Line::from(vec![
                    Span::styled(
                        "You:",
                        Style::default().fg(theme.user_msg_fg).add_modifier(Modifier::BOLD),
                    ),
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
                    Span::styled(
                        "forge:",
                        Style::default().fg(theme.assistant_msg_fg).add_modifier(Modifier::BOLD),
                    ),
                ]));
                let mut in_code_block = false;
                for text_line in msg.content.lines() {
                    if text_line.trim_start().starts_with("```") {
                        in_code_block = !in_code_block;
                        lines.push(Line::from(Span::styled(
                            format!("  {text_line}"),
                            Style::default().fg(theme.muted_fg),
                        )));
                    } else if in_code_block {
                        lines.push(Line::from(Span::styled(
                            format!("  {text_line}"),
                            Style::default().fg(theme.added_fg),
                        )));
                    } else {
                        lines.push(Line::from(Span::styled(
                            format!("  {text_line}"),
                            Style::default().fg(theme.assistant_msg_fg),
                        )));
                    }
                }
                lines.push(Line::from(""));
            }
            MessageRole::ToolCall { name } => {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  [{name}]"),
                        Style::default().fg(theme.tool_name_fg).add_modifier(Modifier::BOLD),
                    ),
                ]));
                render_tool_input(&mut lines, &msg.content, theme);
                lines.push(Line::from(""));
            }
            MessageRole::ToolResult { is_error } => {
                let color = if *is_error { theme.error_fg } else { theme.added_fg };
                let status = if *is_error { "Error" } else { "OK" };
                lines.push(Line::from(Span::styled(
                    format!("  [Result: {status}]"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )));
                let max_lines = 15;
                let content_lines: Vec<&str> = msg.content.lines().collect();
                let show_count = content_lines.len().min(max_lines);
                for text_line in &content_lines[..show_count] {
                    lines.push(Line::from(Span::styled(
                        format!("    {text_line}"),
                        Style::default().fg(theme.muted_fg),
                    )));
                }
                if content_lines.len() > max_lines {
                    lines.push(Line::from(Span::styled(
                        format!("    ... ({} more lines)", content_lines.len() - max_lines),
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
            Span::styled("forge:", Style::default().fg(theme.assistant_msg_fg).add_modifier(Modifier::BOLD)),
        ]));
        let mut in_code_block = false;
        for text_line in state.streaming_text.lines() {
            if text_line.trim_start().starts_with("```") {
                in_code_block = !in_code_block;
                lines.push(Line::from(Span::styled(
                    format!("  {text_line}"),
                    Style::default().fg(theme.muted_fg),
                )));
            } else if in_code_block {
                lines.push(Line::from(Span::styled(
                    format!("  {text_line}"),
                    Style::default().fg(theme.added_fg),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("  {text_line}"),
                    Style::default().fg(theme.assistant_msg_fg),
                )));
            }
        }
    }

    // Update state with computed line metrics
    let total_lines = lines.len();
    let visible_height = area.height as usize;
    state.total_lines = total_lines;
    state.visible_height = visible_height;

    // Calculate effective scroll
    let scroll = state.effective_scroll();

    let conversation = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    frame.render_widget(conversation, area);

    // Scrollbar
    if total_lines > visible_height {
        let max_scroll = state.max_scroll();
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("^"))
            .end_symbol(Some("v"));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Pretty-print tool call input JSON
fn render_tool_input(lines: &mut Vec<Line>, json_str: &str, theme: &Theme) {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(obj) = val.as_object() {
            for (key, value) in obj {
                match value {
                    serde_json::Value::String(s) => {
                        if s.contains('\n') || s.len() > 80 {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("    {key}: "),
                                    Style::default().fg(theme.tool_name_fg),
                                ),
                            ]));
                            for content_line in s.lines().take(20) {
                                lines.push(Line::from(Span::styled(
                                    format!("      {content_line}"),
                                    Style::default().fg(theme.added_fg),
                                )));
                            }
                            let line_count = s.lines().count();
                            if line_count > 20 {
                                lines.push(Line::from(Span::styled(
                                    format!("      ... ({} more lines)", line_count - 20),
                                    Style::default().fg(theme.muted_fg),
                                )));
                            }
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("    {key}: "),
                                    Style::default().fg(theme.tool_name_fg),
                                ),
                                Span::styled(
                                    s.to_string(),
                                    Style::default().fg(theme.fg),
                                ),
                            ]));
                        }
                    }
                    _ => {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("    {key}: "),
                                Style::default().fg(theme.tool_name_fg),
                            ),
                            Span::styled(
                                value.to_string(),
                                Style::default().fg(theme.fg),
                            ),
                        ]));
                    }
                }
            }
        } else {
            let pretty = serde_json::to_string_pretty(&val).unwrap_or_else(|_| json_str.to_string());
            for line in pretty.lines().take(10) {
                lines.push(Line::from(Span::styled(
                    format!("    {line}"),
                    Style::default().fg(theme.muted_fg),
                )));
            }
        }
    } else {
        for line in json_str.lines().take(10) {
            lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(theme.muted_fg),
            )));
        }
    }
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
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(input, area);

    // Set cursor position
    let cursor_x = area.x + 1 + state.input.cursor as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x.min(area.x + area.width - 2), cursor_y));
}

fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let trust = if state.trust_mode { "ON" } else { "OFF" };
    let busy = if state.agent_busy { " Working..." } else { "" };

    let scroll_info = if state.total_lines > state.visible_height {
        let pct = if state.max_scroll() > 0 {
            (state.effective_scroll() as f64 / state.max_scroll() as f64 * 100.0) as u16
        } else {
            100
        };
        format!(" {pct}%")
    } else {
        String::new()
    };

    let status = format!(
        " ^C Cancel | ^M Model | ^P Provider | ^K Keys | ^T Trust [{trust}]{busy}{scroll_info} | F1 Help"
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
        format!(" Models (filter: {}) ", picker.filter)
    } else {
        " Models (/ filter, Enter select, Esc close) ".to_string()
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

fn render_key_manager(frame: &mut Frame, km: &KeyManagerState, theme: &Theme) {
    let area = centered_rect(65, 65, frame.area());
    frame.render_widget(Clear, area);

    if km.editing {
        let provider = km.selected_provider()
            .map(|p| p.provider_id.as_str())
            .unwrap_or("unknown");
        let masked: String = if km.input_buffer.is_empty() {
            "(type your key here)".to_string()
        } else {
            let len = km.input_buffer.len();
            if len <= 8 {
                "*".repeat(len)
            } else {
                // Show first 4 and last 4
                format!(
                    "{}...{}",
                    &km.input_buffer[..4],
                    &km.input_buffer[len - 4..]
                )
            }
        };
        let text = format!(
            "Set API key for: {provider}\n\n\
             Key: {masked}\n\n\
             [Enter] Save    [Esc] Cancel"
        );
        let dialog = Paragraph::new(text)
            .style(Style::default().fg(theme.fg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.warning_fg))
                    .title(" Set API Key "),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(dialog, area);
    } else {
        let items: Vec<ListItem> = km.providers.iter().enumerate().map(|(i, entry)| {
            let style = if i == km.selected {
                Style::default().fg(theme.fg).bg(theme.highlight_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            let (icon, status) = match entry.key_source.as_str() {
                "env+stored" => ("+", "env + saved"),
                "env" => ("+", "env var   "),
                "stored" => ("+", "saved     "),
                _ => ("-", "not set   "),
            };
            let text = format!(" {icon} {:<15} [{status}]", entry.provider_id);
            ListItem::new(text).style(style)
        }).collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(" API Keys  [Enter/e] Set  [d/Del] Delete  [Esc/q] Close "),
        );
        frame.render_widget(list, area);
    }
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
