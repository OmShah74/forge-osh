use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

use super::themes::Theme;
use super::{
    AppState, DetailViewerState, GeneratedSkillPreviewState, HelpState, KeyManagerState,
    McpCustomForm, McpManagerState, McpView, MessageRole, Modal, SessionBrowserState,
    SkillBrowserState, MCP_CUSTOM_FIELD_COUNT, OSH_SPLASH_LINES,
};

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
            Constraint::Length(1),            // header
            Constraint::Min(5),               // conversation
            Constraint::Length(input_height), // input (dynamic)
            Constraint::Length(1),            // status bar
        ])
        .split(area);

    render_header(frame, chunks[0], state, &theme);
    render_conversation(frame, chunks[1], state, &theme);
    render_input(frame, chunks[2], state, &theme);
    render_status_bar(frame, chunks[3], state, &theme);

    // Render modal overlays on top
    if let Some(modal) = &state.modal {
        match modal {
            Modal::Confirmation {
                tool_name,
                description,
                scroll,
                ..
            } => {
                render_confirmation(frame, tool_name, description, *scroll, &theme);
            }
            Modal::Help(h) => {
                render_help(frame, &theme, h);
            }
            Modal::DetailViewer(dv) => {
                render_detail_viewer(frame, dv, &theme);
            }
            Modal::GeneratedSkillPreview(preview) => {
                render_generated_skill_preview(frame, preview, &theme);
            }
            Modal::PasteConfirm(paste) => {
                render_paste_confirm(frame, paste, &theme);
            }
            Modal::RenameSession { input_buffer } => {
                render_rename_session(frame, input_buffer, &theme);
            }
            Modal::SkillBrowser(browser) => {
                render_skill_browser(frame, browser, &theme);
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
            Modal::CustomModelInput {
                provider_id,
                input_buffer,
            } => {
                render_custom_model_input(frame, provider_id, input_buffer, &theme);
            }
            Modal::SessionBrowser(browser) => {
                render_session_browser(frame, browser, &theme);
            }
            Modal::McpManager(m) => {
                render_mcp_manager(frame, m, &theme);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MCP manager modal
// ---------------------------------------------------------------------------

fn render_mcp_manager(frame: &mut Frame, m: &McpManagerState, theme: &Theme) {
    let area = centered_rect(82, 78, frame.area());
    frame.render_widget(Clear, area);

    match m.view {
        McpView::List => render_mcp_list(frame, area, m, theme),
        McpView::Detail => render_mcp_detail(frame, area, m, theme),
        McpView::SecretInput => render_mcp_secret_input(frame, area, m, theme),
        McpView::CustomForm => render_mcp_custom_form(frame, area, &m.custom_form, theme),
    }
}

fn render_mcp_custom_form(frame: &mut Frame, area: Rect, f: &McpCustomForm, theme: &Theme) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning_fg))
        .title(" Add Custom MCP Server   Tab/↑↓ next field   Ctrl+S save   Esc cancel ");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),                                  // intro
            Constraint::Min(MCP_CUSTOM_FIELD_COUNT as u16 * 2 + 2), // fields
            Constraint::Length(3),                                  // footer
        ])
        .split(inner);

    let intro = Paragraph::new(
        "Define a server that's not in the built-in catalog. The command runs as a child process; \
         each named secret is sourced from the encrypted KeyStore (or env var) and passed as an env \
         var to the child. After saving, the server appears in the list and connects in the background.",
    )
    .style(Style::default().fg(theme.muted_fg))
    .wrap(Wrap { trim: false });
    frame.render_widget(intro, chunks[0]);

    // Field list — two lines per field: label, value box
    let field_area = chunks[1];
    let mut y = field_area.y;
    for i in 0..MCP_CUSTOM_FIELD_COUNT {
        let label = McpCustomForm::label_for(i);
        let focused = i == f.focused;
        let label_style = if focused {
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted_fg)
        };
        let label_widget = Paragraph::new(label).style(label_style);
        let label_rect = Rect {
            x: field_area.x + 1,
            y,
            width: field_area.width.saturating_sub(2),
            height: 1,
        };
        frame.render_widget(label_widget, label_rect);

        let value = match i {
            0 => f.id.clone(),
            1 => f.display_name.clone(),
            2 => f.description.clone(),
            3 => f.category.clone(),
            4 => f.command.clone(),
            5 => f.args.clone(),
            6 => f.secret_keys.clone(),
            7 => format!(
                "[{}] enabled (Space to toggle)",
                if f.enabled { "x" } else { " " }
            ),
            _ => String::new(),
        };
        let placeholder = match i {
            0 => "e.g. mycorp-internal",
            1 => "e.g. MyCorp Internal Tools",
            2 => "Short description shown in the list",
            3 => "e.g. Cloud, Custom",
            4 => "e.g. npx",
            5 => "e.g. -y @mycorp/mcp-server",
            6 => "e.g. MYCORP_TOKEN, MYCORP_REGION",
            _ => "",
        };
        let display = if i != 7 && value.is_empty() {
            format!("({placeholder})")
        } else {
            value
        };
        let display_style = if focused {
            Style::default()
                .fg(theme.fg)
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg)
        };
        let cursor = if focused && i != 7 { "▌" } else { " " };
        let value_widget = Paragraph::new(format!(" {} {}", display, cursor))
            .style(display_style)
            .block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(if focused {
                        Style::default().fg(theme.warning_fg)
                    } else {
                        Style::default().fg(theme.border_fg)
                    }),
            );
        let value_rect = Rect {
            x: field_area.x + 2,
            y: y + 1,
            width: field_area.width.saturating_sub(3),
            height: 1,
        };
        frame.render_widget(value_widget, value_rect);
        y = y.saturating_add(2);
        if y >= field_area.y + field_area.height {
            break;
        }
    }

    let footer_text = if let Some(err) = &f.error {
        format!("Error: {err}\n\n[Ctrl+S] Save and connect    [Esc] Cancel")
    } else {
        "Tip: command + args are split on spaces. Each secret is stored encrypted-at-rest under \
         mcp:<id>:<KEY> and exposed to the server process as env. \n[Ctrl+S] Save and connect    [Esc] Cancel"
            .to_string()
    };
    let footer = Paragraph::new(footer_text)
        .style(if f.error.is_some() {
            Style::default().fg(theme.error_fg)
        } else {
            Style::default().fg(theme.muted_fg)
        })
        .wrap(Wrap { trim: false });
    frame.render_widget(footer, chunks[2]);
}

fn render_mcp_list(frame: &mut Frame, area: Rect, m: &McpManagerState, theme: &Theme) {
    let title = " MCP Servers   ↑↓ nav   Space toggle   Enter detail   c connect   x disconnect   n new   D delete-custom   r refresh   q close ";
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_fg))
        .title(title);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if m.servers.is_empty() {
        let p = Paragraph::new("No MCP servers in catalog. Add one in config.toml under [mcp].")
            .style(Style::default().fg(theme.muted_fg));
        frame.render_widget(p, inner);
        return;
    }

    let items: Vec<ListItem> = m
        .servers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == m.selected {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            let icon = if !s.enabled {
                "○"
            } else if s.status.is_active() {
                "●"
            } else if matches!(s.status, crate::mcp::ServerStatus::Connecting) {
                "◐"
            } else if matches!(s.status, crate::mcp::ServerStatus::Error(_)) {
                "✗"
            } else {
                "·"
            };
            let secrets_summary = secret_summary(&s.required_secrets);
            let line = format!(
                " {icon} {:<26} [{:<15}] tools={:<3} secrets={:<8}  {}",
                truncate_to(&s.display_name, 26),
                truncate_to(&s.status.label(), 15),
                s.tool_count,
                secrets_summary,
                truncate_to(&s.description, 100),
            );
            ListItem::new(line).style(style)
        })
        .collect();

    let scroll = m.list_scroll as usize;
    let visible = inner.height.saturating_sub(0) as usize;
    let end = (scroll + visible).min(items.len());
    let slice: Vec<ListItem> = items[scroll.min(items.len())..end].to_vec();
    let list = List::new(slice);
    frame.render_widget(list, inner);
}

fn secret_summary(secs: &[crate::mcp::SecretStatus]) -> String {
    if secs.is_empty() {
        return "none".into();
    }
    let total = secs.len();
    let present = secs.iter().filter(|s| s.present).count();
    let req_missing = secs.iter().filter(|s| s.required && !s.present).count();
    if req_missing > 0 {
        format!("{}/{} need!", present, total)
    } else {
        format!("{}/{}", present, total)
    }
}

fn truncate_to(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

fn render_mcp_detail(frame: &mut Frame, area: Rect, m: &McpManagerState, theme: &Theme) {
    let s = match m.selected_server() {
        Some(s) => s,
        None => {
            let p =
                Paragraph::new("(no server selected)").style(Style::default().fg(theme.muted_fg));
            frame.render_widget(p, area);
            return;
        }
    };
    let title = format!(
        " {}   {} {}   ←/h back   ↑↓ secret   e/Enter set   d delete   Space toggle   c connect   x disconnect ",
        s.display_name,
        if s.enabled { "[enabled]" } else { "[disabled]" },
        s.status.label()
    );
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_fg))
        .title(title);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Layout: top description / status, middle secrets list, bottom stderr.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(4),
            Constraint::Length(7),
        ])
        .split(inner);

    let mut header_lines: Vec<Line> = Vec::new();
    header_lines.push(Line::from(vec![
        Span::styled("Server ID:  ", Style::default().fg(theme.muted_fg)),
        Span::styled(&s.id, Style::default().fg(theme.fg)),
    ]));
    header_lines.push(Line::from(vec![
        Span::styled("Category:   ", Style::default().fg(theme.muted_fg)),
        Span::styled(&s.category, Style::default().fg(theme.fg)),
    ]));
    header_lines.push(Line::from(vec![
        Span::styled("Tools:      ", Style::default().fg(theme.muted_fg)),
        Span::styled(s.tool_count.to_string(), Style::default().fg(theme.fg)),
        Span::raw("    "),
        Span::styled("Version: ", Style::default().fg(theme.muted_fg)),
        Span::styled(
            if s.server_version.is_empty() {
                "—".into()
            } else {
                s.server_version.clone()
            },
            Style::default().fg(theme.fg),
        ),
    ]));
    header_lines.push(Line::from(Span::styled(
        truncate_to(&s.description, 200),
        Style::default().fg(theme.fg),
    )));
    if let Some(err) = &s.last_error {
        header_lines.push(Line::from(vec![
            Span::styled("Error:      ", Style::default().fg(theme.error_fg)),
            Span::styled(truncate_to(err, 200), Style::default().fg(theme.error_fg)),
        ]));
    }
    let header = Paragraph::new(header_lines).wrap(Wrap { trim: false });
    frame.render_widget(header, chunks[0]);

    // Secrets list.
    if s.required_secrets.is_empty() {
        let p = Paragraph::new("(no secrets required for this server)")
            .style(Style::default().fg(theme.muted_fg));
        frame.render_widget(p, chunks[1]);
    } else {
        let items: Vec<ListItem> = s
            .required_secrets
            .iter()
            .enumerate()
            .map(|(i, sec)| {
                let style = if i == m.secret_selected {
                    Style::default()
                        .fg(theme.fg)
                        .bg(theme.highlight_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg)
                };
                let icon = if sec.present {
                    "●"
                } else if sec.required {
                    "✗"
                } else {
                    "○"
                };
                let src = match sec.source {
                    crate::mcp::SecretSource::Stored => "saved",
                    crate::mcp::SecretSource::Env => "env var",
                    crate::mcp::SecretSource::None => {
                        if sec.required {
                            "MISSING (required)"
                        } else {
                            "not set"
                        }
                    }
                };
                let line = format!(
                    " {icon} {:<32} [{:<20}]  {}",
                    truncate_to(&sec.label, 32),
                    src,
                    truncate_to(&sec.help, 80)
                );
                ListItem::new(line).style(style)
            })
            .collect();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::TOP)
                .title(" Secrets — Enter/e to set, d to clear stored value "),
        );
        frame.render_widget(list, chunks[1]);
    }

    // Stderr.
    let stderr_block = Block::default()
        .borders(Borders::TOP)
        .title(" Recent stderr (last 5 lines) ");
    let stderr_text = if s.recent_stderr.is_empty() {
        "(no stderr captured)".to_string()
    } else {
        s.recent_stderr.join("\n")
    };
    let p = Paragraph::new(stderr_text)
        .style(Style::default().fg(theme.muted_fg))
        .block(stderr_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(p, chunks[2]);
}

fn render_mcp_secret_input(frame: &mut Frame, area: Rect, m: &McpManagerState, theme: &Theme) {
    let s = match m.selected_server() {
        Some(s) => s,
        None => return,
    };
    let key = m.editing_secret_key.as_deref().unwrap_or("");
    let label = s
        .required_secrets
        .iter()
        .find(|x| x.key == key)
        .map(|x| x.label.clone())
        .unwrap_or_else(|| key.to_string());
    let help = s
        .required_secrets
        .iter()
        .find(|x| x.key == key)
        .map(|x| x.help.clone())
        .unwrap_or_default();

    let masked = if m.input_buffer.is_empty() {
        "(type the secret here — input is masked)".to_string()
    } else {
        let len = m.input_buffer.len();
        if len <= 8 {
            "*".repeat(len)
        } else {
            format!("{}…{}", "*".repeat(4), "*".repeat(4))
        }
    };
    let body = format!(
        "Server:  {} ({})\n\
         Secret:  {}\n\
         Help:    {}\n\
         \n\
         Value:   {}\n\
         \n\
         Stored encrypted-at-rest in ~/.forge-osh/keys.json (same as provider API keys).\n\
         \n\
         [Enter] Save    [Esc] Cancel",
        s.display_name, s.id, label, help, masked
    );
    let dialog = Paragraph::new(body)
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.warning_fg))
                .title(" Set MCP Secret "),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(dialog, area);
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

fn render_header(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let trust_indicator = if state.trust_mode { " [TRUST]" } else { "" };
    let busy_indicator = if state.agent_busy { " ●" } else { "" };

    // Context window progress bar: [████░░░░░░] 40%
    let ctx_bar = if state.context_pct > 0 {
        let filled = (state.context_pct as usize * 10 / 100).min(10);
        let empty = 10 - filled;
        let bar: String = "█".repeat(filled) + &"░".repeat(empty);
        let color_hint = if state.context_pct >= 90 {
            "!"
        } else if state.context_pct >= 70 {
            "~"
        } else {
            ""
        };
        format!("  [{}]{} {}%", bar, color_hint, state.context_pct)
    } else {
        String::new()
    };

    let header_text = format!(
        " forge-osh  {}  {}  {}  {}  {}{}{}{}",
        state.model_name,
        state.provider_name,
        state.session_name,
        state.format_tokens,
        state.format_cost,
        ctx_bar,
        trust_indicator,
        busy_indicator,
    );

    let header =
        Paragraph::new(header_text).style(Style::default().fg(theme.header_fg).bg(theme.header_bg));

    frame.render_widget(header, area);
}

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

fn render_conversation(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    let _wrap_width = area.width.saturating_sub(2) as usize; // reserved for future per-line wrap estimation

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
                                let color = if in_hash {
                                    theme.prompt_fg
                                } else {
                                    theme.border_fg
                                };
                                spans.push(Span::styled(
                                    segment.clone(),
                                    Style::default().fg(color).add_modifier(if in_hash {
                                        Modifier::BOLD
                                    } else {
                                        Modifier::empty()
                                    }),
                                ));
                                segment.clear();
                            }
                            in_hash = ch_is_hash;
                        }
                        segment.push(ch);
                    }
                    if !segment.is_empty() {
                        let color = if in_hash {
                            theme.prompt_fg
                        } else {
                            theme.border_fg
                        };
                        spans.push(Span::styled(
                            segment,
                            Style::default().fg(color).add_modifier(if in_hash {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                        ));
                    }
                    lines.push(Line::from(spans));
                }
                lines.push(Line::from(""));
            }

            MessageRole::User => {
                lines.push(Line::from(vec![Span::styled(
                    " You ",
                    Style::default()
                        .fg(theme.header_bg)
                        .bg(theme.user_msg_fg)
                        .add_modifier(Modifier::BOLD),
                )]));
                for text_line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {text_line}"),
                        Style::default().fg(theme.user_msg_fg),
                    )));
                }
                lines.push(Line::from(""));
            }

            MessageRole::Assistant => {
                lines.push(Line::from(vec![Span::styled(
                    " forge ",
                    Style::default()
                        .fg(theme.header_bg)
                        .bg(theme.assistant_msg_fg)
                        .add_modifier(Modifier::BOLD),
                )]));
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

            MessageRole::ToolResult {
                is_error,
                tool_name,
            } => {
                let (color, status_icon) = if *is_error {
                    (theme.error_fg, "✗")
                } else {
                    (theme.prompt_fg, "✓")
                };
                let tool_label = if tool_name.is_empty() {
                    format!("  {} Result", status_icon)
                } else {
                    format!("  {} {}", status_icon, tool_name)
                };
                lines.push(Line::from(vec![Span::styled(
                    tool_label,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )]));

                // Performance: only inspect the first max_lines+1 lines. For
                // huge tool outputs (e.g. 10k-line file reads) the previous
                // `.lines().collect::<Vec<_>>()` + full is_diff scan dominated
                // every frame, making scroll feel frozen. We cap both work
                // items at `max_lines` instead.
                let max_lines: usize = 50;
                let preview: Vec<&str> = msg.content.split('\n').take(max_lines + 1).collect();
                let is_diff = preview.iter().take(max_lines).any(|l| {
                    (l.starts_with('+') && !l.starts_with("+++"))
                        || (l.starts_with('-') && !l.starts_with("---"))
                });
                // Cheap O(1) count of total newlines for the "hidden lines" footer.
                let total_line_count = msg
                    .content
                    .as_bytes()
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count()
                    + 1;

                for text_line in preview.iter().take(max_lines) {
                    if is_diff {
                        if text_line.starts_with('+') && !text_line.starts_with("+++") {
                            // Addition — bright green text on dark green background
                            lines.push(Line::from(Span::styled(
                                format!("    {text_line}"),
                                Style::default().fg(theme.added_fg).bg(theme.added_bg),
                            )));
                        } else if text_line.starts_with('-') && !text_line.starts_with("---") {
                            // Removal — bright red text on dark red background
                            lines.push(Line::from(Span::styled(
                                format!("    {text_line}"),
                                Style::default().fg(theme.removed_fg).bg(theme.removed_bg),
                            )));
                        } else if text_line.starts_with("@@") {
                            // Hunk header — cyan/amber
                            lines.push(Line::from(Span::styled(
                                format!("    {text_line}"),
                                Style::default()
                                    .fg(theme.tool_name_fg)
                                    .add_modifier(Modifier::ITALIC),
                            )));
                        } else {
                            // Context lines
                            lines.push(Line::from(Span::styled(
                                format!("    {text_line}"),
                                Style::default().fg(theme.muted_fg),
                            )));
                        }
                    } else {
                        lines.push(Line::from(Span::styled(
                            format!("    {text_line}"),
                            Style::default().fg(theme.muted_fg),
                        )));
                    }
                }
                if total_line_count > max_lines {
                    lines.push(Line::from(Span::styled(
                        format!("    … ({} more lines hidden)", total_line_count - max_lines),
                        Style::default()
                            .fg(theme.muted_fg)
                            .add_modifier(Modifier::ITALIC),
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
        lines.push(Line::from(vec![Span::styled(
            " forge ",
            Style::default()
                .fg(theme.header_bg)
                .bg(theme.assistant_msg_fg)
                .add_modifier(Modifier::BOLD),
        )]));
        render_assistant_content(&mut lines, &state.streaming_text, theme);
    }

    // Spinner (thinking indicator)
    if state.spinner.active {
        lines.push(Line::from(Span::styled(
            format!("  {}", state.spinner.display()),
            Style::default().fg(theme.spinner_fg),
        )));
    }

    // Estimate total visual lines after word-wrapping. Without this, the raw
    // line count underestimates content height when long lines wrap, causing
    // max_scroll() to be too small and the viewport to freeze near the bottom.
    let wrap_width = area.width.saturating_sub(2) as usize; // -1 scrollbar, -1 border safety
    let total_visual = if wrap_width > 0 {
        lines
            .iter()
            .map(|line| {
                let w: usize = line
                    .spans
                    .iter()
                    .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                    .sum();
                if w == 0 {
                    1
                } else {
                    (w + wrap_width - 1) / wrap_width
                }
            })
            .sum()
    } else {
        lines.len()
    };
    let visible_height = area.height as usize;

    state.total_lines = total_visual;
    state.visible_height = visible_height;

    let scroll_start = state.effective_scroll();

    // We skip source lines until we've passed `scroll_start` visual rows.
    // This is more accurate than skipping by raw line index.
    let mut skipped_visual = 0usize;
    let mut skip_raw = 0usize;
    if wrap_width > 0 {
        for line in &lines {
            if skipped_visual >= scroll_start {
                break;
            }
            let w: usize = line
                .spans
                .iter()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            let rows = if w == 0 {
                1
            } else {
                (w + wrap_width - 1) / wrap_width
            };
            skipped_visual += rows;
            skip_raw += 1;
        }
    } else {
        skip_raw = scroll_start;
    }

    // Take enough source lines to fill the viewport plus a generous buffer
    // for wrapped lines that expand to multiple visual rows.
    let extra = visible_height / 2 + 12;
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(skip_raw)
        .take(visible_height + extra)
        .collect();

    // Give the paragraph one less column so the scrollbar gets its own column
    // and never overwrites text characters.
    let para_area = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };

    let conversation = Paragraph::new(Text::from(visible_lines))
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });

    frame.render_widget(conversation, para_area);

    // Scrollbar: shown whenever content is taller than the viewport.
    // Rendered in the rightmost column of the full area (not para_area).
    if total_visual > visible_height {
        let content_size = total_visual.saturating_sub(visible_height);
        let mut scrollbar_state = ScrollbarState::new(content_size).position(scroll_start);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Render assistant message content with basic markdown support.
fn render_assistant_content(lines: &mut Vec<Line>, content: &str, theme: &Theme) {
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut in_math_block = false;
    let mut math_buffer = String::new();

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

        // Display math blocks. The terminal cannot typeset full TeX, so we
        // normalize common LaTeX commands into readable Unicode instead.
        if in_math_block {
            if let Some(before_end) = trimmed.strip_suffix("$$") {
                if !math_buffer.is_empty() && !before_end.trim().is_empty() {
                    math_buffer.push(' ');
                }
                math_buffer.push_str(before_end.trim());
                render_math_display(lines, &math_buffer, theme);
                math_buffer.clear();
                in_math_block = false;
            } else if trimmed == r"\]" {
                render_math_display(lines, &math_buffer, theme);
                math_buffer.clear();
                in_math_block = false;
            } else {
                if !math_buffer.is_empty() {
                    math_buffer.push(' ');
                }
                math_buffer.push_str(trimmed);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("$$") {
            if let Some(expr) = rest.strip_suffix("$$") {
                render_math_display(lines, expr.trim(), theme);
            } else {
                math_buffer = rest.trim().to_string();
                in_math_block = true;
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix(r"\[") {
            if let Some(expr) = rest.strip_suffix(r"\]") {
                render_math_display(lines, expr.trim(), theme);
            } else {
                math_buffer = rest.trim().to_string();
                in_math_block = true;
            }
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
                    Style::default()
                        .fg(theme.tool_name_fg)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    heading.to_string(),
                    Style::default()
                        .fg(theme.warning_fg)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
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
    if in_math_block && !math_buffer.trim().is_empty() {
        render_math_display(lines, &math_buffer, theme);
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
        // Inline math: \( ... \)
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '(' {
            if let Some(end) = find_latex_inline_end(&chars, i + 2) {
                push_plain_span(&mut spans, &mut current, theme);
                let expr: String = chars[i + 2..end].iter().collect();
                spans.push(math_span(&expr, theme));
                i = end + 2;
                continue;
            }
        }

        // Inline math: $ ... $. Avoid treating $$ display delimiters as inline.
        if chars[i] == '$' && (i + 1 >= chars.len() || chars[i + 1] != '$') {
            if let Some(end) = find_dollar_inline_end(&chars, i + 1) {
                let expr: String = chars[i + 1..end].iter().collect();
                if !expr.trim().is_empty() {
                    push_plain_span(&mut spans, &mut current, theme);
                    spans.push(math_span(&expr, theme));
                    i = end + 1;
                    continue;
                }
            }
        }

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
            while i + 1 < chars.len() && !(chars[i] == marker && chars[i + 1] == marker) {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(
                current.clone(),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
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

fn push_plain_span(spans: &mut Vec<Span<'static>>, current: &mut String, theme: &Theme) {
    if !current.is_empty() {
        spans.push(Span::styled(
            std::mem::take(current),
            Style::default().fg(theme.assistant_msg_fg),
        ));
    }
}

fn math_span(expr: &str, theme: &Theme) -> Span<'static> {
    Span::styled(
        normalize_math_text(expr),
        Style::default()
            .fg(theme.tool_name_fg)
            .add_modifier(Modifier::ITALIC),
    )
}

fn render_math_display(lines: &mut Vec<Line>, expr: &str, theme: &Theme) {
    for rendered_line in format_display_math_lines(expr) {
        if rendered_line.trim().is_empty() {
            continue;
        }
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                rendered_line,
                Style::default()
                    .fg(theme.tool_name_fg)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
    }
}

fn find_latex_inline_end(chars: &[char], mut i: usize) -> Option<usize> {
    while i + 1 < chars.len() {
        if chars[i] == '\\' && chars[i + 1] == ')' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_dollar_inline_end(chars: &[char], mut i: usize) -> Option<usize> {
    while i < chars.len() {
        if chars[i] == '$' {
            if i + 1 < chars.len() && chars[i + 1] == '$' {
                return None;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

fn normalize_math_text(expr: &str) -> String {
    let mut out = expr.trim().to_string();
    for _ in 0..4 {
        let next = replace_first_frac(&out);
        if next == out {
            break;
        }
        out = next;
    }
    for _ in 0..4 {
        let next = replace_first_sqrt(&out);
        if next == out {
            break;
        }
        out = next;
    }
    for command in [
        r"\mathrm",
        r"\mathbf",
        r"\mathit",
        r"\mathbb",
        r"\mathcal",
        r"\mathsf",
        r"\text",
    ] {
        for _ in 0..8 {
            let next = unwrap_first_command_group(&out, command);
            if next == out {
                break;
            }
            out = next;
        }
    }
    for (from, to) in LATEX_REPLACEMENTS {
        out = out.replace(from, to);
    }
    out = normalize_math_scripts(&out);
    out = out.replace('{', "(").replace('}', ")");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_display_math_lines(expr: &str) -> Vec<String> {
    let cleaned = strip_display_math_environment(expr);
    let mut rendered = Vec::new();

    for segment in cleaned.split(r"\\") {
        let segment = segment.trim().trim_matches('&').trim();
        if segment.is_empty() {
            continue;
        }
        rendered.extend(format_display_math_segment(segment));
    }

    if rendered.is_empty() {
        let normalized = normalize_math_text(expr);
        if !normalized.trim().is_empty() {
            rendered.push(normalized);
        }
    }

    rendered
}

fn strip_display_math_environment(expr: &str) -> String {
    let mut out = expr.replace(r"\begin{aligned}", "");
    out = out.replace(r"\end{aligned}", "");
    out = out.replace(r"\begin{align}", "");
    out = out.replace(r"\end{align}", "");
    out = out.replace(r"\begin{equation}", "");
    out = out.replace(r"\end{equation}", "");
    out.replace('&', "")
}

fn format_display_math_segment(segment: &str) -> Vec<String> {
    if let Some(parts) = split_first_fraction(segment) {
        return format_stacked_fraction(parts);
    }

    let normalized = normalize_math_text(segment);
    if normalized.trim().is_empty() {
        Vec::new()
    } else {
        vec![normalized]
    }
}

struct FractionParts {
    prefix: String,
    numerator: String,
    denominator: String,
    suffix: String,
}

fn split_first_fraction(input: &str) -> Option<FractionParts> {
    let (command, start) = first_fraction_command(input)?;
    let first_open = skip_ascii_spaces(input, start + command.len());
    let (numerator, numerator_end) = extract_braced(input, first_open)?;
    let second_open = skip_ascii_spaces(input, numerator_end);
    let (denominator, denominator_end) = extract_braced(input, second_open)?;

    Some(FractionParts {
        prefix: input[..start].to_string(),
        numerator,
        denominator,
        suffix: input[denominator_end..].to_string(),
    })
}

fn first_fraction_command(input: &str) -> Option<(&'static str, usize)> {
    [r"\frac", r"\dfrac", r"\tfrac"]
        .into_iter()
        .filter_map(|command| input.find(command).map(|start| (command, start)))
        .min_by_key(|(_, start)| *start)
}

fn format_stacked_fraction(parts: FractionParts) -> Vec<String> {
    let prefix = normalize_math_text(&parts.prefix);
    let suffix = normalize_math_text(&parts.suffix);
    let numerator = normalize_math_text(&parts.numerator);
    let denominator = normalize_math_text(&parts.denominator);

    let numerator_width = unicode_width::UnicodeWidthStr::width(numerator.as_str());
    let denominator_width = unicode_width::UnicodeWidthStr::width(denominator.as_str());
    let fraction_width = numerator_width.max(denominator_width).max(1);
    let prefix_with_space = if prefix.trim().is_empty() {
        String::new()
    } else {
        format!("{} ", prefix.trim())
    };
    let suffix_with_space = if suffix.trim().is_empty() {
        String::new()
    } else {
        format!(" {}", suffix.trim())
    };
    let prefix_indent = " ".repeat(unicode_width::UnicodeWidthStr::width(
        prefix_with_space.as_str(),
    ));

    vec![
        format!(
            "{prefix_indent}{}",
            center_to_width(&numerator, fraction_width)
        ),
        format!(
            "{prefix_with_space}{}{suffix_with_space}",
            "\u{2500}".repeat(fraction_width)
        ),
        format!(
            "{prefix_indent}{}",
            center_to_width(&denominator, fraction_width)
        ),
    ]
}

fn center_to_width(text: &str, width: usize) -> String {
    let text_width = unicode_width::UnicodeWidthStr::width(text);
    if text_width >= width {
        return text.to_string();
    }
    let padding = width - text_width;
    let left = padding / 2;
    let right = padding - left;
    format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
}

const LATEX_REPLACEMENTS: &[(&str, &str)] = &[
    (r"\varepsilon", "ε"),
    (r"\epsilon", "ε"),
    (r"\vartheta", "ϑ"),
    (r"\theta", "θ"),
    (r"\lambda", "λ"),
    (r"\alpha", "α"),
    (r"\beta", "β"),
    (r"\gamma", "γ"),
    (r"\delta", "δ"),
    (r"\kappa", "κ"),
    (r"\sigma", "σ"),
    (r"\omega", "ω"),
    (r"\Omega", "Ω"),
    (r"\Delta", "Δ"),
    (r"\Gamma", "Γ"),
    (r"\Lambda", "Λ"),
    (r"\Sigma", "Σ"),
    (r"\Theta", "Θ"),
    (r"\prod", "∏"),
    (r"\sum", "∑"),
    (r"\int", "∫"),
    (r"\partial", "∂"),
    (r"\nabla", "∇"),
    (r"\infty", "∞"),
    (r"\subseteq", "⊆"),
    (r"\subset", "⊂"),
    (r"\notin", "∉"),
    (r"\in", "∈"),
    (r"\forall", "∀"),
    (r"\exists", "∃"),
    (r"\Rightarrow", "⇒"),
    (r"\rightarrow", "→"),
    (r"\leftarrow", "←"),
    (r"\implies", "⇒"),
    (r"\iff", "⇔"),
    (r"\leq", "≤"),
    (r"\geq", "≥"),
    (r"\neq", "≠"),
    (r"\approx", "≈"),
    (r"\times", "×"),
    (r"\cdot", "·"),
    (r"\pm", "±"),
    (r"\cup", "∪"),
    (r"\cap", "∩"),
    (r"\lVert", "‖"),
    (r"\rVert", "‖"),
    (r"\Vert", "‖"),
    (r"\|", "‖"),
    (r"\quad", " "),
    (r"\qquad", "  "),
    (r"\ldots", "…"),
    (r"\dots", "…"),
    (r"\sin", "sin"),
    (r"\cos", "cos"),
    (r"\tan", "tan"),
    (r"\log", "log"),
    (r"\ln", "ln"),
    (r"\max", "max"),
    (r"\min", "min"),
    (r"\argmax", "argmax"),
    (r"\argmin", "argmin"),
    (r"\Pr", "P"),
    (r"\to", "→"),
    (r"\left", ""),
    (r"\right", ""),
    (r"\,", " "),
    (r"\;", " "),
    (r"\:", " "),
    (r"\!", ""),
];

fn replace_first_frac(input: &str) -> String {
    let Some((command, _)) = first_fraction_command(input) else {
        return input.to_string();
    };
    replace_first_binary_command(input, command, |a, b| format!("({a})/({b})"))
}

fn replace_first_sqrt(input: &str) -> String {
    replace_first_unary_command(input, r"\sqrt", |a| format!("√({a})"))
}

fn unwrap_first_command_group(input: &str, command: &str) -> String {
    replace_first_unary_command(input, command, |a| a.to_string())
}

fn replace_first_binary_command<F>(input: &str, command: &str, build: F) -> String
where
    F: Fn(&str, &str) -> String,
{
    let Some(start) = input.find(command) else {
        return input.to_string();
    };
    let first_open = skip_ascii_spaces(input, start + command.len());
    let Some((left, left_end)) = extract_braced(input, first_open) else {
        return input.to_string();
    };
    let second_open = skip_ascii_spaces(input, left_end);
    let Some((right, right_end)) = extract_braced(input, second_open) else {
        return input.to_string();
    };

    let mut out = String::new();
    out.push_str(&input[..start]);
    out.push_str(&build(&left, &right));
    out.push_str(&input[right_end..]);
    out
}

fn replace_first_unary_command<F>(input: &str, command: &str, build: F) -> String
where
    F: Fn(&str) -> String,
{
    let Some(start) = input.find(command) else {
        return input.to_string();
    };
    let open = skip_ascii_spaces(input, start + command.len());
    let Some((inner, end)) = extract_braced(input, open) else {
        return input.to_string();
    };

    let mut out = String::new();
    out.push_str(&input[..start]);
    out.push_str(&build(&inner));
    out.push_str(&input[end..]);
    out
}

fn skip_ascii_spaces(input: &str, mut idx: usize) -> usize {
    while idx < input.len() && input.as_bytes()[idx].is_ascii_whitespace() {
        idx += 1;
    }
    idx
}

fn extract_braced(input: &str, open: usize) -> Option<(String, usize)> {
    if input.as_bytes().get(open).copied() != Some(b'{') {
        return None;
    }

    let mut depth = 0usize;
    let mut body_start = None;
    for (offset, ch) in input[open..].char_indices() {
        let idx = open + offset;
        match ch {
            '{' => {
                depth += 1;
                if depth == 1 {
                    body_start = Some(idx + ch.len_utf8());
                }
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let start = body_start?;
                    return Some((input[start..idx].to_string(), idx + ch.len_utf8()));
                }
            }
            _ => {}
        }
    }
    None
}

fn normalize_math_scripts(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '^' || chars[i] == '_' {
            let superscript = chars[i] == '^';
            if let Some((script, next)) = take_math_script(&chars, i + 1) {
                let mapped = if superscript {
                    map_script_chars(&script, superscript_char)
                } else {
                    map_script_chars(&script, subscript_char)
                };
                if let Some(mapped) = mapped {
                    out.push_str(&mapped);
                } else if superscript {
                    out.push_str("^(");
                    out.push_str(&script);
                    out.push(')');
                } else {
                    out.push_str("_(");
                    out.push_str(&script);
                    out.push(')');
                }
                i = next;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn take_math_script(chars: &[char], start: usize) -> Option<(String, usize)> {
    if start >= chars.len() {
        return None;
    }
    if chars[start] == '{' {
        let mut script = String::new();
        let mut i = start + 1;
        while i < chars.len() && chars[i] != '}' {
            script.push(chars[i]);
            i += 1;
        }
        if i < chars.len() {
            return Some((script, i + 1));
        }
        return None;
    }
    Some((chars[start].to_string(), start + 1))
}

fn map_script_chars<F>(script: &str, mapper: F) -> Option<String>
where
    F: Fn(char) -> Option<char>,
{
    let mut out = String::new();
    for ch in script.chars() {
        out.push(mapper(ch)?);
    }
    Some(out)
}

fn superscript_char(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'd' => '\u{1d48}',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        'k' => '\u{1d4f}',
        _ => return None,
    })
}

fn subscript_char(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        _ => return None,
    })
}

/// Pretty-print tool call input JSON with key-value formatting.
fn render_tool_input(lines: &mut Vec<Line>, json_str: &str, theme: &Theme) {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(obj) = val.as_object() {
            for (key, value) in obj {
                match value {
                    serde_json::Value::String(s) if s.contains('\n') || s.len() > 60 => {
                        lines.push(Line::from(vec![Span::styled(
                            format!("    {key}: "),
                            Style::default().fg(theme.tool_name_fg),
                        )]));
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
                                format!(
                                    "      ... ({} more lines)",
                                    content_lines.len() - max_preview
                                ),
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
                            Span::styled(value.to_string(), Style::default().fg(theme.fg)),
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
    let inner_height = area.height.saturating_sub(1) as usize;

    let (display_text, text_style, cursor) = if state.input.text.is_empty() {
        if state.agent_busy {
            (
                "Agent is working... (Ctrl+C to cancel)".to_string(),
                Style::default().fg(theme.muted_fg),
                None,
            )
        } else {
            (
                "Type a message or /help for commands...".to_string(),
                Style::default().fg(theme.muted_fg),
                None,
            )
        }
    } else {
        let viewport = input_viewport(
            &state.input.text,
            state.input.cursor,
            state.input.scroll_top,
            inner_height.max(1),
            inner_width.max(1),
        );
        (
            viewport.text,
            Style::default().fg(theme.fg),
            Some((viewport.cursor_x, viewport.cursor_y)),
        )
    };

    let title = if state.agent_busy {
        Span::styled(
            " ⏳ ",
            Style::default()
                .fg(theme.spinner_fg)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ❯ ",
            Style::default()
                .fg(theme.prompt_fg)
                .add_modifier(Modifier::BOLD),
        )
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

    if let Some((x, y)) = cursor {
        let cursor_x = (area.x + 1 + x as u16).min(area.x + area.width.saturating_sub(2));
        let cursor_y = (area.y + 1 + y as u16).min(area.y + area.height.saturating_sub(1));
        frame.set_cursor_position((cursor_x, cursor_y));
    } else {
        frame.set_cursor_position((area.x + 1, area.y + 1));
    }
}

struct InputViewport {
    text: String,
    cursor_x: usize,
    cursor_y: usize,
}

fn input_viewport(
    text: &str,
    cursor: usize,
    scroll_top: usize,
    visible_rows: usize,
    visible_cols: usize,
) -> InputViewport {
    let (cursor_line, cursor_col) = cursor_line_col(text, cursor);
    let total_lines = text.split('\n').count().max(1);
    let max_scroll = total_lines.saturating_sub(visible_rows);
    let mut start_line = scroll_top.min(max_scroll);
    if cursor_line < start_line {
        start_line = cursor_line;
    } else if cursor_line >= start_line.saturating_add(visible_rows) {
        start_line = cursor_line.saturating_add(1).saturating_sub(visible_rows);
    }

    let horizontal_offset = if visible_cols > 0 && cursor_col >= visible_cols {
        cursor_col.saturating_sub(visible_cols).saturating_add(1)
    } else {
        0
    };

    let mut rendered = Vec::new();
    for (line_idx, line) in text
        .split('\n')
        .enumerate()
        .skip(start_line)
        .take(visible_rows)
    {
        if line_idx == cursor_line {
            rendered.push(slice_by_display_width(
                line,
                horizontal_offset,
                visible_cols,
            ));
        } else {
            rendered.push(slice_by_display_width(line, 0, visible_cols));
        }
    }

    InputViewport {
        text: rendered.join("\n"),
        cursor_x: cursor_col
            .saturating_sub(horizontal_offset)
            .min(visible_cols),
        cursor_y: cursor_line
            .saturating_sub(start_line)
            .min(visible_rows.saturating_sub(1)),
    }
}

fn cursor_line_col(text: &str, cursor: usize) -> (usize, usize) {
    let mut safe_cursor = cursor.min(text.len());
    while safe_cursor > 0 && !text.is_char_boundary(safe_cursor) {
        safe_cursor -= 1;
    }
    let before = &text[..safe_cursor];
    let line = before.chars().filter(|&c| c == '\n').count();
    let last_line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = unicode_width::UnicodeWidthStr::width(&before[last_line_start..]);
    (line, col)
}

fn slice_by_display_width(line: &str, start_col: usize, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut col = 0usize;
    for ch in line.chars() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if col.saturating_add(width) <= start_col {
            col = col.saturating_add(width);
            continue;
        }
        if unicode_width::UnicodeWidthStr::width(out.as_str()).saturating_add(width) > max_cols {
            break;
        }
        out.push(ch);
        col = col.saturating_add(width);
    }
    out
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
        if total >= 6 {
            return total;
        }
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
        if state.vim_normal_mode {
            " [VIM]".to_string()
        } else {
            String::new()
        }
    };

    // Unread indicator: shown when scrolled away from bottom and new messages arrived
    let unread_indicator = if state.unread_count > 0 && !state.auto_scroll {
        format!("  ↓{} new", state.unread_count)
    } else {
        String::new()
    };

    let skill_indicator = state
        .active_skill_label
        .as_deref()
        .map(|n| format!("  🪄 {n}"))
        .unwrap_or_default();

    let goal_indicator = {
        let s = state.goal_status_blurb.lock();
        if s.is_empty() {
            String::new()
        } else {
            format!("  {}", *s)
        }
    };

    let status = format!(
        " ^C Cancel  ^O Model  ^P Provider  ^K Keys  ^B Cost  ^R Theme  ^T Trust[{trust}]  /help Cmds{skill_indicator}{goal_indicator}{scroll_info}{unread_indicator}"
    );

    let bar =
        Paragraph::new(status).style(Style::default().fg(theme.status_fg).bg(theme.status_bg));

    frame.render_widget(bar, area);
}

// ---------------------------------------------------------------------------
// Modals
// ---------------------------------------------------------------------------

fn render_confirmation(
    frame: &mut Frame,
    tool_name: &str,
    description: &str,
    scroll: u16,
    theme: &Theme,
) {
    let area = centered_rect(78, 72, frame.area());
    frame.render_widget(Clear, area);

    let text = format!(
        "Permission Required\n\nTool: {tool_name}\n\n{description}\n\n[Y/Enter] Allow  [N/Esc] Deny  [A] Always Allow  [T] Trust Mode\n[↑/↓ PgUp/PgDn] Scroll"
    );

    let dialog = Paragraph::new(text)
        .style(Style::default().fg(theme.warning_fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.warning_fg))
                .title(" Confirm Action "),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(dialog, area);
}

fn render_paste_confirm(frame: &mut Frame, paste: &super::PasteConfirmState, theme: &Theme) {
    let area = centered_rect(72, 42, frame.area());
    frame.render_widget(Clear, area);

    let severity = match paste.analysis.recommendation {
        super::PasteRecommendation::RejectTooLarge => {
            "This paste is larger than the estimated usable context budget."
        }
        super::PasteRecommendation::AskForStrategy => {
            "This paste may overflow the active model context."
        }
        super::PasteRecommendation::InsertInlineWithWarning => {
            "This paste is close to the active model context limit."
        }
        super::PasteRecommendation::InsertInline => "This paste fits the current estimate.",
    };
    let body = format!(
        "{severity}\n\n\
         Size: {} chars, {} bytes, {} line(s), ~{} tokens\n\
         Estimated available for new input: ~{} / {} context tokens\n\n\
         Press Y, I, or Enter to insert the full paste into the prompt.\n\
         Press Esc or C to cancel.\n\n\
         The paste is not sent to the model until you submit the prompt.",
        paste.analysis.chars,
        paste.analysis.bytes,
        paste.analysis.lines,
        paste.analysis.estimated_tokens,
        paste.analysis.available_tokens_estimate,
        paste.analysis.context_limit,
    );

    let dialog = Paragraph::new(body)
        .style(Style::default().fg(theme.warning_fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.warning_fg))
                .title(" Large Clipboard Paste "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(dialog, area);
}

fn render_help(frame: &mut Frame, theme: &Theme, h: &HelpState) {
    let area = centered_rect(82, 85, frame.area());
    frame.render_widget(Clear, area);

    // Clamp scroll to the visible body height so scrolling past the end just
    // pins the bottom of the text in view.
    let body_text = super::help::help_text();
    let total_lines = body_text.lines().count() as u16;
    let inner_h = area.height.saturating_sub(2); // borders
    let max_scroll = total_lines.saturating_sub(inner_h);
    let scroll = h.scroll.min(max_scroll);

    let help = Paragraph::new(body_text)
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(" Help  ↑↓/jk scroll · PgUp/PgDn page · g/G top/bottom · Esc close "),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(help, area);

    // Scrollbar column on the right edge.
    if max_scroll > 0 {
        let mut sb_state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));
        frame.render_stateful_widget(sb, area, &mut sb_state);
    }
}

fn render_detail_viewer(frame: &mut Frame, dv: &DetailViewerState, theme: &Theme) {
    let area = centered_rect(82, 80, frame.area());
    frame.render_widget(Clear, area);

    let total_lines = dv.body.lines().count() as u16;
    let inner_h = area.height.saturating_sub(2);
    let max_scroll = total_lines.saturating_sub(inner_h);
    let scroll = dv.scroll.min(max_scroll);

    let title = format!("{} (↑↓/jk scroll · Esc close)", dv.title);
    let p = Paragraph::new(dv.body.as_str())
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_fg))
                .title(title),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(p, area);

    if max_scroll > 0 {
        let mut sb_state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));
        frame.render_stateful_widget(sb, area, &mut sb_state);
    }
}

fn render_generated_skill_preview(
    frame: &mut Frame,
    preview: &GeneratedSkillPreviewState,
    theme: &Theme,
) {
    let area = centered_rect(86, 84, frame.area());
    frame.render_widget(Clear, area);

    let body = preview.body();
    let total_lines = body.lines().count() as u16;
    let inner_h = area.height.saturating_sub(2);
    let max_scroll = total_lines.saturating_sub(inner_h);
    let scroll = preview.scroll.min(max_scroll);
    let mode = if preview.showing_raw {
        "raw SKILL.md"
    } else {
        "review"
    };
    let title = format!(
        " Generated Skill: {} ({mode})  Y create · E toggle raw · Esc cancel ",
        preview.draft.name
    );

    let p = Paragraph::new(body)
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.spinner_fg))
                .title(title),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(p, area);

    if max_scroll > 0 {
        let mut sb_state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("â–²"))
            .end_symbol(Some("â–¼"));
        frame.render_stateful_widget(sb, area, &mut sb_state);
    }
}

fn render_rename_session(frame: &mut Frame, input: &str, theme: &Theme) {
    use ratatui::layout::Rect;
    let area_outer = centered_rect(60, 20, frame.area());
    // Small box — clamp height.
    let area = Rect {
        height: area_outer.height.min(7),
        ..area_outer
    };
    frame.render_widget(Clear, area);

    let body = format!(
        "Enter a new name for this session, then press Enter.\n\n  › {}_",
        input
    );
    let p = Paragraph::new(body)
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.spinner_fg))
                .title(" Rename session  (Esc to cancel) "),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(p, area);
}

fn render_skill_browser(frame: &mut Frame, browser: &SkillBrowserState, theme: &Theme) {
    use ratatui::layout::Rect;
    let area = centered_rect(82, 80, frame.area());
    frame.render_widget(Clear, area);

    let title = if browser.confirm_delete.is_some() {
        " Skills  Press D again to confirm delete, any other key to cancel ".to_string()
    } else if browser.name_input.is_some() {
        " Skills  Enter name, Enter to create, Esc to cancel ".to_string()
    } else {
        " Skills  ↑↓/jk nav · Enter invoke · s show · e edit · n new · d delete · r reload · o off · Esc close ".to_string()
    };

    if browser.entries.is_empty() {
        let text = "No skills found.\n\n\
                    Press `n` to create a new project skill, or add one manually at:\n  \
                    • ./.claude/skills/<name>/SKILL.md (project)\n  \
                    • ~/.forge-osh/skills/<name>/SKILL.md (user)\n\n\
                    Press Esc to close.";
        let p = Paragraph::new(text)
            .style(Style::default().fg(theme.muted_fg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border_fg))
                    .title(title),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = browser
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let is_deleting = browser.confirm_delete.as_deref() == Some(e.name.as_str());
            let style = if i == browser.selected {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            let active_marker = if browser.active_skill.as_deref() == Some(e.name.as_str()) {
                " ●"
            } else {
                "  "
            };
            let delete_mark = if is_deleting { "  ← DELETE?" } else { "" };
            let text = format!(
                " {active_marker} [{:<7}] {:<22}  {}{}",
                e.source,
                truncate(&e.name, 22),
                truncate(&e.description, 60),
                delete_mark,
            );
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_fg))
            .title(title),
    );

    let mut list_state = ListState::default().with_selected(Some(browser.selected));
    frame.render_stateful_widget(list, area, &mut list_state);

    // Footer: help line + in-progress name input (if any)
    let footer_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(2),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let footer_text = if let Some((_intent, buf)) = &browser.name_input {
        format!("  new skill name: {}_", buf)
    } else if let Some(e) = browser.selected_entry() {
        let when = e.when_to_use.as_deref().unwrap_or("");
        if when.is_empty() {
            format!(
                "  mode: {}   tools: {}",
                e.execution_mode,
                if e.allowed_tools.is_empty() {
                    "(unrestricted)".to_string()
                } else {
                    e.allowed_tools.join(", ")
                }
            )
        } else {
            format!(
                "  ↳ {}",
                truncate(when, (area.width.saturating_sub(4)) as usize)
            )
        }
    } else {
        String::new()
    };
    let footer = Paragraph::new(footer_text).style(Style::default().fg(theme.muted_fg));
    frame.render_widget(footer, footer_area);
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
    let area = centered_rect(64, 45, frame.area());
    frame.render_widget(Clear, area);

    let ctx_bar = {
        let filled = (state.context_pct as usize * 20 / 100).min(20);
        let empty = 20 - filled;
        "█".repeat(filled) + &"░".repeat(empty)
    };
    let text = format!(
        "Usage & Cost\n\n\
         Provider : {}\n\
         Model    : {}\n\
         Context  : [{}] {}% of {} tokens\n\
         Tokens   : {}\n\
         Cost     : {}\n\n\
         Context % reflects the last prompt's size in the model's window.\n\
         Cumulative tokens keep growing as each turn adds to the bill.\n\
         Use /compact [keep] to free context with an AI summary.\n\n\
         Press Esc to close",
        state.provider_name,
        state.model_name,
        ctx_bar,
        state.context_pct,
        state.context_limit,
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
                format!(
                    "{}...{}",
                    &km.input_buffer[..4],
                    &km.input_buffer[len - 4..]
                )
            }
        };

        let text =
            format!("Set API key for: {provider}\n\nKey: {masked}\n\n[Enter] Save    [Esc] Cancel");

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

fn render_custom_model_input(
    frame: &mut Frame,
    provider_id: &str,
    input_buffer: &str,
    theme: &Theme,
) {
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
// Session browser modal
// ---------------------------------------------------------------------------

fn render_session_browser(frame: &mut Frame, browser: &SessionBrowserState, theme: &Theme) {
    use ratatui::layout::Rect;

    let area = centered_rect(80, 70, frame.area());
    frame.render_widget(Clear, area);

    // Title / hint changes when confirming a delete
    let title = if browser.confirm_delete.is_some() {
        " Sessions  Press D again to confirm delete, any other key to cancel "
    } else {
        " Sessions  ↑↓ navigate   Enter load   D delete   Esc close "
    };

    if browser.sessions.is_empty() {
        let text = "No saved sessions found.\n\nUse /save to save the current session.";
        let p = Paragraph::new(text)
            .style(Style::default().fg(theme.muted_fg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border_fg))
                    .title(title),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(p, area);
        return;
    }

    // Build list items
    let items: Vec<ListItem> = browser
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_deleting = browser.confirm_delete.as_deref() == Some(s.id.as_str());
            let style = if i == browser.selected {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            // Truncate updated_at to date portion only
            let date = s.updated_at.get(..10).unwrap_or(&s.updated_at);

            let delete_mark = if is_deleting { " ← DELETE?" } else { "" };
            let text = format!(
                "  {:<28} {:<18} {}  {} msgs{}",
                truncate(&s.name, 28),
                truncate(&format!("{} / {}", s.model, s.provider), 18),
                date,
                s.message_count,
                delete_mark,
            );

            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_fg))
            .title(title),
    );

    let mut list_state = ListState::default().with_selected(Some(browser.selected));
    frame.render_stateful_widget(list, area, &mut list_state);

    // Footer hint inside the box (draw a small line at the bottom of the area)
    let footer_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(2),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let footer = Paragraph::new(
        "  ID: press Enter to load conversation  |  D: mark for delete  |  Esc: close",
    )
    .style(Style::default().fg(theme.muted_fg));
    frame.render_widget(footer, footer_area);
}

/// Truncate a string to at most `max` chars, appending '…' if cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        if max == 0 {
            return String::new();
        }
        let prefix: String = s.chars().take(max.saturating_sub(1)).collect();
        return format!("{prefix}â€¦");
    }
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_johnson_lindenstrauss_math() {
        let input =
            r"(1 - \epsilon) \|u - v\|^2 \leq \|f(u) - f(v)\|^2 \leq (1 + \epsilon) \|u - v\|^2";
        let normalized = normalize_math_text(input);

        assert!(normalized.contains("ε"));
        assert!(normalized.contains("≤"));
        assert!(normalized.contains("‖u - v‖²"));
        assert!(!normalized.contains(r"\epsilon"));
        assert!(!normalized.contains(r"\leq"));
    }

    #[test]
    fn normalizes_mathbb_arrows_fractions_and_roots() {
        let input = r"f: \mathbb{R}^d \to \mathbb{R}^k,\quad \frac{a+b}{\sqrt{c}}";
        let normalized = normalize_math_text(input);

        assert!(normalized.contains("Rᵈ"));
        assert!(normalized.contains("Rᵏ"));
        assert!(normalized.contains("→"));
        assert!(normalized.contains("(a+b)/(√(c))"));
    }

    #[test]
    fn display_math_stacks_simple_fraction() {
        let rendered = format_display_math_lines(r"E = mc^2 + \frac{a+b}{c+d}");

        assert_eq!(rendered.len(), 3);
        assert!(rendered[0].contains("a+b"));
        assert!(rendered[1].contains("E = mc² +"));
        assert!(rendered[1].contains("─"));
        assert!(rendered[2].contains("c+d"));
    }

    #[test]
    fn renders_display_math_without_literal_delimiters() {
        let theme = Theme::dark();
        let mut lines = Vec::new();
        render_assistant_content(
            &mut lines,
            "Before\n\\[\n\\alpha_i \\leq \\beta^2\n\\]\nAfter",
            &theme,
        );

        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("αᵢ ≤ β²"));
        assert!(!rendered.contains(r"\["));
        assert!(!rendered.contains(r"\]"));
    }
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
