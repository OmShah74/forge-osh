use forge_agent::tui::spinner::*;

#[test]
fn spinner_default_state() {
    let s = SpinnerState::default();
    assert!(!s.active);
    assert_eq!(s.frame, 0);
    assert!(s.message.is_empty());
}

#[test]
fn spinner_start_and_stop() {
    let mut s = SpinnerState::default();
    s.start("thinking".into());
    assert!(s.active);
    assert_eq!(s.message, "thinking");

    s.stop();
    assert!(!s.active);
}

#[test]
fn spinner_tick_advances_frame() {
    let mut s = SpinnerState::default();
    s.start("".into());
    let initial_frame = s.frame;
    s.tick();
    assert_eq!(s.frame, initial_frame + 1);
}

#[test]
fn spinner_display_when_active() {
    let mut s = SpinnerState::default();
    s.start("loading".into());
    let txt = s.display();
    assert!(txt.contains("loading"));
}

#[test]
fn spinner_current_frame_returns_char_when_active() {
    let mut s = SpinnerState::default();
    s.start("".into());
    assert_ne!(s.current_frame(), " ");
}

#[test]
fn spinner_current_frame_returns_space_when_inactive() {
    let s = SpinnerState::default();
    assert_eq!(s.current_frame(), " ");
}
