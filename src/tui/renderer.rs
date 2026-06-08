use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

use super::themes::Theme;
use super::{
    AppState, DetailViewerState, GeneratedSkillPreviewState, HelpState, KeyManagerState,
    McpCustomForm, McpManagerState, McpView, MentionKind, MentionState, MessageRole, Modal,
    SessionBrowserState, SkillBrowserState, MCP_CUSTOM_FIELD_COUNT,
};

/// Render the entire TUI
pub fn render(frame: &mut Frame, state: &mut AppState) {
    let theme = state.theme.clone();
    let area = frame.area();

    // Fill the entire frame with the theme's background colour AND an explicit
    // space symbol in every cell.  `Block::default().style(...)` only calls
    // `Buffer::set_style` which updates `fg`/`bg`/modifier but never touches
    // `Cell::symbol`.  Cells previously written with a colored background and
    // a real character (e.g. wrapped diff lines highlighted red/green) would
    // therefore keep their old symbol on the next frame, and ratatui's diff
    // could mis-send the cell to the terminal — leaving "ghost" highlighted
    // text behind when the user scrolled past or cleared the conversation.
    // Writing a space into every cell with the theme bg defeats that:
    // every cell now has a known, uniform baseline before widgets render.
    {
        let buf = frame.buffer_mut();
        let fill_style = Style::default().bg(theme.bg).fg(theme.fg);
        let buf_area = buf.area;
        let clip = area.intersection(buf_area);
        for y in clip.top()..clip.bottom() {
            for x in clip.left()..clip.right() {
                let cell = &mut buf[(x, y)];
                cell.reset();
                cell.set_symbol(" ");
                cell.set_style(fill_style);
            }
        }
    }

    // Compute input area height based on content (min 3, max 8 rows)
    let input_lines = count_input_lines(&state.input.text, area.width.saturating_sub(2) as usize);
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

    // Inline @-mention autocomplete popup, floating above the input box.
    if let Some(m) = &state.mention {
        render_mention_popup(frame, chunks[2], m, &theme);
    }

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
            Modal::GoalManager(g) => {
                render_goal_manager(frame, g, &theme);
            }
            Modal::ContextInfo(report) => {
                render_context_info(frame, report, &theme);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Context usage modal (/context)
// ---------------------------------------------------------------------------

fn render_context_info(frame: &mut Frame, r: &super::ContextReport, theme: &Theme) {
    let area = centered_rect(74, 80, frame.area());
    frame.render_widget(Clear, area);

    let used = r.used();
    let free = r.free();
    let limit = r.context_limit.max(1);
    let pct = |t: u32| (t as f64 * 100.0 / limit as f64);
    let fmt = |t: u32| -> String {
        if t >= 1000 {
            format!("{:.1}k", t as f64 / 1000.0)
        } else {
            t.to_string()
        }
    };

    // Category colours (also used by the coin grid).
    let c_system = theme.accent_bright;
    let c_tools = theme.warning_fg;
    let c_messages = theme.accent;
    let c_free = theme.ghost_fg;

    let mut lines: Vec<Line> = Vec::new();

    // Header.
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}  ", r.model_name),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("· {} ", r.provider_name), Style::default().fg(theme.muted_fg)),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            "{} / {} tokens ({:.0}%)",
            fmt(used),
            fmt(limit),
            pct(used)
        ),
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // ── Coin grid: 10 rows × 20 = 200 coins, each ≈ limit/200 tokens. ──
    const COLS: usize = 20;
    const ROWS: usize = 10;
    const CELLS: usize = COLS * ROWS;
    let cells_for = |t: u32| ((t as f64 / limit as f64) * CELLS as f64).round() as usize;
    let sys_cells = cells_for(r.system_tokens);
    let tool_cells = cells_for(r.tools_tokens);
    let msg_cells = cells_for(r.messages_tokens);
    // Build the colour for each cell, clamping to the grid size.
    let mut colors: Vec<Color> = Vec::with_capacity(CELLS);
    for _ in 0..sys_cells.min(CELLS) {
        colors.push(c_system);
    }
    for _ in 0..tool_cells.min(CELLS.saturating_sub(colors.len())) {
        colors.push(c_tools);
    }
    for _ in 0..msg_cells.min(CELLS.saturating_sub(colors.len())) {
        colors.push(c_messages);
    }
    while colors.len() < CELLS {
        colors.push(c_free);
    }
    for row in 0..ROWS {
        let mut spans: Vec<Span> = Vec::with_capacity(COLS + 1);
        spans.push(Span::raw(" "));
        for col in 0..COLS {
            let idx = row * COLS + col;
            let filled = idx < used_cells(sys_cells, tool_cells, msg_cells);
            let glyph = if filled { "⛁ " } else { "⛶ " };
            spans.push(Span::styled(glyph, Style::default().fg(colors[idx])));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));

    // ── Estimated usage by category. ──
    lines.push(Line::from(Span::styled(
        "Estimated usage by category",
        Style::default().fg(theme.muted_fg).add_modifier(Modifier::ITALIC),
    )));
    let cat = |label: &str, tokens: u32, color: Color| -> Line {
        Line::from(vec![
            Span::styled(" ⛁ ", Style::default().fg(color)),
            Span::styled(format!("{label}: "), Style::default().fg(theme.fg)),
            Span::styled(
                format!("{} tokens ({:.1}%)", fmt(tokens), pct(tokens)),
                Style::default().fg(theme.muted_fg),
            ),
        ])
    };
    lines.push(cat("System prompt (+ memory, skills list)", r.system_tokens, c_system));
    lines.push(cat("Tools", r.tools_tokens, c_tools));
    lines.push(cat("Messages", r.messages_tokens, c_messages));
    lines.push(Line::from(vec![
        Span::styled(" ⛶ ", Style::default().fg(c_free)),
        Span::styled("Free space: ", Style::default().fg(theme.fg)),
        Span::styled(
            format!("{} tokens ({:.1}%)", fmt(free), pct(free)),
            Style::default().fg(theme.muted_fg),
        ),
    ]));
    lines.push(Line::from(""));

    // ── Breakdown sections. ──
    let section = |title: &str, hint: &str, detail: String| -> Vec<Line> {
        vec![
            Line::from(vec![
                Span::styled(
                    title.to_string(),
                    Style::default().fg(theme.accent_bright).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  · {hint}"), Style::default().fg(theme.faint_fg)),
            ]),
            Line::from(Span::styled(
                format!("  └ {detail}"),
                Style::default().fg(theme.muted_fg),
            )),
        ]
    };
    for l in section(
        "MCP tools",
        "/mcp",
        format!("{} active server(s) · {} tool(s)", r.mcp_servers_active, r.mcp_tools),
    ) {
        lines.push(l);
    }
    for l in section("Skills", "/skills", format!("{} skill(s) available", r.skills)) {
        lines.push(l);
    }
    for l in section(
        "Memory files",
        "/memory (CLAUDE.md)",
        format!("{} file(s) loaded", r.memory_files),
    ) {
        lines.push(l);
    }
    lines.push(Line::from(""));

    // ── Session usage (cumulative across the whole session). ──
    let big = |n: u64| -> String {
        if n >= 1_000_000 {
            format!("{:.2}M", n as f64 / 1_000_000.0)
        } else if n >= 1_000 {
            format!("{:.1}K", n as f64 / 1_000.0)
        } else {
            n.to_string()
        }
    };
    lines.push(Line::from(Span::styled(
        "Session usage (cumulative)",
        Style::default()
            .fg(theme.muted_fg)
            .add_modifier(Modifier::ITALIC),
    )));
    let kv = |label: &str, value: String| -> Line {
        Line::from(vec![
            Span::styled(
                format!("  {label:<16}: "),
                Style::default().fg(theme.fg),
            ),
            Span::styled(value, Style::default().fg(theme.accent_bright)),
        ])
    };
    lines.push(kv(
        "Model",
        format!("{} · {}", r.model_id, r.provider_id),
    ));
    lines.push(kv(
        "Session",
        if r.session_name.is_empty() {
            "(unnamed)".to_string()
        } else {
            r.session_name.clone()
        },
    ));
    lines.push(kv("Working dir", r.working_dir.clone()));
    lines.push(kv(
        "Current context",
        format!(
            "{} tokens ({:.0}% of {})",
            big(r.current_context_tokens),
            (r.current_context_tokens as f64 * 100.0 / limit as f64).min(100.0),
            fmt(limit)
        ),
    ));
    lines.push(kv(
        "Messages",
        format!(
            "{} ({} user · {} assistant · {} tool)",
            r.message_count, r.user_msgs, r.assistant_msgs, r.tool_msgs
        ),
    ));
    lines.push(kv("API calls", r.api_calls.to_string()));
    lines.push(kv("Input tokens", big(r.total_input_tokens)));
    lines.push(kv("Output tokens", big(r.total_output_tokens)));
    lines.push(kv("Cache read", big(r.total_cache_read_tokens)));
    lines.push(kv("Cache write", big(r.total_cache_write_tokens)));
    lines.push(kv(
        "Cache hit rate",
        format!("{:.1}%", r.cache_hit_rate),
    ));
    lines.push(kv(
        "Cache savings",
        format!("${:.4}", r.cache_savings_usd),
    ));
    lines.push(kv(
        "Total cost",
        crate::session::tokens::CostTracker::format_cost_total(r.total_cost_usd),
    ));
    lines.push(Line::from(""));

    // ── Suggestion when messages dominate. ──
    if pct(r.messages_tokens) > 50.0 {
        lines.push(Line::from(Span::styled(
            "ⓘ Messages use over half the window — /compact [keep] to free space with an AI summary.",
            Style::default().fg(theme.warning_fg),
        )));
    } else if pct(used) > 85.0 {
        lines.push(Line::from(Span::styled(
            "ⓘ Context is nearly full — consider /compact or starting a /new session.",
            Style::default().fg(theme.warning_fg),
        )));
    }

    let title = Line::from(Span::styled(
        " Context Usage   ↑↓ scroll · Esc close ",
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    ));
    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title(title),
        )
        .wrap(Wrap { trim: false })
        .scroll((r.scroll, 0));
    frame.render_widget(para, area);
}

/// Total filled cells across the three categories, clamped to the grid.
fn used_cells(sys: usize, tools: usize, msgs: usize) -> usize {
    (sys + tools + msgs).min(200)
}

// ---------------------------------------------------------------------------
// Goal manager modal
// ---------------------------------------------------------------------------

fn goal_state_icon(state: &crate::agent::goal::GoalState) -> &'static str {
    use crate::agent::goal::GoalState;
    match state {
        GoalState::Running => "●",
        GoalState::Verifying => "◐",
        GoalState::Paused => "⏸",
        GoalState::Blocked(_) => "⚠",
        GoalState::Completed => "✓",
        GoalState::Cleared => "✗",
        GoalState::Failed(_) => "✖",
        GoalState::Idle => "·",
    }
}

fn render_goal_manager(frame: &mut Frame, g: &super::GoalManagerState, theme: &Theme) {
    let area = centered_rect(84, 78, frame.area());
    frame.render_widget(Clear, area);

    let title =
        " Goals   ↑↓ nav   Enter detail   p pause   r resume   f finish   c clear   R refresh   q close ";
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
        .title(title);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Reserve one row at the bottom for the toast / confirm message.
    let (body_area, footer_area) = {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        (rows[0], rows[1])
    };

    if g.goals.is_empty() {
        let p = Paragraph::new("No goals. Start one with /goal <objective>.")
            .style(Style::default().fg(theme.muted_fg));
        frame.render_widget(p, body_area);
        return;
    }

    // Detail panel takes over the body when a snapshot is loaded.
    if let Some(snap) = &g.detail {
        render_goal_detail(frame, body_area, snap, theme);
    } else {
        render_goal_list(frame, body_area, g, theme);
    }

    // Footer: confirm prompt / toast.
    if let Some(msg) = &g.message {
        let style = if g.confirm_clear.is_some() {
            Style::default()
                .fg(theme.warning_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted_fg)
        };
        frame.render_widget(
            Paragraph::new(format!(" {}", truncate_to(msg, footer_area.width as usize)))
                .style(style),
            footer_area,
        );
    }
}

fn render_goal_list(frame: &mut Frame, area: Rect, g: &super::GoalManagerState, theme: &Theme) {
    let items: Vec<ListItem> = g
        .goals
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let selected = i == g.selected;
            let style = if selected {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            // Short id: drop the timestamp prefix so the row isn't dominated by
            // the hex blob. Keep the full id reachable in the detail panel.
            let id_str = s.id.to_string();
            let line = format!(
                " {} {:<20} {:<10} turns={:<4} ${:<8.4} {}",
                goal_state_icon(&s.state),
                truncate_to(&id_str, 20),
                truncate_to(s.state.label(), 10),
                s.turns,
                s.cost_usd,
                truncate_to(&s.objective, 80),
            );
            ListItem::new(line).style(style)
        })
        .collect();

    let scroll = g.list_scroll as usize;
    let visible = area.height as usize;
    let end = (scroll + visible).min(items.len());
    let slice: Vec<ListItem> = items[scroll.min(items.len())..end].to_vec();
    frame.render_widget(List::new(slice), area);
}

fn render_goal_detail(
    frame: &mut Frame,
    area: Rect,
    snap: &crate::agent::goal::StatusSnapshot,
    theme: &Theme,
) {
    let mut lines: Vec<Line> = Vec::new();

    fn kv(lines: &mut Vec<Line>, theme: &Theme, label: &str, val: String) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label:<12}"),
                Style::default().fg(theme.muted_fg),
            ),
            Span::styled(val, Style::default().fg(theme.fg)),
        ]));
    }

    kv(&mut lines, theme, "Goal", snap.id.to_string());
    kv(&mut lines, theme, "State", snap.state.label().to_string());
    kv(
        &mut lines,
        theme,
        "Objective",
        truncate_to(&snap.spec_objective, 110),
    );
    if !snap.spec_stopping.is_empty() && snap.spec_stopping != snap.spec_objective {
        kv(
            &mut lines,
            theme,
            "Stopping",
            truncate_to(&snap.spec_stopping, 110),
        );
    }
    let m = &snap.metrics;
    kv(&mut lines, theme, "Turns", m.turns.to_string());
    kv(
        &mut lines,
        theme,
        "Tokens",
        format!("in {} / out {}", m.input_tokens, m.output_tokens),
    );
    kv(&mut lines, theme, "Cost", format!("${:.4}", m.cost_usd));
    kv(
        &mut lines,
        theme,
        "Verifiers",
        format!("{}✓ / {}✗", m.verifiers_passed, m.verifiers_failed),
    );

    if let Some(c) = &snap.last_checkpoint {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Last checkpoint",
            Style::default()
                .fg(theme.assistant_msg_fg)
                .add_modifier(Modifier::BOLD),
        )));
        kv(
            &mut lines,
            theme,
            "  At",
            format!("{} · phase {}", c.at.format("%H:%M:%S"), c.phase),
        );
        kv(
            &mut lines,
            theme,
            "  Action",
            truncate_to(&c.last_action, 100),
        );
        if !c.files_touched.is_empty() {
            let show: Vec<String> = c
                .files_touched
                .iter()
                .take(8)
                .map(|p| p.display().to_string())
                .collect();
            let extra = if c.files_touched.len() > 8 {
                format!(" (+{} more)", c.files_touched.len() - 8)
            } else {
                String::new()
            };
            kv(
                &mut lines,
                theme,
                "  Files",
                format!("{} — {}{}", c.files_touched.len(), show.join(", "), extra),
            );
        }
    }

    if !snap.tail_progress.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Recent progress",
            Style::default()
                .fg(theme.assistant_msg_fg)
                .add_modifier(Modifier::BOLD),
        )));
        for l in snap.tail_progress.iter().rev().take(8) {
            let trimmed = l.trim();
            if trimmed.is_empty() {
                continue;
            }
            lines.push(Line::from(Span::styled(
                format!("  • {}", truncate_to(trimmed, 110)),
                Style::default().fg(theme.fg),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc / ← back to list",
        Style::default().fg(theme.muted_fg),
    )));

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        area,
    );
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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.warning_fg))
        .style(Style::default().bg(theme.modal_bg))
        .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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

/// Render a diff line prefixed with the 4-space indent and pad it on the right
/// with spaces so the colored background fills the full row width.  When the
/// line is longer than `pad_width`, it is returned as-is (ratatui's wrap will
/// handle the overflow per-row; the wrapped continuation rows won't be padded
/// but the leading row — which is what dominates the visual band — will be).
/// Make a string safe to render in the TUI.
///
/// Tool output and diffs frequently contain control characters that corrupt
/// terminal rendering and desync ratatui's width accounting from what the
/// terminal actually draws:
///   - `\r` (carriage return) moves the cursor back to column 0 mid-line, so
///     later text overwrites earlier text (this is why CRLF diffs lost their
///     `-` prefix and indentation).
///   - `\t` (tab) is counted as width 0/1 by `unicode_width` but rendered as
///     several columns by the terminal, so manually padded background bands
///     (diff highlight rows) overflow and wrap into a jagged staircase.
///   - other C0/C1 control bytes (including stray ANSI ESC) can do anything.
///
/// We drop carriage returns and other control characters and expand tabs to a
/// fixed number of spaces, so the rendered width always matches what we
/// measured. `\n` is never expected here (callers split on it first) but is
/// preserved defensively.
pub fn sanitize_for_tui(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' => out.push_str("    "),
            '\r' => {}
            '\n' => out.push('\n'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn pad_diff_line(text_line: &str, pad_width: usize) -> String {
    let prefixed = format!("    {text_line}");
    let cur = unicode_width::UnicodeWidthStr::width(prefixed.as_str());
    if cur >= pad_width || pad_width == 0 {
        prefixed
    } else {
        let pad = pad_width - cur;
        let mut out = String::with_capacity(prefixed.len() + pad);
        out.push_str(&prefixed);
        for _ in 0..pad {
            out.push(' ');
        }
        out
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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.warning_fg))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
                .title(" Set MCP Secret "),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(dialog, area);
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

/// Linearly interpolate between two `Color::Rgb` values (falls back to `a` if
/// either side isn't an RGB colour).
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    if let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) {
        let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
    } else {
        a
    }
}

/// Three-stop gradient `hi → mid → lo` sampled at `t` in `[0,1]`.
fn gradient3(hi: Color, mid: Color, lo: Color, t: f32) -> Color {
    if t < 0.5 {
        lerp_color(hi, mid, t * 2.0)
    } else {
        lerp_color(mid, lo, (t - 0.5) * 2.0)
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let bg = theme.header_bg;
    let base = Style::default().bg(bg);
    let dim = base.fg(theme.faint_fg);
    let label = base.fg(theme.muted_fg);
    let value = base.fg(theme.header_fg);

    // Warm divider drawn between header segments.
    let sep = || Span::styled(" · ", dim);

    let mut spans: Vec<Span> = Vec::new();
    // Brand mark — the molten accent, bold.
    spans.push(Span::styled(
        " ◆ forge-osh ",
        base.fg(theme.accent).add_modifier(Modifier::BOLD),
    ));
    spans.push(sep());
    spans.push(Span::styled(state.model_name.clone(), value));
    spans.push(sep());
    spans.push(Span::styled(state.provider_name.clone(), base.fg(theme.accent_bright)));
    spans.push(sep());
    spans.push(Span::styled(state.session_name.clone(), label));
    spans.push(sep());
    spans.push(Span::styled(state.format_tokens.clone(), label));
    spans.push(sep());
    spans.push(Span::styled(state.format_cost.clone(), base.fg(theme.user_msg_fg)));

    // Context window meter: ▰▰▰▱▱▱▱ 42%
    if state.context_pct > 0 {
        let filled = (state.context_pct as usize * 10 / 100).min(10);
        let empty = 10 - filled;
        let meter_color = if state.context_pct >= 90 {
            theme.error_fg
        } else if state.context_pct >= 70 {
            theme.warning_fg
        } else {
            theme.accent
        };
        spans.push(Span::styled("  ", base));
        spans.push(Span::styled("▰".repeat(filled), base.fg(meter_color)));
        spans.push(Span::styled("▱".repeat(empty), dim));
        spans.push(Span::styled(format!(" {}%", state.context_pct), base.fg(meter_color)));
    }

    // Trust + busy indicators.
    if state.trust_mode {
        spans.push(sep());
        spans.push(Span::styled(
            "TRUST",
            base.fg(theme.ok_fg).add_modifier(Modifier::BOLD),
        ));
    }
    if state.agent_busy {
        spans.push(Span::styled("  ● ", base.fg(theme.accent_bright)));
    }

    // Fill the rest of the header row with the chrome background so the bar
    // reads as a solid band rather than text on the canvas.
    let header = Paragraph::new(Line::from(spans)).style(base);
    frame.render_widget(Block::default().style(base), area);
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
            // Startup banner — modern "ANSI Shadow" block wordmark. The block
            // letters are painted in a vertical ember gradient (bright → deep),
            // the `◆ forge-osh` brand line in bright accent, and the tagline in
            // muted ash. See `OSH_SPLASH_LINES` / `splash_line_kind`.
            // ------------------------------------------------------------------
            MessageRole::Splash => {
                use crate::tui::{splash_line_kind, splash_lines, SplashKind};
                // Choose the banner that fits the pane, then paint the block
                // letters in a smooth ember gradient (bright → accent → deep).
                let banner = splash_lines(area.width);
                let total_logo = banner
                    .iter()
                    .filter(|l| matches!(splash_line_kind(l), SplashKind::Logo))
                    .count()
                    .max(1);
                let mut logo_row = 0usize;
                lines.push(Line::from(""));
                for splash_line in banner {
                    match splash_line_kind(splash_line) {
                        SplashKind::Blank => lines.push(Line::from("")),
                        SplashKind::Logo => {
                            let t = logo_row as f32 / (total_logo.saturating_sub(1).max(1)) as f32;
                            let color = gradient3(
                                theme.accent_bright,
                                theme.accent,
                                theme.accent_dim,
                                t,
                            );
                            logo_row += 1;
                            lines.push(Line::from(Span::styled(
                                splash_line.to_string(),
                                Style::default().fg(color).add_modifier(Modifier::BOLD),
                            )));
                        }
                        SplashKind::Brand => {
                            // Split into "◆  forge-osh" (bright) + remainder (muted).
                            let trimmed = splash_line.trim_end();
                            let mut spans: Vec<Span> = Vec::new();
                            if let Some(idx) = trimmed.find("forge-osh") {
                                let head = &trimmed[..idx + "forge-osh".len()];
                                let tail = &trimmed[idx + "forge-osh".len()..];
                                spans.push(Span::styled(
                                    head.to_string(),
                                    Style::default()
                                        .fg(theme.accent_bright)
                                        .add_modifier(Modifier::BOLD),
                                ));
                                spans.push(Span::styled(
                                    tail.to_string(),
                                    Style::default().fg(theme.muted_fg),
                                ));
                            } else {
                                spans.push(Span::styled(
                                    trimmed.to_string(),
                                    Style::default()
                                        .fg(theme.accent_bright)
                                        .add_modifier(Modifier::BOLD),
                                ));
                            }
                            lines.push(Line::from(spans));
                        }
                        SplashKind::Tagline => {
                            lines.push(Line::from(Span::styled(
                                splash_line.to_string(),
                                Style::default().fg(theme.faint_fg),
                            )));
                        }
                    }
                }
                lines.push(Line::from(""));
            }

            MessageRole::User => {
                lines.push(Line::from(vec![Span::styled(
                    " You ",
                    Style::default()
                        .fg(theme.badge_fg)
                        .bg(theme.user_msg_fg)
                        .add_modifier(Modifier::BOLD),
                )]));
                for text_line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", sanitize_for_tui(text_line)),
                        Style::default().fg(theme.user_msg_fg),
                    )));
                }
                lines.push(Line::from(""));
            }

            MessageRole::Assistant => {
                lines.push(Line::from(vec![Span::styled(
                    " forge ",
                    Style::default()
                        .fg(theme.badge_fg)
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
                            .fg(theme.badge_fg)
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

                // Available width for padding diff lines so their colored
                // background fills the row (avoids jagged right edges and
                // makes the buffer-diff between frames unambiguous).
                let pad_width = (area.width as usize).saturating_sub(5);
                for raw_line in preview.iter().take(max_lines) {
                    // Strip control characters (\r, \t, etc.) before rendering so
                    // the colored diff bands and width math stay in sync with the
                    // terminal. Prefix detection uses the same cleaned text.
                    let text_line = sanitize_for_tui(raw_line);
                    let text_line = text_line.as_str();
                    if is_diff {
                        if text_line.starts_with('+') && !text_line.starts_with("+++") {
                            // Addition — bright green text on dark green background
                            lines.push(Line::from(Span::styled(
                                pad_diff_line(text_line, pad_width),
                                Style::default().fg(theme.added_fg).bg(theme.added_bg),
                            )));
                        } else if text_line.starts_with('-') && !text_line.starts_with("---") {
                            // Removal — bright red text on dark red background
                            lines.push(Line::from(Span::styled(
                                pad_diff_line(text_line, pad_width),
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
                for text_line in sanitize_for_tui(&msg.content).lines() {
                    lines.push(Line::from(Span::styled(
                        format!("  {text_line}"),
                        Style::default().fg(theme.warning_fg),
                    )));
                }
                lines.push(Line::from(""));
            }
        }
    }

    // Streaming text (currently being generated)
    if !state.streaming_text.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " forge ",
            Style::default()
                .fg(theme.badge_fg)
                .bg(theme.assistant_msg_fg)
                .add_modifier(Modifier::BOLD),
        )]));
        render_assistant_content(&mut lines, &state.streaming_text, theme);
    }

    // Live reasoning (intermediate thought process) — dimmed, ephemeral.
    if !state.thinking_text.is_empty() {
        render_thinking(&mut lines, &state.thinking_text, theme);
    }

    // Live task plan — the ticking checklist, rendered just above the spinner.
    if let Some(plan) = &state.current_plan {
        if !plan.is_empty() {
            render_plan_panel(&mut lines, plan, theme);
        }
    }

    // Spinner (thinking indicator)
    if state.spinner.active {
        lines.push(Line::from(Span::styled(
            format!("  {}", state.spinner.display()),
            Style::default().fg(theme.spinner_fg),
        )));
    }

    // Background-goal activity indicator. Goals run independently of the
    // foreground turn, so the normal spinner above is silent while a goal
    // churns. Show an animated line here whenever there are live goals so the
    // user can see work is happening before the final checkpoint lands. The
    // detailed progress remains available via `/goal` / `/goal-check`.
    {
        let blurb = state.goal_status_blurb.lock().clone();
        if !blurb.is_empty() {
            let frame = super::spinner::SPINNER_FRAMES
                [state.goal_anim_frame % super::spinner::SPINNER_FRAMES.len()];
            lines.push(Line::from(Span::styled(
                format!("  {frame} {blurb}  (/goal to manage)"),
                Style::default().fg(theme.spinner_fg),
            )));
        }
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
            .end_symbol(Some("▼"))
            .thumb_style(Style::default().fg(theme.accent))
            .track_style(Style::default().fg(theme.scrollbar_fg));
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Render the model's live, intermediate reasoning as dimmed italic lines so
/// the user can follow the thought process behind tool calls. Ephemeral —
/// cleared once the visible answer or a tool result supersedes it.
fn render_thinking(lines: &mut Vec<Line>, text: &str, theme: &Theme) {
    lines.push(Line::from(Span::styled(
        "  💭 thinking",
        Style::default()
            .fg(theme.muted_fg)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
    )));
    // Only show the tail so a long reasoning trace never floods the viewport.
    let collected: Vec<&str> = text.lines().collect();
    let start = collected.len().saturating_sub(12);
    for line in &collected[start..] {
        lines.push(Line::from(Span::styled(
            format!("    {}", sanitize_for_tui(line)),
            Style::default()
                .fg(theme.muted_fg)
                .add_modifier(Modifier::ITALIC),
        )));
    }
}

/// Render the persistent task plan as a live checklist whose steps tick off in
/// real time (the equivalent of Claude Code / Codex's task manager panel).
fn render_plan_panel(lines: &mut Vec<Line>, plan: &crate::session::TaskPlan, theme: &Theme) {
    use crate::session::StepStatus;

    let (done, total) = plan.progress();
    let title = if plan.title.is_empty() {
        "Plan"
    } else {
        &plan.title
    };

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "● ",
            Style::default()
                .fg(theme.prompt_fg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            title.to_string(),
            Style::default()
                .fg(theme.fg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ({done}/{total})"),
            Style::default().fg(theme.muted_fg),
        ),
    ]));

    let multi_task = plan.tasks.len() > 1;
    let max_step_lines: usize = 24;
    let mut emitted = 0usize;

    for task in &plan.tasks {
        if multi_task {
            let (td, tn) = task.step_progress();
            lines.push(Line::from(vec![
                Span::styled("  └ ", Style::default().fg(theme.border_fg)),
                Span::styled(
                    sanitize_for_tui(&task.subject),
                    Style::default()
                        .fg(theme.assistant_msg_fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  ({td}/{tn})"),
                    Style::default().fg(theme.muted_fg),
                ),
            ]));
        }
        for step in &task.steps {
            if emitted >= max_step_lines {
                lines.push(Line::from(Span::styled(
                    "    …".to_string(),
                    Style::default().fg(theme.muted_fg),
                )));
                break;
            }
            emitted += 1;
            let (fg, modifier) = match step.status {
                StepStatus::InProgress => (theme.tool_name_fg, Modifier::BOLD),
                StepStatus::Completed => (theme.added_fg, Modifier::empty()),
                StepStatus::Blocked => (theme.error_fg, Modifier::BOLD),
                StepStatus::Pending => (theme.muted_fg, Modifier::empty()),
            };
            let indent = if multi_task { "      " } else { "    " };
            let mut spans = vec![
                Span::styled(
                    format!("{indent}{} ", step.status.checkbox()),
                    Style::default().fg(fg),
                ),
                Span::styled(
                    sanitize_for_tui(&step.content),
                    Style::default().fg(fg).add_modifier(modifier),
                ),
            ];
            if let Some(note) = &step.note {
                spans.push(Span::styled(
                    format!("  — {}", sanitize_for_tui(note)),
                    Style::default()
                        .fg(theme.muted_fg)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            lines.push(Line::from(spans));
        }
        if emitted >= max_step_lines {
            break;
        }
    }
    lines.push(Line::from(""));
}

/// Render assistant message content with basic markdown support.
fn render_assistant_content(lines: &mut Vec<Line>, content: &str, theme: &Theme) {
    // Strip control characters (\r, \t, …) up front so pasted code or
    // tool-echoed text can never corrupt terminal rendering or width math.
    let content = sanitize_for_tui(content);
    let content = content.as_str();

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
                    strip_inline_markers(heading),
                    Style::default()
                        .fg(theme.accent_bright)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    strip_inline_markers(heading),
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
                    strip_inline_markers(heading),
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

/// Parse a line with inline markdown: `**bold**`, `__bold__`, `*italic*`,
/// `_italic_`, `` `code` ``, and inline math (`\( … \)`, `$ … $`).
///
/// Emphasis is only applied when a **matching closing marker exists on the
/// line**; an unmatched marker (e.g. a stray `**`) is emitted as literal text
/// rather than greedily swallowing the rest of the line or being dropped. This
/// is the fix for `**bold**` rendering literally / inconsistently in list items
/// and headings: the previous scanner advanced past the opening marker
/// unconditionally and used an off-by-one (`i + 1 < len`) closing test that
/// mishandled markers landing at the end of the string.
fn parse_inline_markdown(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut current = String::new();

    while i < n {
        // Inline math: \( ... \)
        if chars[i] == '\\' && i + 1 < n && chars[i + 1] == '(' {
            if let Some(end) = find_latex_inline_end(&chars, i + 2) {
                push_plain_span(&mut spans, &mut current, theme);
                let expr: String = chars[i + 2..end].iter().collect();
                spans.push(math_span(&expr, theme));
                i = end + 2;
                continue;
            }
        }

        // Inline math: $ ... $. Avoid treating $$ display delimiters as inline.
        if chars[i] == '$' && (i + 1 >= n || chars[i + 1] != '$') {
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

        // **bold** or __bold__ — only when a matching closing pair exists.
        if i + 1 < n
            && ((chars[i] == '*' && chars[i + 1] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_'))
        {
            let marker = chars[i];
            if let Some(close) = find_double_marker(&chars, i + 2, marker) {
                push_plain_span(&mut spans, &mut current, theme);
                let inner: String = chars[i + 2..close].iter().collect();
                spans.push(Span::styled(
                    inner,
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ));
                i = close + 2; // skip past the closing marker pair
                continue;
            }
            // Unmatched — render the two marker chars literally.
            current.push(chars[i]);
            current.push(chars[i + 1]);
            i += 2;
            continue;
        }

        // *italic* or _italic_ — single marker, needs a non-space start and a
        // matching closing marker on the line.
        if (chars[i] == '*' || chars[i] == '_')
            && i + 1 < n
            && chars[i + 1] != ' '
            && (i == 0 || chars[i - 1] != chars[i])
        {
            let marker = chars[i];
            if let Some(close) = find_single_marker(&chars, i + 1, marker) {
                push_plain_span(&mut spans, &mut current, theme);
                let inner: String = chars[i + 1..close].iter().collect();
                spans.push(Span::styled(
                    inner,
                    Style::default()
                        .fg(theme.assistant_msg_fg)
                        .add_modifier(Modifier::ITALIC),
                ));
                i = close + 1;
                continue;
            }
            current.push(chars[i]);
            i += 1;
            continue;
        }

        // `inline code`
        if chars[i] == '`' {
            if let Some(close) = find_single_marker(&chars, i + 1, '`') {
                push_plain_span(&mut spans, &mut current, theme);
                let inner: String = chars[i + 1..close].iter().collect();
                spans.push(Span::styled(inner, Style::default().fg(theme.added_fg)));
                i = close + 1;
                continue;
            }
            current.push('`');
            i += 1;
            continue;
        }

        current.push(chars[i]);
        i += 1;
    }

    push_plain_span(&mut spans, &mut current, theme);
    spans
}

/// Index of the next `marker marker` pair at or after `from`, or None.
fn find_double_marker(chars: &[char], from: usize, marker: char) -> Option<usize> {
    let mut j = from;
    while j + 1 < chars.len() {
        if chars[j] == marker && chars[j + 1] == marker {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Index of the next single `marker` at or after `from`, or None.
fn find_single_marker(chars: &[char], from: usize, marker: char) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        if chars[j] == marker {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Strip paired emphasis/code markers from a heading line so `## **Title**`
/// renders as a bold heading without literal `**`/`` ` `` showing through.
fn strip_inline_markers(s: &str) -> String {
    s.replace("**", "").replace("__", "").replace('`', "")
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
                                format!("      {}", sanitize_for_tui(content_line)),
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
                            Span::styled(sanitize_for_tui(s), Style::default().fg(theme.fg)),
                        ]));
                    }
                    _ => {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("    {key}: "),
                                Style::default().fg(theme.tool_name_fg),
                            ),
                            Span::styled(
                                sanitize_for_tui(&value.to_string()),
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
                format!("    {}", sanitize_for_tui(line)),
                Style::default().fg(theme.muted_fg),
            )));
        }
    }
}

// ---------------------------------------------------------------------------
// Input area
// ---------------------------------------------------------------------------

fn render_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    // Draw the top rule + prompt first, then the text into a padded inner area.
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
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.accent_dim))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // One column of breathing room on each side; the text origin and the
    // cursor share this exact rect so the caret always sits on its character.
    let text_area = Rect {
        x: inner.x + 1,
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };
    let cols = text_area.width as usize;
    let rows = text_area.height as usize;

    let (display_text, text_style, cursor) = if state.input.text.is_empty() {
        let placeholder = if state.agent_busy {
            "Agent is working... (Ctrl+C to cancel)"
        } else {
            "Type a message or /help for commands..."
        };
        (
            placeholder.to_string(),
            Style::default().fg(theme.faint_fg),
            None,
        )
    } else {
        let viewport = input_viewport(
            &state.input.text,
            state.input.cursor,
            state.input.scroll_top,
            rows.max(1),
            cols.max(1),
        );
        (
            viewport.text,
            Style::default().fg(theme.fg),
            Some((viewport.cursor_x, viewport.cursor_y)),
        )
    };

    let para = Paragraph::new(display_text)
        .style(text_style)
        .wrap(Wrap { trim: false });
    frame.render_widget(para, text_area);

    if let Some((x, y)) = cursor {
        let max_x = text_area.x + text_area.width.saturating_sub(1);
        let max_y = text_area.y + text_area.height.saturating_sub(1);
        let cursor_x = (text_area.x + x as u16).min(max_x);
        let cursor_y = (text_area.y + y as u16).min(max_y);
        frame.set_cursor_position((cursor_x, cursor_y));
    } else {
        frame.set_cursor_position((text_area.x, text_area.y));
    }
}

struct InputViewport {
    text: String,
    cursor_x: usize,
    cursor_y: usize,
}

/// Lay the input text out into visual rows that **soft-wrap** at
/// `visible_cols`, then vertically scroll so the cursor row stays in view.
///
/// Each explicit `\n` is a hard break (so multi-line / clipboard paste keeps
/// its line structure); any logical line longer than the box width flows onto
/// the next visual row instead of scrolling horizontally and hiding the start
/// of the prompt. `cursor` is a **byte** offset into `text`.
fn input_viewport(
    text: &str,
    cursor: usize,
    scroll_top: usize,
    visible_rows: usize,
    visible_cols: usize,
) -> InputViewport {
    let cols = visible_cols.max(1);
    let rows_visible = visible_rows.max(1);

    // Clamp the cursor to a char boundary (defensive against mid-UTF-8 indices).
    let mut cur = cursor.min(text.len());
    while cur > 0 && !text.is_char_boundary(cur) {
        cur -= 1;
    }

    // Single pass: build wrapped visual rows AND locate the cursor's
    // (visual_row, visual_col). The cursor is recorded *after* any soft-wrap
    // decision for the current char, so a cursor sitting exactly on a wrap
    // boundary lands at column 0 of the next row (standard editor behaviour).
    let mut rows: Vec<String> = Vec::new();
    let mut cur_row = String::new();
    let mut cur_w = 0usize;
    let mut byte = 0usize;
    let mut cursor_vr = 0usize;
    let mut cursor_vc = 0usize;
    let mut cursor_set = false;

    for ch in text.chars() {
        if ch == '\n' {
            if !cursor_set && byte == cur {
                cursor_vr = rows.len();
                cursor_vc = cur_w;
                cursor_set = true;
            }
            rows.push(std::mem::take(&mut cur_row));
            cur_w = 0;
            byte += 1;
            continue;
        }
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if cur_w + w > cols && cur_w > 0 {
            rows.push(std::mem::take(&mut cur_row));
            cur_w = 0;
        }
        if !cursor_set && byte == cur {
            cursor_vr = rows.len();
            cursor_vc = cur_w;
            cursor_set = true;
        }
        cur_row.push(ch);
        cur_w += w;
        byte += ch.len_utf8();
    }
    if !cursor_set {
        // Cursor at end of text.
        cursor_vr = rows.len();
        cursor_vc = cur_w;
    }
    rows.push(cur_row);

    // Vertical scroll so the cursor row is visible.
    let max_scroll = rows.len().saturating_sub(rows_visible);
    let mut start = scroll_top.min(max_scroll);
    if cursor_vr < start {
        start = cursor_vr;
    } else if cursor_vr >= start + rows_visible {
        start = cursor_vr + 1 - rows_visible;
    }

    let rendered: Vec<String> = rows
        .into_iter()
        .skip(start)
        .take(rows_visible)
        .collect();

    InputViewport {
        text: rendered.join("\n"),
        cursor_x: cursor_vc.min(cols),
        cursor_y: cursor_vr.saturating_sub(start).min(rows_visible - 1),
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
    // Transient toast (e.g. image-paste feedback) takes over the bar briefly.
    if let Some((msg, _)) = &state.toast {
        let base = Style::default().bg(theme.status_bg);
        frame.render_widget(Block::default().style(base), area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {msg}"),
                base.fg(theme.accent_bright).add_modifier(Modifier::BOLD),
            )))
            .style(base),
            area,
        );
        return;
    }

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

    // Background process indicator (P1.4): show how many detached processes the
    // agent has running so the user can see dev servers / watchers at a glance.
    let proc_indicator = {
        let running = crate::tools::process::registry().lock().running_count();
        if running > 0 {
            format!("  ⚙ {running} bg")
        } else {
            String::new()
        }
    };

    let bg = theme.status_bg;
    let base = Style::default().bg(bg);
    let keycap = base.fg(theme.accent).add_modifier(Modifier::BOLD);
    let lbl = base.fg(theme.status_fg);

    // Build the `^`-key legend as alternating accent-keycap / dim-label spans.
    let mut spans: Vec<Span> = Vec::new();
    let mut shortcut = |spans: &mut Vec<Span>, k: &'static str, name: &'static str| {
        spans.push(Span::styled(format!(" {k} "), keycap));
        spans.push(Span::styled(name, lbl));
    };
    shortcut(&mut spans, "^C", "Cancel");
    shortcut(&mut spans, "^O", "Model");
    shortcut(&mut spans, "^P", "Provider");
    shortcut(&mut spans, "^K", "Keys");
    shortcut(&mut spans, "^B", "Cost");
    shortcut(&mut spans, "^R", "Theme");
    // Trust toggle with on/off colouring.
    spans.push(Span::styled(" ^T ", keycap));
    spans.push(Span::styled("Trust[", lbl));
    spans.push(Span::styled(
        trust,
        if state.trust_mode {
            base.fg(theme.ok_fg).add_modifier(Modifier::BOLD)
        } else {
            base.fg(theme.faint_fg)
        },
    ));
    spans.push(Span::styled("]", lbl));
    spans.push(Span::styled("  /help ", base.fg(theme.accent_bright)));
    spans.push(Span::styled("Cmds", lbl));

    if !skill_indicator.is_empty() {
        spans.push(Span::styled(skill_indicator, base.fg(theme.user_msg_fg)));
    }
    if !goal_indicator.is_empty() {
        spans.push(Span::styled(goal_indicator, base.fg(theme.accent_bright)));
    }
    if !proc_indicator.is_empty() {
        spans.push(Span::styled(
            proc_indicator,
            base.fg(theme.ok_fg).add_modifier(Modifier::BOLD),
        ));
    }
    if !scroll_info.is_empty() {
        spans.push(Span::styled(scroll_info, base.fg(theme.faint_fg)));
    }
    if !unread_indicator.is_empty() {
        spans.push(Span::styled(
            unread_indicator,
            base.fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    }

    frame.render_widget(Block::default().style(base), area);
    frame.render_widget(Paragraph::new(Line::from(spans)).style(base), area);
}

// ---------------------------------------------------------------------------
// Modals
// ---------------------------------------------------------------------------

/// Shared rounded modal frame: warm accent-dim border + filled overlay body.
/// Callers add their own `.title(...)` (an accent-styled `Line`).
fn themed_modal_block<'a>(theme: &Theme) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        // Brighter accent edge gives the floating panel a vivid "glass rim".
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.modal_bg))
}

fn render_confirmation(
    frame: &mut Frame,
    tool_name: &str,
    description: &str,
    scroll: u16,
    theme: &Theme,
) {
    let area = centered_rect(78, 72, frame.area());
    frame.render_widget(Clear, area);

    let muted = Style::default().fg(theme.muted_fg);
    let mut body: Vec<Line> = Vec::new();
    body.push(Line::from(Span::styled(
        "⚠ Permission Required",
        Style::default()
            .fg(theme.warning_fg)
            .add_modifier(Modifier::BOLD),
    )));
    body.push(Line::from(""));
    body.push(Line::from(vec![
        Span::styled("Tool  ", muted),
        Span::styled(
            tool_name.to_string(),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
    ]));
    body.push(Line::from(""));
    for l in description.lines() {
        body.push(Line::from(Span::styled(
            sanitize_for_tui(l),
            Style::default().fg(theme.fg),
        )));
    }
    body.push(Line::from(""));
    // Action key legend with accent keycaps.
    let kc = |k: &'static str, name: &'static str| {
        vec![
            Span::styled(
                format!(" {k} "),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{name}   "), muted),
        ]
    };
    let mut keys: Vec<Span> = Vec::new();
    keys.extend(kc("Y", "Allow"));
    keys.extend(kc("N", "Deny"));
    keys.extend(kc("A", "Always"));
    keys.extend(kc("T", "Trust"));
    body.push(Line::from(keys));
    body.push(Line::from(Span::styled(
        "↑↓ / PgUp PgDn  scroll",
        Style::default().fg(theme.faint_fg),
    )));

    let title = Line::from(Span::styled(
        " Confirm Action ",
        Style::default()
            .fg(theme.warning_fg)
            .add_modifier(Modifier::BOLD),
    ));
    let dialog = Paragraph::new(body)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.warning_fg))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
                .title(title),
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.warning_fg))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.warning_fg).add_modifier(Modifier::BOLD))
                .title(" Large Clipboard Paste "),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(dialog, area);
}

fn render_help(frame: &mut Frame, theme: &Theme, h: &HelpState) {
    let area = centered_rect(82, 85, frame.area());
    frame.render_widget(Clear, area);

    // Outer frame + title, then carve the interior into a scrollable body and a
    // one-row search bar pinned to the bottom.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_dim))
        .style(Style::default().bg(theme.modal_bg))
        .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
        .title(" Help  ↑↓/jk scroll · / search · n/N next/prev · Esc close ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let search_area = chunks[1];

    let body_text = super::help::help_text();
    let q = h.query.trim().to_lowercase();
    let current_line = h.matches.get(h.current_match).copied();

    // Build styled lines, highlighting query matches (current match strongest).
    let mut lines: Vec<Line> = Vec::with_capacity(body_text.lines().count());
    for (i, raw) in body_text.lines().enumerate() {
        if q.is_empty() {
            lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(theme.fg),
            )));
        } else {
            let is_current = Some(i as u16) == current_line;
            lines.push(highlight_line(raw, &q, is_current, theme));
        }
    }

    let total_lines = lines.len() as u16;
    let max_scroll = total_lines.saturating_sub(body_area.height);
    let scroll = h.scroll.min(max_scroll);

    let help = Paragraph::new(lines)
        .style(Style::default().fg(theme.fg))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(help, body_area);

    if max_scroll > 0 {
        let mut sb_state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .thumb_style(Style::default().fg(theme.accent))
            .track_style(Style::default().fg(theme.scrollbar_fg));
        frame.render_stateful_widget(sb, body_area, &mut sb_state);
    }

    // ── Search bar ────────────────────────────────────────────────────────
    let count = if h.query.trim().is_empty() {
        String::new()
    } else if h.matches.is_empty() {
        "  (no matches)".to_string()
    } else {
        format!("  ({}/{})", h.current_match + 1, h.matches.len())
    };
    let icon_style = if h.search_active {
        Style::default()
            .fg(theme.accent_bright)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.faint_fg)
    };
    let query_text = if h.query.is_empty() && !h.search_active {
        "Press / to search".to_string()
    } else if h.search_active {
        format!("{}\u{2588}", h.query) // block cursor while focused
    } else {
        h.query.clone()
    };
    let hint = if h.search_active {
        "   ↑↓ prev/next · Enter next · Esc done"
    } else {
        "   / search · n/N jump"
    };
    let bar = Line::from(vec![
        Span::styled(" search ", icon_style),
        Span::styled(query_text, Style::default().fg(theme.fg)),
        Span::styled(count, Style::default().fg(theme.accent)),
        Span::styled(hint, Style::default().fg(theme.faint_fg)),
    ]);
    frame.render_widget(
        Paragraph::new(bar).style(Style::default().bg(theme.modal_bg)),
        search_area,
    );
}

/// Build a styled help line, highlighting every (case-insensitive) occurrence
/// of `q_lower`. The current match line uses a filled highlight; other match
/// lines use a bright accent. Falls back to whole-line highlight for non-ASCII
/// lines so byte-offset slicing can never panic on multi-byte characters.
fn highlight_line(raw: &str, q_lower: &str, is_current: bool, theme: &Theme) -> Line<'static> {
    let base = Style::default().fg(theme.fg);
    let hl = if is_current {
        Style::default()
            .fg(theme.modal_bg)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.accent_bright)
            .add_modifier(Modifier::BOLD)
    };

    if q_lower.is_empty() {
        return Line::from(Span::styled(raw.to_string(), base));
    }

    // Non-ASCII: avoid byte-offset slicing across multi-byte chars.
    if !raw.is_ascii() || !q_lower.is_ascii() {
        if raw.to_lowercase().contains(q_lower) {
            return Line::from(Span::styled(raw.to_string(), hl));
        }
        return Line::from(Span::styled(raw.to_string(), base));
    }

    // ASCII fast path: to_lowercase preserves byte length, so offsets into the
    // lowercased copy map 1:1 onto `raw` and always land on char boundaries.
    let lower = raw.to_ascii_lowercase();
    let qlen = q_lower.len();
    let mut spans: Vec<Span> = Vec::new();
    let mut start = 0usize;
    while let Some(rel) = lower[start..].find(q_lower) {
        let m = start + rel;
        if m > start {
            spans.push(Span::styled(raw[start..m].to_string(), base));
        }
        let end = m + qlen;
        spans.push(Span::styled(raw[m..end].to_string(), hl));
        start = end;
    }
    if start < raw.len() {
        spans.push(Span::styled(raw[start..].to_string(), base));
    }
    if spans.is_empty() {
        spans.push(Span::styled(raw.to_string(), base));
    }
    Line::from(spans)
}

/// Inline `@`-mention picker, floating directly above the input box (P0.2).
fn render_mention_popup(frame: &mut Frame, input_area: Rect, m: &MentionState, theme: &Theme) {
    let screen = frame.area();
    let max_rows = 10u16;
    let list_h = (m.candidates.len() as u16).min(max_rows).max(1);
    let height = list_h + 2; // borders
    let width = 72u16.min(screen.width.saturating_sub(4)).max(24);
    let x = input_area
        .x
        .saturating_add(1)
        .min(screen.width.saturating_sub(width));
    let y = input_area.y.saturating_sub(height);
    let popup = Rect {
        x,
        y,
        width,
        height,
    };
    frame.render_widget(Clear, popup);

    let title = format!(" @{}  ↑↓ select · Enter/Tab insert · Esc cancel ", m.query);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.modal_bg))
        .title_style(
            Style::default()
                .fg(theme.accent_bright)
                .add_modifier(Modifier::BOLD),
        )
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if m.candidates.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  no matches",
                Style::default().fg(theme.faint_fg),
            ))
            .style(Style::default().bg(theme.modal_bg)),
            inner,
        );
        return;
    }

    let visible = inner.height as usize;
    let start = if m.selected >= visible {
        m.selected - visible + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, c) in m.candidates.iter().enumerate().skip(start).take(visible) {
        let is_sel = i == m.selected;
        let icon = match c.kind {
            MentionKind::Dir => "▸ ",
            MentionKind::File => "· ",
            MentionKind::Symbol => "ƒ ",
            MentionKind::Special => "* ",
        };
        let row_style = if is_sel {
            Style::default()
                .fg(theme.modal_bg)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg)
        };
        let hint_style = if is_sel {
            Style::default().fg(theme.modal_bg).bg(theme.accent)
        } else {
            Style::default().fg(theme.faint_fg)
        };
        let mut spans = vec![
            Span::styled(
                format!("{}{}", if is_sel { "> " } else { "  " }, icon),
                row_style,
            ),
            Span::styled(c.label.clone(), row_style),
        ];
        if !c.hint.is_empty() {
            spans.push(Span::styled(format!("  {}", c.hint), hint_style));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.modal_bg)),
        inner,
    );
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
                .title(title),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(p, area);

    if max_scroll > 0 {
        let mut sb_state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .thumb_style(Style::default().fg(theme.accent))
            .track_style(Style::default().fg(theme.scrollbar_fg));
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.spinner_fg))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.spinner_fg))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
            let (mark, mark_style) = if item.connected {
                ("●", Style::default().fg(theme.ok_fg))
            } else {
                ("○", Style::default().fg(theme.faint_fg))
            };
            let line = Line::from(vec![
                Span::styled(format!(" {mark} "), mark_style),
                Span::styled(
                    format!("{:<20}", item.provider_name),
                    Style::default().fg(theme.accent_bright),
                ),
                Span::styled(
                    format!("{:<30}", item.model_name),
                    Style::default().fg(theme.fg),
                ),
                Span::styled(
                    item.cost_display.clone(),
                    Style::default().fg(theme.user_msg_fg),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let title = if picker.filtering {
        Line::from(vec![
            Span::styled(" Models ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" filter: ", Style::default().fg(theme.muted_fg)),
            Span::styled(format!("{} ", picker.filter), Style::default().fg(theme.accent_bright)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Models ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(
                " / filter   ↑↓ navigate   Enter select   Esc close ",
                Style::default().fg(theme.faint_fg),
            ),
        ])
    };

    let list = List::new(items)
        .block(themed_modal_block(theme).title(title))
        .highlight_symbol("▎")
        .highlight_style(
            Style::default()
                .fg(theme.selection_fg)
                .bg(theme.selection_bg)
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
    let cache_line = if state.format_cache_summary.is_empty() {
        "Cache    : (no cached tokens yet this session)".to_string()
    } else {
        format!("Cache    : {}", state.format_cache_summary)
    };
    let text = format!(
        "Usage & Cost\n\n\
         Provider : {}\n\
         Model    : {}\n\
         Context  : [{}] {}% of {} tokens\n\
         Tokens   : {}\n\
         Cost     : {}\n\
         {}\n\n\
         Context % reflects the last prompt's size in the model's window.\n\
         Cumulative tokens keep growing as each turn adds to the bill.\n\
         Cached tokens are billed at a discount (Anthropic 0.1×, OpenAI/\n\
         DeepSeek 0.5×, Gemini 0.25× of the input rate).\n\
         Use /compact [keep] to free context with an AI summary.\n\n\
         Press Esc to close",
        state.provider_name,
        state.model_name,
        ctx_bar,
        state.context_pct,
        state.context_limit,
        state.format_tokens,
        state.format_cost,
        cache_line,
    );

    let info = Paragraph::new(text)
        .style(Style::default().fg(theme.fg))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme.warning_fg))
                    .style(Style::default().bg(theme.modal_bg))
                    .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.spinner_fg))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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
                .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent_dim))
                .style(Style::default().bg(theme.modal_bg))
                .title_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
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

    /// Concatenate the textual content of all spans (ignoring styling).
    fn spans_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// True if the span carrying `needle` is bold.
    fn needle_is_bold(spans: &[Span<'static>], needle: &str) -> bool {
        spans
            .iter()
            .any(|s| s.content.contains(needle) && s.style.add_modifier.contains(Modifier::BOLD))
    }

    #[test]
    fn bold_at_end_of_line_has_no_literal_stars() {
        let theme = Theme::molten_rust();
        // Numbered-heading style content: the whole line is one bold run ending
        // exactly at the string boundary.
        let spans = parse_inline_markdown(
            "**Comprehensive Feature Ideation (Strictly Planning Mode)**",
            &theme,
        );
        let text = spans_text(&spans);
        assert!(!text.contains("**"), "literal ** leaked: {text:?}");
        assert_eq!(text, "Comprehensive Feature Ideation (Strictly Planning Mode)");
        assert!(needle_is_bold(&spans, "Comprehensive"));
    }

    #[test]
    fn bold_then_trailing_text_renders_clean() {
        let theme = Theme::molten_rust();
        let spans = parse_inline_markdown("**Entered Planning Mode**: Activated read-only", &theme);
        let text = spans_text(&spans);
        assert!(!text.contains("**"), "literal ** leaked: {text:?}");
        assert!(text.starts_with("Entered Planning Mode"));
        assert!(text.ends_with(": Activated read-only"));
        assert!(needle_is_bold(&spans, "Entered Planning Mode"));
    }

    #[test]
    fn unmatched_double_star_is_literal_not_swallowed() {
        let theme = Theme::molten_rust();
        let spans = parse_inline_markdown("a ** b without close", &theme);
        let text = spans_text(&spans);
        assert_eq!(text, "a ** b without close");
        // Nothing should have been bolded.
        assert!(!spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn inline_code_and_italic_render() {
        let theme = Theme::molten_rust();
        let spans = parse_inline_markdown("use `cargo build` and *care*", &theme);
        let text = spans_text(&spans);
        assert_eq!(text, "use cargo build and care");
    }

    #[test]
    fn heading_markers_are_stripped() {
        assert_eq!(strip_inline_markers("**Multi-Agent Architecture**"), "Multi-Agent Architecture");
        assert_eq!(strip_inline_markers("plain heading"), "plain heading");
    }

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
