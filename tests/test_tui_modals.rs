//! State tests for the new modal types: SkillBrowser, HelpState, DetailViewer.
//! Input handling is tested here only at the state-mutation level — the
//! crossterm key dispatch path is exercised manually and not easily unit-tested.

use forge_agent::tui::{DetailViewerState, HelpState, SkillBrowserEntry, SkillBrowserState};
use std::path::PathBuf;

fn mk_entry(name: &str, source: &str) -> SkillBrowserEntry {
    SkillBrowserEntry {
        name: name.into(),
        description: format!("desc of {name}"),
        source: source.into(),
        when_to_use: None,
        execution_mode: "inline".into(),
        allowed_tools: vec![],
        canonical_path: Some(PathBuf::from(format!("/tmp/{name}.md"))),
        body: "body".into(),
    }
}

#[test]
fn skill_browser_nav_clamps_at_ends() {
    let mut b = SkillBrowserState::new(
        vec![mk_entry("a", "project"), mk_entry("b", "user")],
        None,
    );
    assert_eq!(b.selected, 0);
    b.move_up(); // no-op at top
    assert_eq!(b.selected, 0);
    b.move_down();
    assert_eq!(b.selected, 1);
    b.move_down(); // clamp at bottom
    assert_eq!(b.selected, 1);
    b.move_up();
    assert_eq!(b.selected, 0);
}

#[test]
fn skill_browser_empty_entries_is_safe() {
    let mut b = SkillBrowserState::new(vec![], None);
    b.move_up();
    b.move_down();
    assert_eq!(b.selected, 0);
    assert!(b.selected_entry().is_none());
}

#[test]
fn skill_browser_active_tracking() {
    let b = SkillBrowserState::new(
        vec![mk_entry("demo", "project")],
        Some("demo".into()),
    );
    assert_eq!(b.active_skill.as_deref(), Some("demo"));
    assert_eq!(b.selected_entry().unwrap().name, "demo");
}

#[test]
fn help_state_default_starts_at_top() {
    let h = HelpState::default();
    assert_eq!(h.scroll, 0);
}

#[test]
fn detail_viewer_constructor_preserves_title_and_body() {
    let dv = DetailViewerState::new("Title", "line1\nline2");
    assert_eq!(dv.title, "Title");
    assert!(dv.body.contains("line2"));
    assert_eq!(dv.scroll, 0);
}
