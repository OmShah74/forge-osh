use ratatui::style::Color;

/// Convert a packed `0xRRGGBB` hex literal into a ratatui `Color::Rgb`.
/// Lets the palette tables below read exactly like the design tokens in
/// `UI_Overhaul/colors_and_type.css`.
const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Lighten a packed `0xRRGGBB` colour by adding `amt` to each channel
/// (saturating). Used to push the accent ramp brighter and to lift panel
/// surfaces above the canvas for a layered, glass-like sense of depth —
/// uniformly across every theme.
const fn lighten(hex: u32, amt: u8) -> u32 {
    let r = (hex >> 16) as u8;
    let g = (hex >> 8) as u8;
    let b = hex as u8;
    let r = r.saturating_add(amt) as u32;
    let g = g.saturating_add(amt) as u32;
    let b = b.saturating_add(amt) as u32;
    (r << 16) | (g << 8) | b
}

/// A forge-osh theme.
///
/// The colour system is the "Molten Rust" design language (see
/// `UI_Overhaul/colors_and_type.css`): near-black warm-tinted ash backgrounds,
/// a single saturated **accent ramp** (the "fluid" colour of the theme), warm
/// foreground whites→taupes, and functional diff colours. Every alternate
/// theme keeps the exact same structure but swaps the accent hue and tints the
/// ash toward it (fluid green, liquid blue, glittery gold, bright neon, fluid
/// purple).
///
/// The first block of fields is the original, stable surface consumed across
/// `renderer.rs`. The second block (`accent*`, `panel_bg`, …) are the
/// design-system additions used by the overhauled renderer.
#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub user_msg_fg: Color,
    pub assistant_msg_fg: Color,
    pub tool_name_fg: Color,
    pub added_fg: Color,
    pub added_bg: Color,
    pub removed_fg: Color,
    pub removed_bg: Color,
    pub border_fg: Color,
    pub header_bg: Color,
    pub header_fg: Color,
    pub status_bg: Color,
    pub status_fg: Color,
    pub modal_bg: Color,
    pub highlight_bg: Color,
    pub spinner_fg: Color,
    pub error_fg: Color,
    pub warning_fg: Color,
    pub muted_fg: Color,
    pub prompt_fg: Color,

    // ── Design-system additions (Molten Rust) ────────────────────────────
    /// Primary saturated accent — the theme's "fluid" colour (ember-400).
    pub accent: Color,
    /// Lighter accent for hovers / live text (ember-300).
    pub accent_bright: Color,
    /// Oxidized / dimmed accent for idle borders + scrollbars (ember-700).
    pub accent_dim: Color,
    /// Raised panel fill (one step above canvas).
    pub panel_bg: Color,
    /// Header / status / modal chrome fill.
    pub raised_bg: Color,
    /// Dark ink drawn ON TOP of bright accent/voice badges.
    pub badge_fg: Color,
    /// Foreground for the selected row in lists/pickers.
    pub selection_fg: Color,
    /// Background band for the active (hot) selection.
    pub selection_bg: Color,
    /// Scrollbar thumb / faint chrome rules.
    pub scrollbar_fg: Color,
    /// Captions, disabled, hint text (warmer than muted).
    pub faint_fg: Color,
    /// Frame characters drawn as text (ASCII splash box).
    pub ghost_fg: Color,
    /// "OK / connected" status dot colour (reads warm).
    pub ok_fg: Color,
    /// Subtle hairline divider (quieter than `border_fg`).
    pub border_soft: Color,
}

/// Raw palette in design-token order; `build` maps it onto a [`Theme`].
struct Palette {
    void: u32,
    base: u32,
    panel: u32,
    raised: u32,
    overlay: u32,
    sel: u32,
    sel_hot: u32,
    fg_bright: u32,
    fg_base: u32,
    fg_muted: u32,
    fg_faint: u32,
    fg_ghost: u32,
    line: u32,
    line_hot: u32,
    accent: u32,
    accent_bright: u32,
    accent_dim: u32,
    user: u32,
    ok: u32,
    warning: u32,
    error: u32,
    add_fg: u32,
    add_bg: u32,
    del_fg: u32,
    del_bg: u32,
}

fn build(p: Palette) -> Theme {
    Theme {
        bg: rgb(p.base),
        fg: rgb(p.fg_base),
        user_msg_fg: rgb(p.user),
        assistant_msg_fg: rgb(p.fg_bright),
        tool_name_fg: rgb(p.accent),
        added_fg: rgb(p.add_fg),
        added_bg: rgb(p.add_bg),
        removed_fg: rgb(p.del_fg),
        removed_bg: rgb(p.del_bg),
        border_fg: rgb(p.line_hot),
        header_bg: rgb(p.raised),
        header_fg: rgb(p.fg_bright),
        status_bg: rgb(p.raised),
        status_fg: rgb(p.fg_faint),
        // Lift the modal/overlay surface so panels read as a brighter pane of
        // "glass" floating above the canvas.
        modal_bg: rgb(lighten(p.overlay, 6)),
        highlight_bg: rgb(lighten(p.sel, 4)),
        spinner_fg: rgb(p.accent_bright),
        error_fg: rgb(p.error),
        warning_fg: rgb(p.warning),
        muted_fg: rgb(p.fg_muted),
        prompt_fg: rgb(p.accent_bright),

        // Brighter accent ramp for more vivid gradients/highlights.
        accent: rgb(lighten(p.accent, 10)),
        accent_bright: rgb(lighten(p.accent_bright, 26)),
        accent_dim: rgb(p.accent_dim),
        panel_bg: rgb(lighten(p.panel, 6)),
        raised_bg: rgb(lighten(p.raised, 9)),
        badge_fg: rgb(p.void),
        selection_fg: rgb(p.fg_bright),
        selection_bg: rgb(p.sel_hot),
        scrollbar_fg: rgb(p.line_hot),
        faint_fg: rgb(p.fg_faint),
        ghost_fg: rgb(p.fg_ghost),
        ok_fg: rgb(p.ok),
        border_soft: rgb(p.line),
    }
}

impl Theme {
    /// All available theme names, in `^R` cycle order.
    pub const THEME_NAMES: &'static [&'static str] = &[
        "molten-rust",
        "fluid-green",
        "liquid-blue",
        "glittery-gold",
        "bright-neon",
        "fluid-purple",
    ];

    pub fn from_name(name: &str) -> Self {
        match name.trim().to_lowercase().as_str() {
            "fluid-green" | "green" => Self::fluid_green(),
            "liquid-blue" | "blue" | "nord" => Self::liquid_blue(),
            "glittery-gold" | "gold" | "solarized" | "light" => Self::glittery_gold(),
            "bright-neon" | "neon" => Self::bright_neon(),
            "fluid-purple" | "purple" | "dracula" => Self::fluid_purple(),
            // "molten-rust" | "rust" | "dark" | "default" | anything else
            _ => Self::molten_rust(),
        }
    }

    /// Return the next theme name in the cycle.
    pub fn next_theme_name(current: &str) -> &'static str {
        let idx = Self::THEME_NAMES
            .iter()
            .position(|&n| n == current)
            .unwrap_or(0);
        Self::THEME_NAMES[(idx + 1) % Self::THEME_NAMES.len()]
    }

    // ── Molten Rust — the default. Exact tokens from colors_and_type.css ──
    pub fn molten_rust() -> Self {
        build(Palette {
            void: 0x0B0807,
            base: 0x120D0B,
            panel: 0x1A1310,
            raised: 0x241914,
            overlay: 0x2C1E17,
            sel: 0x3A2619,
            sel_hot: 0x4A2C17,
            fg_bright: 0xF6ECE4,
            fg_base: 0xE2D2C7,
            fg_muted: 0xA88B7B,
            fg_faint: 0x7A6154,
            fg_ghost: 0x54423A,
            line: 0x3A2C24,
            line_hot: 0x6B4632,
            accent: 0xFF5A1F,
            accent_bright: 0xFF7A33,
            accent_dim: 0xB5301A,
            user: 0xE8B04B,
            ok: 0x9DBE5A,
            warning: 0xF4A338,
            error: 0xFF4530,
            add_fg: 0xC2D98A,
            add_bg: 0x1E2410,
            del_fg: 0xFF9472,
            del_bg: 0x2E1410,
        })
    }

    // ── Fluid Green — emerald liquid over green-tinted ash ───────────────
    pub fn fluid_green() -> Self {
        build(Palette {
            void: 0x060A07,
            base: 0x0A120D,
            panel: 0x101A14,
            raised: 0x18261D,
            overlay: 0x1E2C24,
            sel: 0x26382E,
            sel_hot: 0x2E4838,
            fg_bright: 0xE9F7EE,
            fg_base: 0xD2E8DA,
            fg_muted: 0x86A892,
            fg_faint: 0x5A7A66,
            fg_ghost: 0x3E5246,
            line: 0x24362C,
            line_hot: 0x3E6B4E,
            accent: 0x2BE08A,
            accent_bright: 0x5CFFB0,
            accent_dim: 0x17935A,
            user: 0x5CE0C8,
            ok: 0x9DBE5A,
            warning: 0xE8C24B,
            error: 0xFF6B5B,
            add_fg: 0xC2D98A,
            add_bg: 0x14280F,
            del_fg: 0xFF9472,
            del_bg: 0x2E1410,
        })
    }

    // ── Liquid Blue — electric cyan-blue over deep-sea ash ───────────────
    pub fn liquid_blue() -> Self {
        build(Palette {
            void: 0x06080F,
            base: 0x0A0E18,
            panel: 0x101626,
            raised: 0x182236,
            overlay: 0x1E2A40,
            sel: 0x263656,
            sel_hot: 0x2E4068,
            fg_bright: 0xE6EEF9,
            fg_base: 0xCDD9EC,
            fg_muted: 0x8090B0,
            fg_faint: 0x56657E,
            fg_ghost: 0x3C4860,
            line: 0x24304A,
            line_hot: 0x3E5A8C,
            accent: 0x2B8AE0,
            accent_bright: 0x5CB0FF,
            accent_dim: 0x175A93,
            user: 0x4BC8E8,
            ok: 0x7AD89A,
            warning: 0xE8C24B,
            error: 0xFF6B6B,
            add_fg: 0xA6E0A0,
            add_bg: 0x0F2A1A,
            del_fg: 0xFF9472,
            del_bg: 0x2E1414,
        })
    }

    // ── Glittery Gold — molten gold over bronze ash ──────────────────────
    pub fn glittery_gold() -> Self {
        build(Palette {
            void: 0x0C0A05,
            base: 0x14110A,
            panel: 0x1E1810,
            raised: 0x2A2216,
            overlay: 0x32281A,
            sel: 0x40341C,
            sel_hot: 0x50421F,
            fg_bright: 0xFAF2DE,
            fg_base: 0xECDDC0,
            fg_muted: 0xB09A6E,
            fg_faint: 0x7A6840,
            fg_ghost: 0x564732,
            line: 0x3A2F1C,
            line_hot: 0x6B5226,
            accent: 0xE8B04B,
            accent_bright: 0xFFD66B,
            accent_dim: 0xB5832E,
            user: 0xF4D24A,
            ok: 0x9DBE5A,
            warning: 0xF4A338,
            error: 0xFF6B5B,
            add_fg: 0xC2D98A,
            add_bg: 0x1E2410,
            del_fg: 0xFF9472,
            del_bg: 0x2E1410,
        })
    }

    // ── Bright Neon — cyber cyan/magenta over cold slate ─────────────────
    pub fn bright_neon() -> Self {
        build(Palette {
            void: 0x06070C,
            base: 0x0A0C12,
            panel: 0x11141E,
            raised: 0x191F2E,
            overlay: 0x20283A,
            sel: 0x2A3450,
            sel_hot: 0x343E60,
            fg_bright: 0xEAF9FF,
            fg_base: 0xCFE4F2,
            fg_muted: 0x8496B0,
            fg_faint: 0x55647E,
            fg_ghost: 0x3A4760,
            line: 0x222C44,
            line_hot: 0x2E66A0,
            accent: 0x00E5FF,
            accent_bright: 0x6BFFFF,
            accent_dim: 0x0A9CB0,
            user: 0xFF3CAC,
            ok: 0x39FF8B,
            warning: 0xFFD23F,
            error: 0xFF3B6B,
            add_fg: 0x39FF8B,
            add_bg: 0x0C2818,
            del_fg: 0xFF7A8C,
            del_bg: 0x2E1018,
        })
    }

    // ── Fluid Purple — neon violet over plum ash ─────────────────────────
    pub fn fluid_purple() -> Self {
        build(Palette {
            void: 0x09070C,
            base: 0x0F0A14,
            panel: 0x17101E,
            raised: 0x21182C,
            overlay: 0x281E34,
            sel: 0x342344,
            sel_hot: 0x402C54,
            fg_bright: 0xF2E9F9,
            fg_base: 0xDECFEC,
            fg_muted: 0x9C82B0,
            fg_faint: 0x68547E,
            fg_ghost: 0x483A5A,
            line: 0x2E2240,
            line_hot: 0x5A3E8C,
            accent: 0xA24BFF,
            accent_bright: 0xC77CFF,
            accent_dim: 0x6A2BB0,
            user: 0xE84BD0,
            ok: 0x9DBE5A,
            warning: 0xE8C24B,
            error: 0xFF6B6B,
            add_fg: 0xC2D98A,
            add_bg: 0x1E2410,
            del_fg: 0xFF9472,
            del_bg: 0x2E1410,
        })
    }

    // ── Back-compat aliases (old theme names referenced elsewhere) ───────
    pub fn dark() -> Self {
        Self::molten_rust()
    }
    pub fn light() -> Self {
        Self::glittery_gold()
    }
}
