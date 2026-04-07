use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use super::themes::Theme;
use super::{AppState, KeyManagerState, Modal, MessageRole, OSH_SPLASH_LINES};

/// Render the entire TUI
pub fn render(frame: &mut Frame, state: &mut AppState) {
    let theme = state.theme.clone();
    let area = frame.area();

    // Fill the entire frame with the theme's background colour.
    // Without this, cells not covered by any widget keep the terminal's native
    // background — on light terminals, the dark theme's light-coloured text
    // would be invisible against the white background.
    if theme.bg != Color::Reset {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.bg).fg(theme.fg)),
            area,
        );
    }

    // Compute input area height based on content (min 3, max 8 rows)
    let input_lines = count_input_lines(&state.input.text, area.width.saturating_sub(4) as usize);
    let input_height = (input_lines + 2).max(3).min(8) as u16; // +2 for borders/padding

    // Main layout: header | conversation | input | status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),              // header
            Constraint::Min(5),                 // conversation
            Constraint::Length(input_height),   // input (dynamic)
            Constraint::Length(1),              // status bar
        ])
        .split(area);

    render_header(frame, chunks[0], state, &theme);
    render_conversation(frame, chunks[1], state, &theme);
    render_input(frame, chunks[2], state, &theme);
    render_status_bar(frame, chunks[3], state, &theme);

    // Render modal overlays on top
    if let Some(modal) = &state.modal {
        match modal {
            Modal::Confirmation { tool_name, description, .. } => {
                render_confirmation(frame, tool_name, description, &theme);
            }
            Modal::Help => {
                render_help(frame, &theme);
            }
            Modal::Picker(picker) => {
                render_picker(frame, picker, &theme);
            }
            Modal::TokenInfo => {
                render_token_info(frame, state, &theme);
            }
            Modal::KeyManager(km) => {
                render_key_manager(frame, km, &theme);
            }
            Modal::CustomModelInput { provider_id, input_buffer } => {
                render_custom_model_input(frame, provider_id, input_buffer, &theme);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

fn render_header(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let trust_indicator = if state.trust_mode { " [TRUST]" } else { "" };
    let busy_indicator = if state.agent_busy { " ●" } else { "" };

    let header_text = format!(
        " forge-osh  {}  {}  {}  {}  {}{}{}",
        state.model_name,
        state.provider_name,
        state.session_name,
        state.format_tokens,
        state.format_cost,
        trust_indicator,
        busy_indicator,
    );

    let header = Paragraph::new(header_text)
        .style(Style::default().fg(theme.header_fg).bg(theme.header_bg));

    frame.render_widget(header, area);
}

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

fn render_conversation(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    let wrap_width = area.width.saturating_sub(2) as usize; // subtract scrollbar

    for msg in &state.messages {
        match &msg.role {
            // ------------------------------------------------------------------
            // OSH ASCII-art splash banner shown once at startup.
            // Box/frame characters are rendered in border_fg; the block-letter
            // '#' characters are highlighted in the theme's prompt colour so
            // they stand out against the frame.
            // ------------------------------------------------------------------
            MessageRole::Splash => {
                lines.push(Line::from(""));
                for splash_line in OSH_SPLASH_LINES {
                    let mut spans: Vec<Span> = Vec::new();
                    let mut segment = String::new();
                    let mut in_hash = false;

                    for ch in splash_line.chars() {
                        let ch_is_hash = ch == '#';
                        if ch_is_hash != in_hash {
                            if !segment.is_empty() {
                                let color = if in_hash { theme.prompt_fg } else { theme.border_fg };
                                spans.push(Span::styled(
                                    segment.clone(),
                                    Style::default().fg(color).add_modifier(
                                        if in_hash { Modifier::BOLD } else { Modifier::empty() },
                                    ),
                                ));
                                segment.clear();
                            }
                            in_hash = ch_is_hash;
                        }
                        segment.push(ch);
                    }
                    if !segment.is_empty() {
                        let color = if in_hash { theme.prompt_fg } else { theme.border_fg };
                        spans.push(Span::styled(
                            segment,
                            Style::default().fg(color).add_modifier(
                                if in_hash { Modifier::BOLD } else { Modifier::empty() },
                            ),
                        ));
                    }
                    lines.push(Line::from(spans));
                }
                lines.push(Line::from(""));
            }

            MessageRole::User => {
                lines.push(Line::from(vec![
                    Span::styled(
                        " You ",
                        Style::default()
                            .fg(theme.header_bg)
                            .bg(theme.user_msg_fg)
                            .add_modifier(Modifier::BOLD),
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
                        " forge ",
                        Style::default()
                            .fg(theme.header_bg)
                            .bg(theme.assistant_msg_fg)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                render_assistant_content(&mut lines, &msg.content, theme);
                lines.push(Line::from(""));
            }

            MessageRole::ToolCall { name } => {
                lines.push(Line::from(vec![
                    Span::styled(" ", Style::default()),
                    Span::styled(
                        format!(" {} ", name),
                        Style::default()
                            .fg(theme.header_bg)
                            .bg(theme.tool_name_fg)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                render_tool_input(&mut lines, &msg.content, theme);
                lines.push(Line::from(""));
            }

            MessageRole::ToolResult { is_error } => {
                let (color, status) = if *is_error {
                    (theme.error_fg, "Error")
                } else {
                    (theme.added_fg, "Done")
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  Result: {status}"),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]));
                let content_lines: Vec<&str> = msg.content.lines().collect();
                let max_lines = 20;
                for text_line in content_lines.iter().take(max_lines) {
                    lines.push(Line::from(Span::styled(
                        format!("    {text_line}"),
                        Style::default().fg(theme.muted_fg),
                    )));
                }
                if content_lines.len() > max_lines {
                    lines.push(Line::from(Span::styled(
                        format!("    ... ({} more lines hidden)", content_lines.len() - max_lines),
                        Style::default().fg(theme.muted_fg).add_modifier(Modifier::ITALIC),
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

    // Streaming text (currently being generated)
    if !state.streaming_text.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                " forge ",
                Style::default()
                    .fg(theme.header_bg)
                    .bg(theme.assistant_msg_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        render_assistant_content(&mut lines, &state.streaming_text, theme);
    }

    // Spinner (thinking indicator)
    if state.spinner.active {
        let frame_char = state.spinner.current_frame();
        lines.push(Line::from(Span::styled(
            format!("  {frame_char} {}", state.spinner.message),
            Style::default().fg(theme.spinner_fg),
        )));
    }

    // Raw line count drives scroll — no wrap estimation, no artificial ceiling.
    // scroll_offset is "lines scrolled up from bottom" and is unbounded: when it
    // exceeds the actual content height, effective_scroll() saturates to 0 via
    // saturating_sub, which simply shows the very first line.  This means users
    // can scroll freely through any amount of content — 100 lines or 1 000 000 —
    // without any hard limit being imposed by the renderer.
    let total_raw = lines.len();
    let visible_height = area.height as usize;

    state.total_lines = total_raw;
    state.visible_height = visible_height;

    // effective_scroll() = max_scroll().saturating_sub(scroll_offset)
    // → 0 when scrolled to or past the very top (natural floor, not a cap)
    // → max_scroll() when at the bottom (auto-scroll position)
    let scroll_start = state.effective_scroll();

    // Include a small line buffer beyond visible_height so that lines which
    // word-wrap to multiple visual rows are not clipped at the bottom edge.
    // Ratatui hard-clips rendering at the widget boundary, so this is safe.
    let extra = visible_height / 3 + 8;
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll_start)
        .take(visible_height + extra)
        .collect();

    let conversation = Paragraph::new(Text::from(visible_lines))
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });

    frame.render_widget(conversation, area);

    // Scrollbar: shown whenever content is taller than the viewport.
    // Content size = total_raw lines; current position = scroll_start (from top).
    if total_raw > visible_height {
        let content_size = total_raw.saturating_sub(visible_height);
        let mut scrollbar_state = ScrollbarState::new(content_size).position(scroll_start);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("^"))
            .end_symbol(Some("v"));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Render assistant message content with basic markdown support.
fn render_assistant_content(lines: &mut Vec<Line>, content: &str, theme: &Theme) {
    let mut in_code_block = false;
    let mut code_lang = String::new();

    for text_line in content.lines() {
        let trimmed = text_line.trim_start();

        // Code block fence
        if trimmed.starts_with("```") {
            if in_code_block {
                // Closing fence
                in_code_block = false;
                lines.push(Line::from(Span::styled(
                    "  └─────────────────────",
                    Style::default().fg(theme.border_fg),
                )));
                code_lang.clear();
            } else {
                // Opening fence
                in_code_block = true;
                code_lang = trimmed.trim_start_matches('`').trim().to_string();
                let lang_label = if code_lang.is_empty() {
                    String::new()
                } else {
                    format!(" {code_lang}")
                };
                lines.push(Line::from(Span::styled(
                    format!("  ┌─────────────{lang_label}"),
                    Style::default().fg(theme.border_fg),
                )));
            }
            continue;
        }

        if in_code_block {
            lines.push(Line::from(Span::styled(
                format!("  │ {text_line}"),
                Style::default().fg(theme.added_fg),
            )));
            continue;
        }

        // Empty line
        if text_line.trim().is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Headings
        if let Some(heading) = trimmed.strip_prefix("### ") {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    heading.to_string(),
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    heading.to_string(),
                    Style::default().fg(theme.tool_name_fg).add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    heading.to_string(),
                    Style::default().fg(theme.warning_fg).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
            ]));
            continue;
        }

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            lines.push(Line::from(Span::styled(
                "  ──────────────────────────────────────",
                Style::default().fg(theme.border_fg),
            )));
            continue;
        }

        // Bullet points (- or * or +)
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let indent = text_line.len() - trimmed.len();
            let bullet_content = &trimmed[2..];
            let prefix = format!("{}  • ", " ".repeat(indent));
            let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.muted_fg))];
            spans.extend(parse_inline_markdown(bullet_content, theme));
            lines.push(Line::from(spans));
            continue;
        }

        // Numbered list (1. 2. etc.)
        if let Some(rest) = trimmed.split_once(". ") {
            if rest.0.parse::<u32>().is_ok() {
                let indent = text_line.len() - trimmed.len();
                let prefix = format!("{}  {}. ", " ".repeat(indent), rest.0);
                let mut spans = vec![Span::styled(prefix, Style::default().fg(theme.muted_fg))];
                spans.extend(parse_inline_markdown(rest.1, theme));
                lines.push(Line::from(spans));
                continue;
            }
        }

        // Regular text with inline formatting
        let mut spans = vec![Span::raw("  ")];
        spans.extend(parse_inline_markdown(text_line, theme));
        lines.push(Line::from(spans));
    }

    // If we ended inside a code block, close it
    if in_code_block {
        lines.push(Line::from(Span::styled(
            "  └─────────────────────",
            Style::default().fg(theme.border_fg),
        )));
    }
}

/// Parse a line with inline markdown: **bold**, `code`, *italic*.
/// Returns a Vec of Spans with appropriate styles.
fn parse_inline_markdown(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut current = String::new();

    while i < chars.len() {
        // **bold** or __bold__
        if i + 1 < chars.len()
            && ((chars[i] == '*' && chars[i + 1] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_'))
        {
            let marker = chars[i];
            if !current.is_empty() {
                spans.push(Span::styled(
                    current.clone(),
                    Style::default().fg(theme.assistant_msg_fg),
                ));
                current.clear();
            }
            i += 2;
            while i + 1 < chars.len()
                && !(chars[i] == marker && chars[i + 1] == marker)
            {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(
                current.clone(),
                Style::default()
                    .fg(theme.fg)
                    .add_modifier(Modifier::BOLD),
            ));
            current.clear();
            if i + 1 < chars.len() {
                i += 2; // skip closing ** or __
            }
            continue;
        }

        // *italic* or _italic_ (single)
        if (chars[i] == '*' || chars[i] == '_')
            && (i == 0 || chars[i - 1] != chars[i])
            && i + 1 < chars.len()
            && chars[i + 1] != ' '
        {
            let marker = chars[i];
            if !current.is_empty() {
                spans.push(Span::styled(
                    current.clone(),
                    Style::default().fg(theme.assistant_msg_fg),
                ));
                current.clear();
            }
            i += 1;
            while i < chars.len() && chars[i] != marker {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(
                current.clone(),
                Style::default()
                    .fg(theme.assistant_msg_fg)
                    .add_modifier(Modifier::ITALIC),
            ));
            current.clear();
            if i < chars.len() {
                i += 1;
            }
            continue;
        }

        // `inline code`
        if chars[i] == '`' {
            if !current.is_empty() {
                spans.push(Span::styled(
                    current.clone(),
                    Style::default().fg(theme.assistant_msg_fg),
                ));
                current.clear();
            }
            i += 1;
            while i < chars.len() && chars[i] != '`' {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(
                current.clone(),
                Style::default().fg(theme.added_fg),
            ));
            current.clear();
            if i < chars.len() {
                i += 1;
            }
            continue;
        }

        current.push(chars[i]);
        i += 1;
    }

    if !current.is_empty() {
        spans.push(Span::styled(
            current,
            Style::default().fg(theme.assistant_msg_fg),
        ));
    }

    spans
}

/// Pretty-print tool call input JSON with key-value formatting.
fn render_tool_input(lines: &mut Vec<Line>, json_str: &str, theme: &Theme) {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(obj) = val.as_object() {
            for (key, value) in obj {
                match value {
                    serde_json::Value::String(s) if s.contains('\n') || s.len() > 60 => {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("    {key}: "),
                                Style::default().fg(theme.tool_name_fg),
                            ),
                        ]));
                        let content_lines: Vec<&str> = s.lines().collect();
                        let max_preview = 25;
                        for content_line in content_lines.iter().take(max_preview) {
                            lines.push(Line::from(Span::styled(
                                format!("      {content_line}"),
                                Style::default().fg(theme.added_fg),
                            )));
                        }
                        if content_lines.len() > max_preview {
                            lines.push(Line::from(Span::styled(
                                format!("      ... ({} more lines)", content_lines.len() - max_preview),
                                Style::default().fg(theme.muted_fg),
                            )));
                        }
                    }
                    serde_json::Value::String(s) => {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("    {key}: "),
                                Style::default().fg(theme.tool_name_fg),
                            ),
                            Span::styled(s.clone(), Style::default().fg(theme.fg)),
                        ]));
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
        }
    } else {
        // Not JSON, show as-is (truncated)
        for line in json_str.lines().take(10) {
            lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(theme.muted_fg),
            )));
        }
    }
}

// ---------------------------------------------------------------------------
// Input area
// ---------------------------------------------------------------------------

fn render_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let inner_width = area.width.saturating_sub(4) as usize;

    let (display_text, text_style) = if state.input.text.is_empty() {
        if state.agent_busy {
            ("Agent is working... (Ctrl+C to cancel)", Style::default().fg(theme.muted_fg))
        } else {
            ("Type a message or /help for commands...", Style::default().fg(theme.muted_fg))
        }
    } else {
        (state.input.text.as_str(), Style::default().fg(theme.fg))
    };

    let title = if state.agent_busy {
        Span::styled(" ⏳ ", Style::default().fg(theme.spinner_fg).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" ❯ ", Style::default().fg(theme.prompt_fg).add_modifier(Modifier::BOLD))
    };

    let input = Paragraph::new(display_text)
        .style(text_style)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.border_fg))
                .title(title),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(input, area);

    // Compute cursor position accounting for newlines and text wrapping.
    if !state.input.text.is_empty() {
        let text_before_cursor = &state.input.text[..state.input.cursor];
        let newlines = text_before_cursor.chars().filter(|&c| c == '\n').count();
        let last_line_start = text_before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let last_line = &text_before_cursor[last_line_start..];
        let col = unicode_width::UnicodeWidthStr::width(last_line);

        let (wrap_row, wrap_col) = if inner_width > 0 {
            (col / inner_width, col % inner_width)
        } else {
            (0, col)
        };

        let cursor_x = (area.x + 1 + wrap_col as u16).min(area.x + area.width.saturating_sub(2));
        let cursor_y = (area.y + 1 + newlines as u16 + wrap_row as u16)
            .min(area.y + area.height.saturating_sub(1));

        frame.set_cursor_position((cursor_x, cursor_y));
    } else {
        frame.set_cursor_position((area.x + 1, area.y + 1));
    }
}

/// Count the number of rendered lines the input text will take.
fn count_input_lines(text: &str, wrap_width: usize) -> usize {
    if text.is_empty() {
        return 1;
    }
    let mut total = 0usize;
    for line in text.split('\n') {
        let w = unicode_width::UnicodeWidthStr::width(line);
        let rows = if wrap_width == 0 || w == 0 {
            1
        } else {
            (w + wrap_width - 1) / wrap_width
        };
        total += rows;
    }
    total.max(1)
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let trust = if state.trust_mode { "ON" } else { "OFF" };

    let scroll_info = if state.total_lines > state.visible_height {
        let current = state.effective_scroll() + 1;
        let total = state.total_lines;
        let vim_indicator = if state.vim_normal_mode { " [VIM]" } else { "" };
        format!(" L{current}/{total}{vim_indicator}")
    } else {
        if state.vim_normal_mode { " [VIM]".to_string() } else { String::new() }
    };

    let status = format!(
        " ^C Cancel  ^O Model  ^P Provider  ^K Keys  ^B Cost  ^R Theme  ^T Trust[{trust}]  /help Cmds{scroll_info}"
    );

    let bar = Paragraph::new(status)
        .style(Style::default().fg(theme.status_fg).bg(theme.status_bg));

    frame.render_widget(bar, area);
}

// ---------------------------------------------------------------------------
// Modals
// ---------------------------------------------------------------------------

fn render_confirmation(frame: &mut Frame, tool_name: &str, description: &str, theme: &Theme) {
    let area = centered_rect(62, 45, frame.area());
    frame.render_widget(Clear, area);

    let text = format!(
        "Permission Required\n\nTool: {tool_name}\n\n{description}\n\n[Y/Enter] Allow  [N/Esc] Deny  [A] Always Allow  [T] Trust Mode"
    );

    let dialog = Paragraph::new(text)
        .style(Style::default().fg(theme.warning_fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.warning_fg))
                .title(" Confirm Action "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(dialog, area);
}

fn render_help(frame: &mut Frame, theme: &Theme) {
    let area = centered_rect(82, 85, frame.area());
    frame.render_widget(Clear, area);

    let help = Paragraph::new(super::help::help_text())
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(" Help  (Esc or q to close) "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(help, area);
}

fn render_picker(frame: &mut Frame, picker: &super::picker::PickerState, theme: &Theme) {
    let area = centered_rect(72, 72, frame.area());
    frame.render_widget(Clear, area);

    let filtered = picker.filtered_items();
    let items: Vec<ListItem> = filtered
        .iter()
        .map(|item| {
            let active_mark = if item.connected { "●" } else { "○" };
            let text = format!(
                " {active_mark} {:<20} {:<30} {}",
                item.provider_name, item.model_name, item.cost_display
            );
            ListItem::new(text).style(Style::default().fg(theme.fg))
        })
        .collect();

    let title = if picker.filtering {
        format!(" Models  filter: {} ", picker.filter)
    } else {
        " Models  / filter   ↑↓ navigate   Enter select   Esc close ".to_string()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(title),
        )
        .highlight_style(
            Style::default()
                .fg(theme.fg)
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        );

    let mut list_state = ListState::default().with_selected(Some(picker.selected));
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_token_info(frame: &mut Frame, state: &AppState, theme: &Theme) {
    let area = centered_rect(52, 35, frame.area());
    frame.render_widget(Clear, area);

    let text = format!(
        "Usage & Cost\n\nProvider : {}\nModel    : {}\nTokens   : {}\nCost     : {}\n\nPress Esc to close",
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
    let area = centered_rect(66, 68, frame.area());
    frame.render_widget(Clear, area);

    if km.editing {
        let provider = km
            .selected_provider()
            .map(|p| p.provider_id.as_str())
            .unwrap_or("unknown");

        let masked = if km.input_buffer.is_empty() {
            "(type your API key here)".to_string()
        } else {
            let len = km.input_buffer.len();
            if len <= 8 {
                "*".repeat(len)
            } else {
                format!("{}...{}", &km.input_buffer[..4], &km.input_buffer[len - 4..])
            }
        };

        let text = format!(
            "Set API key for: {provider}\n\nKey: {masked}\n\n[Enter] Save    [Esc] Cancel"
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
        let items: Vec<ListItem> = km
            .providers
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = if i == km.selected {
                    Style::default()
                        .fg(theme.fg)
                        .bg(theme.highlight_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg)
                };

                let (icon, status) = match entry.key_source.as_str() {
                    "env+stored" => ("●", "env + saved  "),
                    "env" => ("●", "env var      "),
                    "stored" => ("●", "saved        "),
                    _ => ("○", "not set      "),
                };

                let text = format!(" {icon} {:<15}  [{}]", entry.provider_id, status);
                ListItem::new(text).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(" API Keys   Enter/e Set   d/Del Remove   Esc/q Close "),
        );

        frame.render_widget(list, area);
    }
}

// ---------------------------------------------------------------------------
// Custom model input dialog
// ---------------------------------------------------------------------------

fn render_custom_model_input(frame: &mut Frame, provider_id: &str, input_buffer: &str, theme: &Theme) {
    let area = centered_rect(60, 30, frame.area());
    frame.render_widget(Clear, area);

    let display = if input_buffer.is_empty() {
        "(type model ID, e.g. gpt-4o-mini)".to_string()
    } else {
        input_buffer.to_string()
    };

    let text = format!(
        "Provider: {provider_id}\n\nModel ID: {display}\n\n[Enter] Use model    [Esc] Cancel"
    );

    let dialog = Paragraph::new(text)
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.spinner_fg))
                .title(" Add Custom Model "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(dialog, area);
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

/// Create a centered rectangle of the given percentage of the parent area.
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
