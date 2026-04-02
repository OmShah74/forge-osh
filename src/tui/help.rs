/// Help overlay content
pub fn help_text() -> &'static str {
    r#"forge-osh Help  (v1.0.1)

SLASH COMMANDS  (type at the prompt and press Enter)
  /help              Show this help screen
  /clear             Clear the conversation display
  /cost              Show token usage and cost
  /model             Open model selector
  /provider          Open provider selector
  /keys              Open API key manager
  /theme [name]      Cycle theme or set by name (dark/light/dracula/nord/solarized)
  /trust             Toggle trust mode (skip permission prompts)
  /compact           Compact conversation history with AI summary (LLM-based)
  /undo              Undo the last file mutation made by the agent
  /new               Start a fresh conversation (clears history and display)
  /save              Save session to disk
  /session           Show session info

GIT COMMANDS
  /commit            Generate AI commit message for staged changes
  /diff [staged]     Show git diff stats (add 'staged' for staged only)
  /export [file.md]  Export full conversation to a Markdown file

SESSION & DIAGNOSTICS
  /status            Full system status (provider, model, context %, cost)
  /doctor            Environment diagnostics (git, shell, API keys, config)
  /resume            List saved sessions for resuming
  /add-dir <path>    Add directory to session working scope

AGENT BEHAVIOUR
  /permissions       View/edit permission rules (auto-allow/deny patterns)
                       /permissions add bash(git *)   — always allow git
                       /permissions deny bash(rm -rf *)
                       /permissions remove <index>
  /effort <1-5>      Set response effort level (1=minimal, 5=maximum)
  /copy              Copy last assistant response to clipboard

  /quit or /exit     Exit forge-osh

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
  /           Start filter search       Esc         Cancel

AGENT TOOLS (used autonomously by the AI)
  todo_write        Write structured task list to .forge-osh/todos.md
  task_create       Create a tracked task in this session
  task_update       Update task status (pending→in_progress→completed)
  task_get          Get task details by ID
  task_list         List all session tasks
  ask_user          Agent pauses to ask you a clarifying question
  enter_plan_mode   Agent proposes a plan before executing
  exit_plan_mode    Agent exits plan mode after plan approval
  search_files      Enhanced grep: context lines, file types, output modes
  bash              Enhanced shell: output truncation, per-command timeout

PERMISSION RULES SYSTEM
  Rules are stored in ~/.forge-osh/permissions.json
  Format: tool_name(pattern)
  Examples:
    bash(git *)           auto-allow all git commands
    bash(cargo *)         auto-allow all cargo commands
    read_file(*)          auto-allow all file reads
    edit_file(/src/*)     auto-allow edits under /src/
    bash(rm -rf *)        auto-deny rm -rf commands

HOOKS SYSTEM  (~/.forge-osh/hooks.json)
  PreToolUse   — fires before each tool call
  PostToolUse  — fires after each tool call
  Stop         — fires when agent finishes
  Example:
  { "PreToolUse": [{ "matcher": "bash", "command": "echo Running: $TOOL_INPUT" }] }

MEMORY SYSTEM (CLAUDE.md)
  forge-osh auto-loads CLAUDE.md files into the system prompt:
  - ./CLAUDE.md          project-level instructions
  - ~/.forge-osh/CLAUDE.md   user-level instructions
  - Parent directories up to home are also checked"#
}
