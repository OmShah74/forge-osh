/// Help overlay content
pub fn help_text() -> &'static str {
    r#"forge-osh Help

SLASH COMMANDS  (type at the prompt and press Enter)
  /help            Show this help screen
  /clear           Clear the conversation display
  /cost            Show token usage and cost
  /model           Open model selector
  /provider        Open provider selector
  /keys            Open API key manager
  /theme [name]    Cycle theme or set by name (dark/light/dracula/nord/solarized)
  /trust           Toggle trust mode (skip permission prompts)
  /compact         Compact conversation history to free context window
  /save            Save session to disk
  /session         Show session info
  /quit or /exit   Exit forge-osh

KEYBOARD SHORTCUTS

GLOBAL
  Ctrl+C    Cancel / interrupt agent    Esc       Close modal
  Ctrl+D    Exit (when input empty)     Ctrl+L    Clear conversation

INPUT LINE
  Enter          Submit message         Shift+Enter  Insert new line
  Ctrl+A         Move to line start     Ctrl+E       Move to line end
  Ctrl+U         Delete to line start   Ctrl+W       Delete previous word
  Up / Down      Navigate input history

SCROLLING
  Shift+Up/Down   Scroll by 3 lines     PgUp/PgDn    Scroll by 10 lines
  Mouse Wheel     Scroll by 3 lines     Ctrl+Home    Jump to top
  Ctrl+End        Jump to bottom (re-enables auto-scroll)

QUICK ACTIONS
  Ctrl+O    Open model picker           Ctrl+P    Open provider picker
  Ctrl+K    Open API key manager        Ctrl+B    Show token/cost info
  Ctrl+R    Cycle color theme           Ctrl+T    Toggle trust mode
  Ctrl+S    Save session                Ctrl+N    New session
  Ctrl+X    Export session to Markdown

CONFIRMATION DIALOGS  (when agent requests permission)
  Y / Enter   Allow once                N / Esc   Deny
  A           Always allow this tool    T         Enable trust mode

KEY MANAGER  (Ctrl+K)
  Up / Down   Navigate providers        Enter / e   Set or change key
  d / Del     Delete stored key         Esc / q     Close

MODEL/PROVIDER PICKERS  (Ctrl+O / Ctrl+P)
  Up / Down   Navigate items            Enter       Select
  /           Start filter search       Esc         Cancel"#
}
