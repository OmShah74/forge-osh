//! Tests for src/tui/themes.rs

use forge_agent::tui::themes::Theme;

#[test]
fn theme_names_array() {
    assert!(Theme::THEME_NAMES.contains(&"dark"));
    assert!(Theme::THEME_NAMES.contains(&"light"));
    assert!(Theme::THEME_NAMES.contains(&"solarized"));
    assert!(Theme::THEME_NAMES.contains(&"dracula"));
    assert!(Theme::THEME_NAMES.contains(&"nord"));
}

#[test]
fn next_theme_cycle() {
    assert_eq!(Theme::next_theme_name("dark"), "light");
    assert_eq!(Theme::next_theme_name("light"), "dracula"); // order in THEME_NAMES
    assert_eq!(Theme::next_theme_name("dracula"), "nord");
    assert_eq!(Theme::next_theme_name("nord"), "solarized");
    assert_eq!(Theme::next_theme_name("solarized"), "dark");
}

#[test]
fn fallback_theme_is_dark() {
    let t = Theme::from_name("non-existent-theme-xyz");
    // BG logic
    let dark = Theme::dark();
    assert_eq!(t.bg, dark.bg);
}
