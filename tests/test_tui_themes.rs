//! Tests for src/tui/themes.rs

use forge_agent::tui::themes::Theme;

#[test]
fn theme_names_array() {
    // The 6 "fluid" Molten Rust themes (v1.0.22 recolor).
    assert_eq!(Theme::THEME_NAMES.len(), 6);
    assert!(Theme::THEME_NAMES.contains(&"molten-rust"));
    assert!(Theme::THEME_NAMES.contains(&"fluid-green"));
    assert!(Theme::THEME_NAMES.contains(&"liquid-blue"));
    assert!(Theme::THEME_NAMES.contains(&"glittery-gold"));
    assert!(Theme::THEME_NAMES.contains(&"bright-neon"));
    assert!(Theme::THEME_NAMES.contains(&"fluid-purple"));
}

#[test]
fn next_theme_cycle() {
    // Cycles in THEME_NAMES order, wrapping back to the first.
    assert_eq!(Theme::next_theme_name("molten-rust"), "fluid-green");
    assert_eq!(Theme::next_theme_name("fluid-green"), "liquid-blue");
    assert_eq!(Theme::next_theme_name("liquid-blue"), "glittery-gold");
    assert_eq!(Theme::next_theme_name("glittery-gold"), "bright-neon");
    assert_eq!(Theme::next_theme_name("bright-neon"), "fluid-purple");
    assert_eq!(Theme::next_theme_name("fluid-purple"), "molten-rust");
}

#[test]
fn unknown_current_theme_cycles_from_start() {
    // An unrecognized current theme falls back to index 0 → next is element 1.
    assert_eq!(Theme::next_theme_name("does-not-exist"), "fluid-green");
}

#[test]
fn fallback_theme_is_default() {
    // Unknown names resolve to the default (molten-rust); `dark()` is a
    // back-compat alias for it, so backgrounds must match.
    let t = Theme::from_name("non-existent-theme-xyz");
    assert_eq!(t.bg, Theme::molten_rust().bg);
    assert_eq!(t.bg, Theme::dark().bg);
}

#[test]
fn legacy_names_map_to_fluid_themes() {
    // Old theme names from earlier versions still resolve (back-compat), so
    // existing configs keep working.
    assert_eq!(Theme::from_name("dark").bg, Theme::molten_rust().bg);
    assert_eq!(Theme::from_name("nord").bg, Theme::liquid_blue().bg);
    assert_eq!(Theme::from_name("dracula").bg, Theme::fluid_purple().bg);
    assert_eq!(Theme::from_name("solarized").bg, Theme::glittery_gold().bg);
}
