/// Help overlay content
pub fn help_text() -> &'static str {
    r#"forge-osh Keyboard Shortcuts

GLOBAL
  Ctrl+C    Cancel / interrupt         Esc       Close modal
  Ctrl+D    Exit (empty input)         Ctrl+L    Clear screen

INPUT LINE
  Enter       Submit                   Shift+Enter  New line
  Ctrl+A      Line start               Ctrl+E       Line end
  Ctrl+U      Delete to start          Ctrl+W       Delete word
  Up/Down     Input history

SCROLLING
  Shift+Up/Down  Scroll by 3 lines     PgUp/PgDn   Scroll by 10
  Mouse Wheel    Scroll by 3 lines     Ctrl+Home    Top
  Ctrl+End       Bottom (auto-scroll)

AGENT & SESSION
  Ctrl+M    Model picker               Ctrl+P    Provider picker
  Ctrl+K    API key manager            Ctrl+T    Toggle trust mode
  Ctrl+S    Save session               Ctrl+N    New session
  Ctrl+I    Token/cost info            Ctrl+X    Export session

CONFIRMATION DIALOGS
  Y/Enter   Confirm                    N/Esc     Decline
  A         Always allow               T         Trust mode

KEY MANAGER (Ctrl+K)
  Up/Down   Navigate providers         Enter/e   Set/change key
  d/Del     Delete stored key          Esc/q     Close

PICKERS (model/provider)
  Up/Down  Navigate   Enter  Select   /  Filter   Esc  Cancel"#
}
