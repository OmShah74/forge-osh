use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub user_msg_fg: Color,
    pub assistant_msg_fg: Color,
    pub tool_name_fg: Color,
    pub added_fg: Color,
    pub removed_fg: Color,
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
}

impl Theme {
    /// All available theme names in cycle order
    pub const THEME_NAMES: &'static [&'static str] = &["dark", "light", "dracula", "nord", "solarized"];

    pub fn from_name(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            "solarized" => Self::solarized(),
            "dracula" => Self::dracula(),
            "nord" => Self::nord(),
            _ => Self::dark(),
        }
    }

    /// Return the next theme name in the cycle
    pub fn next_theme_name(current: &str) -> &'static str {
        let idx = Self::THEME_NAMES.iter().position(|&n| n == current).unwrap_or(0);
        Self::THEME_NAMES[(idx + 1) % Self::THEME_NAMES.len()]
    }

    pub fn dark() -> Self {
        Self {
            bg: Color::Reset,
            fg: Color::White,
            user_msg_fg: Color::Cyan,
            assistant_msg_fg: Color::White,
            tool_name_fg: Color::Yellow,
            added_fg: Color::Green,
            removed_fg: Color::Red,
            border_fg: Color::DarkGray,
            header_bg: Color::Rgb(30, 30, 40),
            header_fg: Color::White,
            status_bg: Color::Rgb(30, 30, 40),
            status_fg: Color::DarkGray,
            modal_bg: Color::Rgb(40, 40, 50),
            highlight_bg: Color::Rgb(50, 50, 70),
            spinner_fg: Color::Cyan,
            error_fg: Color::Red,
            warning_fg: Color::Yellow,
            muted_fg: Color::DarkGray,
            prompt_fg: Color::Green,
        }
    }

    pub fn light() -> Self {
        Self {
            bg: Color::White,
            fg: Color::Black,
            user_msg_fg: Color::Blue,
            assistant_msg_fg: Color::Black,
            tool_name_fg: Color::Rgb(128, 0, 128),
            added_fg: Color::Rgb(0, 128, 0),
            removed_fg: Color::Rgb(200, 0, 0),
            border_fg: Color::Gray,
            header_bg: Color::Rgb(240, 240, 245),
            header_fg: Color::Black,
            status_bg: Color::Rgb(240, 240, 245),
            status_fg: Color::Gray,
            modal_bg: Color::Rgb(250, 250, 255),
            highlight_bg: Color::Rgb(220, 220, 240),
            spinner_fg: Color::Blue,
            error_fg: Color::Red,
            warning_fg: Color::Rgb(200, 150, 0),
            muted_fg: Color::Gray,
            prompt_fg: Color::Blue,
        }
    }

    pub fn solarized() -> Self {
        Self {
            bg: Color::Rgb(0, 43, 54),
            fg: Color::Rgb(131, 148, 150),
            user_msg_fg: Color::Rgb(38, 139, 210),
            assistant_msg_fg: Color::Rgb(131, 148, 150),
            tool_name_fg: Color::Rgb(181, 137, 0),
            added_fg: Color::Rgb(133, 153, 0),
            removed_fg: Color::Rgb(220, 50, 47),
            border_fg: Color::Rgb(88, 110, 117),
            header_bg: Color::Rgb(7, 54, 66),
            header_fg: Color::Rgb(147, 161, 161),
            status_bg: Color::Rgb(7, 54, 66),
            status_fg: Color::Rgb(88, 110, 117),
            modal_bg: Color::Rgb(7, 54, 66),
            highlight_bg: Color::Rgb(0, 54, 66),
            spinner_fg: Color::Rgb(42, 161, 152),
            error_fg: Color::Rgb(220, 50, 47),
            warning_fg: Color::Rgb(203, 75, 22),
            muted_fg: Color::Rgb(88, 110, 117),
            prompt_fg: Color::Rgb(133, 153, 0),
        }
    }

    pub fn dracula() -> Self {
        Self {
            bg: Color::Rgb(40, 42, 54),
            fg: Color::Rgb(248, 248, 242),
            user_msg_fg: Color::Rgb(139, 233, 253),
            assistant_msg_fg: Color::Rgb(248, 248, 242),
            tool_name_fg: Color::Rgb(255, 184, 108),
            added_fg: Color::Rgb(80, 250, 123),
            removed_fg: Color::Rgb(255, 85, 85),
            border_fg: Color::Rgb(98, 114, 164),
            header_bg: Color::Rgb(68, 71, 90),
            header_fg: Color::Rgb(248, 248, 242),
            status_bg: Color::Rgb(68, 71, 90),
            status_fg: Color::Rgb(98, 114, 164),
            modal_bg: Color::Rgb(68, 71, 90),
            highlight_bg: Color::Rgb(68, 71, 90),
            spinner_fg: Color::Rgb(189, 147, 249),
            error_fg: Color::Rgb(255, 85, 85),
            warning_fg: Color::Rgb(255, 184, 108),
            muted_fg: Color::Rgb(98, 114, 164),
            prompt_fg: Color::Rgb(80, 250, 123),
        }
    }

    pub fn nord() -> Self {
        Self {
            bg: Color::Rgb(46, 52, 64),
            fg: Color::Rgb(216, 222, 233),
            user_msg_fg: Color::Rgb(136, 192, 208),
            assistant_msg_fg: Color::Rgb(216, 222, 233),
            tool_name_fg: Color::Rgb(235, 203, 139),
            added_fg: Color::Rgb(163, 190, 140),
            removed_fg: Color::Rgb(191, 97, 106),
            border_fg: Color::Rgb(76, 86, 106),
            header_bg: Color::Rgb(59, 66, 82),
            header_fg: Color::Rgb(216, 222, 233),
            status_bg: Color::Rgb(59, 66, 82),
            status_fg: Color::Rgb(76, 86, 106),
            modal_bg: Color::Rgb(59, 66, 82),
            highlight_bg: Color::Rgb(67, 76, 94),
            spinner_fg: Color::Rgb(129, 161, 193),
            error_fg: Color::Rgb(191, 97, 106),
            warning_fg: Color::Rgb(235, 203, 139),
            muted_fg: Color::Rgb(76, 86, 106),
            prompt_fg: Color::Rgb(163, 190, 140),
        }
    }
}
