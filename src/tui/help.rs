/// Help overlay content
pub fn help_text() -> &'static str {
    r#"forge-osh Help  (v1.0.17)

SLASH COMMANDS  (type at the prompt and press Enter)
  /help              Show this help screen (scroll: ↑↓/jk, PgUp/PgDn, g/G)
  /clear             Clear the conversation display
  /cost              Show token usage and cost
  /model             Open model selector
  /model list        List available models for current provider
  /model <id>        Switch to model directly by ID or name
  /provider          Open provider selector
  /keys              Open API key manager
  /theme [name]      Cycle theme or set by name (dark/light/dracula/nord/solarized)
  /trust             Toggle trust mode (skip permission prompts)
  /vim               Toggle vim normal mode (j/k scroll, g/G top/bottom, i/a insert)
  /fast              Toggle fast mode (optimized output display)
  /compact           Compact conversation history with AI summary (LLM-based)
  /undo              Undo the last file mutation made by the agent
  /new               Start a fresh conversation (clears history and display)
  /save              Save session to disk
  /session           Show session info
  /sessions          Open session browser (load / delete past sessions)
  /rename [name]     Rename the active session (opens modal if no arg)
  /init              Generate CLAUDE.md project instructions file
  /find <pattern>    Search files (gitignore-aware glob, e.g. /find *.rs)
  /config [key val]  View or set config (theme/trust/vim). E.g. /config theme dark
  /stats             Show detailed session statistics (tokens, tools, context %)
  /skills                   Open the Skills browser modal (list / invoke / edit / new / delete)
                              keys inside:  ↑↓/jk nav · Enter invoke · s show · e edit
                                            n new · d delete · r reload · o off · Esc close
  /skill <name> [args]      Invoke a skill. Inline skills narrow tool access;
                            fork skills run in an isolated worker.
  /skill show <name>        Display a skill's full SKILL.md body
  /skill new <name>         Scaffold a new project skill + open $EDITOR
  /skill generate <name> <task>
                            Draft a project skill from the current conversation;
                            review it, then press Y to create it.
  /skill gen <name> <task>  Alias for /skill generate
  /skill generate-from-conversation <name> <task>
                            Explicit alias for conversation-based skill generation
  /skill edit <name>        Edit an existing skill in $EDITOR (reload after save)
  /skill delete <name>      Remove a project skill directory
  /skill reload             Re-scan skill directories
  /skill path               Print where skills are loaded from
  /skill off                Clear the currently-active skill scope

  Generated skills:
    â€¢ Created from the current conversation using the active provider/model
    â€¢ Previewed in a modal before writing; press Y to create, E to inspect raw
    â€¢ Saved as project skills under ./.claude/skills/generated-<name>/SKILL.md

  Skill locations:
    • Project:  ./.claude/skills/<name>/SKILL.md
    • User:     ~/.forge-osh/skills/<name>/SKILL.md
    • Bundled:  shipped with forge-osh (debug, review, refactor, project-memory)
  Project overrides user overrides bundled.

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

  /forge-graph       Build semantic code graph for current project (enables graph_query tool)
  /forge-graph status     Show graph info (nodes, edges, age)
  /forge-graph rebuild    Force full graph rebuild
  /forge-graph query <n>  Search graph for symbol by name
  /forge-graph clear      Remove artifact and unload graph

  /quit or /exit     Exit forge-osh

KEYBOARD SHORTCUTS

GLOBAL
  Ctrl+C    Cancel / interrupt agent    Esc       Close modal
  Ctrl+D    Exit (when input empty)     Ctrl+L    Clear conversation

INPUT LINE
  Enter          Submit message         Shift+Enter  Insert new line
  Ctrl+A         Move to line start     Ctrl+E       Move to line end
  Ctrl+U         Delete to line start   Ctrl+W       Delete previous word
  Up / Down      Navigate input history Tab          Auto-complete slash command
  Alt+Up/Down    Scroll long input      Ctrl+Up/Down Scroll long input

CLIPBOARD PASTE
  Multiline paste is captured as one input batch when the terminal supports
  bracketed paste. Large paste is token-estimated before insertion; if it may
  overflow the active model context, forge-osh asks before inserting.
  Very large submitted messages are blocked until shortened, compacted, or a
  larger model is selected, so pasted text is not accidentally sent in pieces.

SCROLLING
  Shift+Up/Down   Scroll by 3 lines     PgUp/PgDn    Scroll by 10 lines
  Mouse Wheel     Scroll by 3 lines     Ctrl+Home    Jump to top
  Ctrl+End        Jump to bottom (re-enables auto-scroll)
  Esc             Enter vim normal mode

VIM NORMAL MODE  (Esc to enter, i/a to return to insert)
  j / k           Scroll down/up 3 lines
  d / u           Scroll down/up half page
  g               Jump to top          G     Jump to bottom
  i / a           Return to insert mode

QUICK ACTIONS
  Ctrl+O    Open model picker           Ctrl+P    Open provider picker
  Ctrl+K    Open API key manager        Ctrl+B    Show token/cost info
  Ctrl+R    Cycle color theme           Ctrl+T    Toggle trust mode
  Ctrl+S    Save session                Ctrl+N    New session
  Ctrl+X    Export session to Markdown

CONFIRMATION DIALOGS  (when agent requests permission)
  Y / Enter   Allow once                N / Esc   Deny
  A           Always allow this tool    T         Enable trust mode
  ↑/↓/jk      Scroll long diff preview  PgUp/PgDn Page preview

PATCH / DIFF REVIEW
  When ui.diff_before_apply = true, file mutations show a patch preview before
  execution. Review the unified diff, then allow or deny. This review gate
  overrides accept-edits and stored allow rules for file tools; trust/bypass
  mode is the explicit no-prompt escape hatch.

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
  bash              Shell: read-only commands (ls/cat/grep/git log) skip permission prompts
  powershell        PowerShell shell (Windows): Get-* cmdlets skip permission prompts
  notebook_read     Read Jupyter .ipynb notebooks as formatted cell text

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
