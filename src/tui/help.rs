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
  Up/Down     Input history            Tab          Autocomplete

AGENT & SESSION
  Ctrl+M    Model picker               Ctrl+P    Provider picker
  Ctrl+T    Toggle trust mode          Ctrl+S    Save session
  Ctrl+N    New session                Ctrl+I    Token/cost info
  Ctrl+X    Export session             Ctrl+G    Git status

CONVERSATION VIEW
  PgUp/PgDn    Scroll                  Ctrl+Home  Top
  Ctrl+End     Bottom

CONFIRMATION DIALOGS
  Y/Enter   Confirm                    N/Esc     Decline
  A         Always allow               T         Trust mode

DIFF VIEW
  A  Apply    D  Decline    E  Edit in $EDITOR    V  Full

PICKERS (model/provider)
  Up/Down  Navigate   Enter  Select   /  Filter   Esc  Cancel"#
}
