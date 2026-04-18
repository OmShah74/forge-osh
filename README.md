# 🛠️ forge-osh

<div align="center">
  <h3>The Universal, Provider-Agnostic Coding Agent for the Terminal</h3>
  <p>An autonomous AI coding assistant that works with <strong>any LLM provider</strong> — cloud or local.<br/>
  Built in Rust for speed. Designed for developers who live in the terminal.</p>
  <br/>
  <code>v1.0.8</code> &nbsp;·&nbsp;
  <strong>MIT License</strong> &nbsp;·&nbsp;
  <a href="mailto:omamitshah@gmail.com">Request Binary</a>
</div>

---

## 📑 Table of Contents

1.  [Project Vision](#-project-vision)
2.  [Key Features at a Glance](#-key-features-at-a-glance)
3.  [Tech Stack & Architecture](#-tech-stack--architecture)
4.  [Getting the Application](#-getting-the-application)
5.  [Quick Start Guide](#-quick-start-guide)
6.  [Supported LLM Providers](#-supported-llm-providers)
7.  [Agent Tool Suite (40+ Tools)](#-agent-tool-suite-40-tools)
8.  [The Agentic Loop & Planning](#-the-agentic-loop--planning)
9.  [Terminal User Interface (TUI)](#-terminal-user-interface-tui)
10. [Slash Commands](#-slash-commands)
11. [Keyboard Shortcuts](#-keyboard-shortcuts)
12. [Permission Rules System](#-permission-rules-system)
13. [Hooks System](#-hooks-system)
14. [Memory System (CLAUDE.md)](#-memory-system-claudemd)
15. [Session Management](#-session-management)
16. [Context Window & Token Management](#-context-window--token-management)
17. [File Undo System](#-file-undo-system)
18. [Git Worktree Isolation](#-git-worktree-isolation)
19. [Semantic Code Graph (forge-graph)](#-semantic-code-graph-forge-graph)
20. [CLI Commands Reference](#-cli-commands-reference)
21. [Configuration Reference](#-configuration-reference)
22. [Environment Variables](#-environment-variables)
23. [Future Roadmap](#-future-roadmap)
24. [Contributing](#-contributing)
25. [License & Contact](#-license--contact)

---

## 🎯 Project Vision

**`forge-osh`** was created to give developers a lightning-fast, native AI coding assistant that runs entirely inside the terminal — no Electron apps, no browser tabs, no vendor lock-in.

- **Use Any LLM**: Bring your own keys. Anthropic, OpenAI, Gemini, Groq, xAI, OpenRouter, DeepSeek, or run models locally with Ollama. Switch providers mid-conversation with a single keystroke.
- **True Agentic Autonomy**: The agent doesn't just chat. It reads files, writes code, runs shell commands, manages Git, searches the web, fixes its own errors, and loops until the task is complete.
- **Uncompromised Safety**: Every destructive action (writes, deletes, shell commands) goes through a permission system. Wildcard allow/deny rules persist across sessions so you're never nagged for the same `git commit` twice.
- **Single Binary, Zero Dependencies**: One compiled executable. Works on Windows, macOS, and Linux. No Python, no Node, no Docker required.

---

## ✨ Key Features at a Glance

| Category | Feature |
|---|---|
| **Providers** | 12+ cloud providers, 6 local providers, auto-detection of local models |
| **Tools** | 40+ tools: file I/O, shell, Git (14 ops), search, web, code quality, tasks, notebooks, worktrees |
| **Agent** | Autonomous plan-execute-observe loop with `enter_plan_mode` / `exit_plan_mode` |
| **TUI** | 5 color themes, Vim normal mode, mouse scroll, conversation history, modal pickers |
| **Safety** | Per-tool permission rules with glob patterns, blocked-command lists, trust mode |
| **Sessions** | Auto-save, named sessions, resume, export to Markdown |
| **Context** | LLM-based context compaction, token counting, cost tracking in real-time |
| **Undo** | File snapshot stack — undo any agent mutation instantly with `/undo` |
| **Hooks** | Shell hooks on `PreToolUse`, `PostToolUse`, `Stop`, `Notification` events |
| **Memory** | Auto-loads `CLAUDE.md` files from project, parent dirs, and `~/.forge-osh/` |
| **Code Graph** | `/forge-graph` builds a full semantic code graph — deterministic O(1) symbol lookup for the agent, token-efficient codebase navigation |

---

## 🏗️ Tech Stack & Architecture

| Layer | Technology |
|---|---|
| **Language** | [Rust](https://www.rust-lang.org/) 2021 Edition |
| **Async Runtime** | [Tokio](https://tokio.rs/) (full features) |
| **Terminal UI** | [Ratatui](https://ratatui.rs/) + [Crossterm](https://github.com/crossterm-rs/crossterm) |
| **CLI Parsing** | [Clap](https://docs.rs/clap) v4 with derive macros |
| **HTTP** | [Reqwest](https://docs.rs/reqwest) with Rustls TLS + SSE streaming |
| **Tokenization** | [tiktoken-rs](https://docs.rs/tiktoken-rs) for accurate token counting |
| **Serialization** | [Serde](https://serde.rs/) + JSON + TOML + [Bincode](https://docs.rs/bincode) (graph artifact) |
| **Code Graph** | [Petgraph](https://docs.rs/petgraph) `StableGraph` + [Rayon](https://docs.rs/rayon) parallel parsing |
| **Code Quality** | [Syntect](https://docs.rs/syntect) for syntax highlighting, [Similar](https://docs.rs/similar) for diff generation |
| **Error Handling** | [thiserror](https://docs.rs/thiserror) typed errors + [Anyhow](https://docs.rs/anyhow) |
| **Logging** | [Tracing](https://docs.rs/tracing) with environment filtering |

### Architecture Overview

```
┌──────────────┐   ┌──────────────┐   ┌──────────────────┐
│   CLI/TUI    │──▶│   App Core   │──▶│  Provider Router  │
│  (Ratatui)   │   │  (app.rs)    │   │ (12+ clouds, 6+  │
│              │   │              │   │  local detected)  │
└──────────────┘   └──────┬───────┘   └──────────────────┘
                          │
              ┌───────────┼───────────┐
              │           │           │
      ┌───────┴──┐  ┌─────┴────┐ ┌───┴────────┐
      │  Agent   │  │ Sessions │ │   Config    │
      │  Loop    │  │ History  │ │  Keyring    │
      │ Planner  │  │ Tokens   │ │  Models DB  │
      │ Context  │  │ Checkpt  │ │  Hooks      │
      │ Compact  │  └──────────┘ │  Permissions│
      │ Hooks    │               └─────────────┘
      │ Perms    │
      └───┬──────┘
          │
    ┌─────┴──────────────────────────────────┐
    │           Tool Registry (40+)          │
    ├────────────┬────────────┬──────────────┤
    │ File I/O   │ Git (14)   │ Shell/PS     │
    │ Search     │ Web (2)    │ Code Quality │
    │ Tasks (5)  │ Agent (3)  │ Notebooks    │
    │ Worktree(3)│ graph_query│              │
    └────────────┴────────────┴──────────────┘
          │
    ┌─────┴──────────────────────────────────┐
    │       Semantic Code Graph (opt.)       │
    ├────────────┬────────────┬──────────────┤
    │ petgraph   │ Two-pass   │ Bincode      │
    │ StableGraph│ parallel   │ artifact     │
    │ 3 indices  │ builder    │ persistence  │
    └────────────┴────────────┴──────────────┘
```

---

## 📥 Getting the Application

### Method 1: Request a Pre-Built Binary (Easiest)

If you don't have Rust or Cargo installed and don't want to set them up, simply **email [omamitshah@gmail.com](mailto:omamitshah@gmail.com)** with your operating system (Windows/macOS/Linux) and architecture (x64/ARM). You'll receive a compiled `forge-osh` executable ready to run — no build tools needed.

### Method 2: Download from GitHub Releases

Visit the **[Releases](https://github.com/OmShah74/forge-osh/releases)** page on GitHub and download the pre-compiled archive for your platform:

| Platform | File |
|---|---|
| Windows (x64) | `forge-osh-windows-amd64.zip` |
| macOS (Apple Silicon) | `forge-osh-macos-arm64.tar.gz` |
| macOS (Intel) | `forge-osh-macos-amd64.tar.gz` |
| Linux (x64) | `forge-osh-linux-x86_64.tar.gz` |

Extract the archive and place the binary in a directory on your `PATH`.

### Method 3: Install from Source (via Cargo)

Requires [Rust](https://rustup.rs/) (1.75+).

```bash
git clone https://github.com/OmShah74/forge-osh.git
cd forge-osh
cargo install --path .
```

### Method 4: Build from Source (Custom Target Directory)

```powershell
# Windows (PowerShell)
$env:PATH = "$env:USERPROFILE\.cargo\bin;C:\msys64\mingw64\bin;$env:PATH"
$env:CARGO_TARGET_DIR = "C:\forge-build"
cargo build --release
# Binary → C:\forge-build\release\forge-osh.exe
```
```bash
# Linux / macOS
cargo build --release
# Binary → target/release/forge-osh
```

---

## ⚡ Quick Start Guide

### 1. Set Up an API Key

```bash
# Option A: Interactive first-run setup (guided wizard)
forge-osh

# Option B: Direct CLI key management
forge-osh config keys set anthropic sk-ant-api-xxxxxxxxxxxx

# Option C: Environment variable (ephemeral)
export ANTHROPIC_API_KEY=sk-ant-api-xxxxxxxxxxxx
```

### 2. Launch the Agent

```bash
# Interactive TUI mode
forge-osh

# Non-interactive single-task mode
forge-osh "Fix the null pointer exception in src/handler.rs"

# Pipe mode (feed logs, code, or errors via stdin)
cat build_errors.log | forge-osh "Diagnose and fix these build errors"

# Specify a provider and model for this session
forge-osh -p groq -m llama-3.3-70b-versatile "Refactor the auth module"

# Resume the last session
forge-osh --resume

# Start or resume a named session
forge-osh --session feature-auth-refactor
```

---

## ☁️ Supported LLM Providers

### Cloud Providers (12)

| Provider | Env Variable | Default Model |
|---|---|---|
| **Anthropic** | `ANTHROPIC_API_KEY` | `claude-sonnet-4-20250514` |
| **OpenAI** | `OPENAI_API_KEY` | `gpt-4o` |
| **Google Gemini** | `GEMINI_API_KEY` | `gemini-2.0-flash` |
| **Groq** | `GROQ_API_KEY` | `llama-3.3-70b-versatile` |
| **xAI (Grok)** | `XAI_API_KEY` | `grok-3` |
| **OpenRouter** | `OPENROUTER_API_KEY` | `anthropic/claude-sonnet-4-20250514` |
| **Mistral** | `MISTRAL_API_KEY` | `mistral-large-latest` |
| **DeepSeek** | `DEEPSEEK_API_KEY` | `deepseek-chat` |
| **Together AI** | `TOGETHER_API_KEY` | `meta-llama/Llama-3.3-70B-Instruct-Turbo` |
| **Fireworks** | `FIREWORKS_API_KEY` | `llama-v3p3-70b-instruct` |
| **Perplexity** | `PERPLEXITY_API_KEY` | `sonar-pro` |
| **Cohere** | `COHERE_API_KEY` | `command-r-plus` |

### Local Providers (6) — Auto-Detected

`forge-osh` probes common local ports at startup and automatically adds any running local inference server.

| Provider | Default URL | Auto-detect |
|---|---|---|
| **Ollama** | `http://localhost:11434` | ✅ |
| **LM Studio** | `http://localhost:1234` | ✅ |
| **llama.cpp** | `http://localhost:8080` | ✅ |
| **vLLM** | `http://localhost:8000` | ✅ |
| **Jan** | `http://localhost:1337` | ✅ |
| **LocalAI** | `http://localhost:8080` | ✅ |

---

## 🧰 Agent Tool Suite (40+ Tools)

### File System Operations (8 tools)

| Tool | Permission | Description |
|---|---|---|
| `read_file` | ReadOnly | Read file content with optional line ranges |
| `write_file` | Mutating | Write an entire file (new or overwrite) |
| `edit_file` | Mutating | Surgical find-and-replace edits (preferred over `write_file`) |
| `create_file` | Mutating | Create a new file (errors if exists) |
| `delete_file` | Destructive | Delete a file with confirmation |
| `list_directory` | ReadOnly | List directory contents |
| `move_file` | Mutating | Move or rename files |
| `copy_file` | Mutating | Copy files |

Every mutating file operation automatically **snapshots** the file before modifying it, enabling `/undo`.

### Shell Execution (2 tools)

| Tool | Permission | Description |
|---|---|---|
| `bash` | Varies | Run any shell command. Read-only commands (`ls`, `cat`, `grep`, `git log`) are **auto-allowed**. |
| `powershell` | Varies | Run PowerShell commands (Windows). `Get-*` cmdlets are auto-allowed. |

Configurable timeouts (default: 30s, max: 300s) and a blocked-commands list prevent accidental damage.

### Git Operations (14 tools)

| Tool | Permission | Description |
|---|---|---|
| `git_status` | ReadOnly | Working tree status |
| `git_diff` | ReadOnly | Diff with options (staged, file-specific) |
| `git_log` | ReadOnly | Commit history with formatting |
| `git_blame` | ReadOnly | Line-by-line blame |
| `git_show` | ReadOnly | Show commit contents |
| `git_add` | Mutating | Stage files |
| `git_commit` | Mutating | Create commits |
| `git_branch` | Mutating | Create/list branches |
| `git_checkout` | Mutating | Switch branches |
| `git_stash` | Mutating | Stash changes |
| `git_reset` | Destructive | Reset HEAD |
| `git_fetch` | Network | Fetch from remotes |
| `git_push` | Network | Push to remotes |
| `git_pull` | Network | Pull from remotes |

### Search & Navigation (2 tools)

| Tool | Description |
|---|---|
| `search_files` | Grep-based content search with context lines, file type filters, and output modes |
| `find_files` | Glob-pattern file discovery across the entire project tree |

### Web (2 tools)

| Tool | Description |
|---|---|
| `web_fetch` | Fetch a URL and return content as text (HTML auto-converted to readable text) |
| `web_search` | Search the web via DuckDuckGo — returns titles, URLs, and snippets |

### Code Quality (3 tools)

| Tool | Description |
|---|---|
| `run_linter` | Run the project's linter (auto-detects: ESLint, Clippy, Pylint, etc.) |
| `run_tests` | Run the project's test suite |
| `run_formatter` | Run the project's formatter (Prettier, rustfmt, Black, etc.) |

### Task Management (5 tools)

| Tool | Description |
|---|---|
| `todo_write` | Write a structured TODO list to `.forge-osh/todos.md` with statuses and priorities |
| `task_create` | Create a tracked in-session task |
| `task_update` | Update a task's status (`pending` → `in_progress` → `completed` / `failed`) |
| `task_get` | Retrieve details of a specific task by ID |
| `task_list` | List all tasks in the session, optionally filtered by status |

### Agent Orchestration (3 tools)

| Tool | Description |
|---|---|
| `ask_user` | Pause the agent loop and present a clarifying question to the user |
| `enter_plan_mode` | Switch to planning mode — agent proposes a plan before executing |
| `exit_plan_mode` | Exit planning mode and proceed with execution |

### Jupyter Notebooks (1 tool)

| Tool | Description |
|---|---|
| `notebook_read` | Parse `.ipynb` files and display cells (code, markdown, outputs) as formatted text |

### Git Worktrees (3 tools)

| Tool | Description |
|---|---|
| `enter_worktree` | Create an isolated git worktree for experimental or risky changes |
| `exit_worktree` | Remove a worktree after the experiment concludes |
| `list_worktrees` | List all worktrees, marking which ones were created in this session |

### Semantic Code Graph (1 tool)

| Tool | Permission | Description |
|---|---|---|
| `graph_query` | ReadOnly | Query the pre-built semantic code graph. Returns "no graph loaded" gracefully if no artifact exists. Supports `find`, `context_pack`, `blast_radius`, `file_graph`, `mutations`, and `stats` operations. |

---

## 🔄 The Agentic Loop & Planning

`forge-osh` operates in an autonomous **plan-execute-observe** loop:

1. **Understand** — Read relevant files and context before acting
2. **Plan** — For complex tasks, the agent enters `plan_mode` and presents its strategy
3. **Execute** — Make targeted edits, run commands, verify with tests
4. **Observe** — Check results, fix errors, iterate
5. **Report** — Summarize what was done and flag any issues

The **Planner** module uses heuristics to detect complex tasks (words like "refactor", "migrate", "build", requests longer than 30 words) and auto-enters plan mode.

---

## 🖥️ Terminal User Interface (TUI)

### Layout

The TUI is a full-screen terminal application with four panes:
- **Header Bar**: Shows active model, provider, session name, token count, cost, theme, and trust mode status
- **Conversation View**: Scrollable, syntax-highlighted conversation with user, assistant, and tool messages
- **Input Box**: Multi-line text input with history support
- **Status Bar**: Displays all available keyboard shortcuts and scroll position

### Color Themes (5 built-in)

Cycle themes live with `Ctrl+R` or `/theme [name]`:

| Theme | Description |
|---|---|
| `dark` | Default dark theme |
| `light` | Light background for bright environments |
| `dracula` | Purple-accented Dracula palette |
| `nord` | Cool blue-grey Nord palette |
| `solarized` | Warm Solarized palette |

### Vim Normal Mode

Press `Esc` to enter Vim normal mode for keyboard-only navigation:
- `j` / `k` — Scroll down/up 3 lines
- `d` / `u` — Scroll half-page down/up
- `g` / `G` — Jump to top / bottom
- `i` / `a` — Return to insert mode

---

## 💬 Slash Commands

Type these at the prompt and press Enter:

### General
| Command | Description |
|---|---|
| `/help` | Show the full help overlay |
| `/clear` | Clear the conversation display |
| `/quit`, `/exit` | Exit forge-osh |
| `/new` | Start a fresh conversation |
| `/save` | Save session to disk |
| `/session` | Show current session info |

### Model & Provider
| Command | Description |
|---|---|
| `/model` | Open model selector picker |
| `/model list` | List all available models for the current provider |
| `/model <id>` | Switch to a model directly by ID |
| `/provider` | Open provider selector picker |
| `/keys` | Open the API key manager |

### Agent Control
| Command | Description |
|---|---|
| `/trust` | Toggle trust mode (skip all permission prompts) |
| `/vim` | Toggle Vim normal mode |
| `/fast` | Toggle fast mode (optimized output) |
| `/compact` | Run LLM-based context compaction (summarize old messages) |
| `/undo` | Undo the last file modification made by the agent |
| `/effort <1-5>` | Set response effort level |
| `/copy` | Copy last assistant response to clipboard |
| `/permissions` | View/edit permission rules |

### Git & Export
| Command | Description |
|---|---|
| `/commit` | Generate an AI commit message for staged changes |
| `/diff [staged]` | Show git diff statistics |
| `/export [file.md]` | Export the full conversation to Markdown |

### Diagnostics
| Command | Description |
|---|---|
| `/cost` | Show token usage and cost breakdown |
| `/status` | Full system status (provider, model, context %, cost) |
| `/doctor` | Environment diagnostics (git, shell, API keys, config health) |
| `/resume` | List saved sessions for resuming |

### Semantic Code Graph
| Command | Description |
|---|---|
| `/forge-graph` | Build a semantic code graph for the current project and save as a `.bin` artifact |
| `/forge-graph rebuild` | Force a full graph rebuild (discards existing artifact) |
| `/forge-graph status` | Show graph info: node count, edge count, build time, file count |
| `/forge-graph query <name>` | Search the graph for a symbol by name |
| `/forge-graph clear` | Remove the artifact file and unload the graph from memory |

---

## ⌨️ Keyboard Shortcuts

### Global & Navigation
| Shortcut | Action |
|---|---|
| `Ctrl+C` | Cancel / interrupt agent |
| `Ctrl+D` | Exit (empty input) |
| `Ctrl+L` | Clear conversation |
| `Esc` | Close modal / enter Vim mode |

### Prompt Input
| Shortcut | Action |
|---|---|
| `Enter` | Submit prompt |
| `Shift+Enter` | Insert new line |
| `Ctrl+A` / `Ctrl+E` | Cursor to start / end |
| `Ctrl+U` | Delete to start of line |
| `Ctrl+W` | Delete previous word |
| `Up` / `Down` | Navigate prompt history |

### Quick Actions
| Shortcut | Action |
|---|---|
| `Ctrl+O` | Open **Model Picker** |
| `Ctrl+P` | Open **Provider Picker** |
| `Ctrl+K` | Open **API Key Manager** |
| `Ctrl+B` | Show **Token & Cost Info** |
| `Ctrl+R` | **Cycle Color Theme** |
| `Ctrl+T` | Toggle **Trust Mode** |
| `Ctrl+S` | Save session |
| `Ctrl+N` | New session |
| `Ctrl+X` | Export session |

### Scrolling
| Shortcut | Action |
|---|---|
| `Shift+Up/Down` | Scroll 3 lines |
| `PgUp` / `PgDn` | Scroll 10 lines |
| `Mouse Wheel` | Scroll 3 lines |
| `Ctrl+Home` | Jump to top |
| `Ctrl+End` | Jump to bottom (re-enables auto-scroll) |

### Confirmation Dialogs
| Key | Action |
|---|---|
| `Y` / `Enter` | Allow once |
| `N` / `Esc` | Deny |
| `A` | Always allow (saves as rule) |
| `T` | Enable trust mode |

---

## 🔒 Permission Rules System

`forge-osh` uses a wildcard-based permission rules system stored in `~/.forge-osh/permissions.json`. Rules are persistent across sessions.

### Format

```
tool_name(pattern)
```

### Managing Rules

```
/permissions                          — view all rules
/permissions add bash(git *)          — auto-allow all git commands
/permissions add bash(cargo *)        — auto-allow all cargo commands
/permissions add read_file(*)         — auto-allow all file reads
/permissions add edit_file(/src/*)    — auto-allow edits under /src/
/permissions deny bash(rm -rf *)      — auto-deny rm -rf commands
/permissions remove <index>           — remove a rule by index
```

### Evaluation Order

1. **Deny rules** are checked first (always win)
2. **Allow rules** are checked second
3. If no rule matches → user is prompted
4. **ReadOnly tools** (`read_file`, `list_directory`, `search_files`) never prompt
5. **Trust mode** bypasses all prompts

---

## 🪝 Hooks System

Define shell commands that fire at specific agent lifecycle events. Configure in `~/.forge-osh/hooks.json`:

```json
{
  "PreToolUse": [
    { "matcher": "bash", "command": "echo 'Running: $TOOL_INPUT'" }
  ],
  "PostToolUse": [
    { "matcher": "*", "command": "echo 'Tool $TOOL_NAME done (error=$IS_ERROR)'" }
  ],
  "Stop": [
    { "command": "notify-send 'forge-osh task complete'" }
  ]
}
```

**Environment variables** available in hook commands:
- `TOOL_NAME` — name of the tool (e.g. `bash`)
- `TOOL_INPUT` — JSON-serialized tool input
- `TOOL_OUTPUT` — tool output (`PostToolUse` only)
- `IS_ERROR` — `"1"` if tool errored (`PostToolUse` only)

Each hook has a configurable timeout (default: 10 seconds).

---

## 🧠 Memory System (CLAUDE.md)

`forge-osh` automatically loads `CLAUDE.md` files into the system prompt, giving the agent persistent project knowledge:

| Location | Scope |
|---|---|
| `./CLAUDE.md` | Project-level instructions (coding standards, architecture notes) |
| `~/.forge-osh/CLAUDE.md` | User-level preferences (global across all projects) |
| `~/.claude/CLAUDE.md` | Compatible with Claude Code memory files |
| Parent directories | Checked from working dir up to home |

Write instructions like "Always use TypeScript strict mode" or "Test framework is pytest" and the agent will follow them in every session.

---

## 💾 Session Management

- **Auto-save**: Sessions are automatically saved to `~/.local/share/forge-osh/sessions/`
- **Named sessions**: `forge-osh --session my-feature` creates or resumes a named session
- **Resume**: `forge-osh --resume` picks up the last session
- **Export**: `/export report.md` exports the full conversation to Markdown
- **List & Delete**: `forge-osh sessions list` and `forge-osh sessions delete <id>`

Each session records: provider, model, full message history, timestamps, and token usage.

---

## 📊 Context Window & Token Management

### Real-Time Tracking

The header bar shows live token count and cost. Press `Ctrl+B` or type `/cost` for a detailed breakdown.

### LLM-Based Context Compaction

When the conversation approaches the model's context limit (configurable, default: 80%), `forge-osh` uses the active LLM itself to produce a **dense, lossless summary** of the older messages. This summary replaces the dropped messages as a single context block, preserving:

- Files read, created, modified, or deleted
- Key decisions and reasoning
- Current task state and next steps
- Errors encountered and resolutions
- Important variable names, IDs, and branch names

Trigger manually with `/compact` or let it auto-trigger at the configured threshold.

---

## ↩️ File Undo System

Every time the agent modifies a file (`write_file`, `edit_file`, `create_file`, `delete_file`), a **snapshot** of the original file content is pushed onto a global stack.

- Type `/undo` to immediately restore the last modified file to its previous state.
- If the agent created a new file, `/undo` deletes it.
- If the agent modified an existing file, `/undo` restores the original content byte-for-byte.
- Multiple `/undo` calls walk back through the entire stack.

---

## 🌿 Git Worktree Isolation

When the agent needs to perform risky refactors or experiments, it can create an isolated **git worktree**:

```
Agent: I'll create a worktree for this experimental refactor.
[Tool: enter_worktree] path: .worktree/experiment, branch: forge-worktree-1234
```

The main working tree stays untouched. If the experiment succeeds, changes can be merged. If it fails, the worktree is trivially removed with `exit_worktree`.

---

## 🕸️ Semantic Code Graph (forge-graph)

`forge-osh` v1.0.8 ships with an optional but powerful **semantic code graph** engine. Once built, the agent uses it for deterministic, O(1) symbol lookup instead of spending tokens on file searches.

### How It Works

```
/forge-graph          ← you type this once
       │
       ▼
┌──────────────────────────────────────────────┐
│          Two-Pass Parallel Builder           │
│                                              │
│  Pass 1 (parallel, rayon):                  │
│    For every source file → regex parse       │
│    → collect defs, imports, calls            │
│                                              │
│  Pass 2 (sequential):                        │
│    Insert file nodes + symbol nodes          │
│    Resolve edges (Contains, Calls,           │
│    Imports, MutatesState, …)                 │
└──────────────────┬───────────────────────────┘
                   │
                   ▼
        petgraph StableGraph
    ┌──────────────────────────┐
    │  3 in-memory indices:    │
    │  fqdn_index  (O(1))      │
    │  file_index  (by path)   │
    │  name_index  (by name)   │
    └────────────┬─────────────┘
                 │  bincode serialize
                 ▼
     forge_graph_<dirname>.bin     ← reloaded automatically on next launch
```

### Supported Languages

| Language | Definitions Parsed | Imports | Call Graph |
|---|---|---|---|
| **Rust** | `fn`, `struct`, `enum`, `trait`, `impl`, `macro_rules!`, `mod`, `type`, `static`, `const` | `use` statements | function/method calls |
| **Python** | `def`, `class`, `async def` | `import`, `from ... import` | function calls |
| **JavaScript / TypeScript** | `function`, `class`, `const/let/var` arrows, `interface`, `type`, `enum` | `import`, `require()` | function calls |
| **Go** | `func`, `type struct`, `type interface`, `var`, `const` | `import` blocks | function calls |

### Node & Edge Types

**Node kinds**: `File`, `Module`, `Class`, `Struct`, `Enum`, `EnumVariant`, `Function`, `Method`, `Trait`, `Interface`, `Impl`, `GlobalVar`, `TypeAlias`, `Macro`, `Field`, `ExternalStub`

**Edge types**: `Contains`, `Defines`, `Calls`, `Instantiates`, `Returns`, `ReadsState`, `MutatesState`, `Implements`, `Inherits`, `Imports`, `ExternalDependency`

### graph_query Operations

The agent uses the `graph_query` tool automatically when a graph is loaded:

```jsonc
// Find any symbol by name
{ "operation": "find", "target": "MyStruct" }

// Get full context with transitive dependencies, within a token budget
{ "operation": "context_pack", "target": "src/agent/loop.rs::AgentLoop::run", "token_budget": 4000 }

// Blast radius — what breaks if you change this symbol?
{ "operation": "blast_radius", "target": "src/graph/types.rs::GraphNode" }

// All symbols defined in a file
{ "operation": "file_graph", "target": "src/tui/mod.rs" }

// All mutation points for a variable / field
{ "operation": "mutations", "target": "scroll_top" }

// Graph statistics
{ "operation": "stats" }
```

### Context Pack Algorithm

The `context_pack` operation uses a **token-budget BFS** to intelligently pack context:

1. Start from the primary node (full snippet)
2. BFS outward: callers → callees → containers → implementors
3. For each candidate: include full snippet if budget allows, degrade to `signature_only` otherwise
4. Return structured `PackedContext` with primary node + dependency list + truncation notice

This avoids burning thousands of tokens reading whole files — the agent gets exactly the context it needs.

### Artifact & Persistence

- Artifact: `forge_graph_<sanitized-dirname>.bin` stored next to the forge-osh executable
- Auto-loaded on startup if a matching artifact exists
- Version-stamped (`GRAPH_VERSION = 1`) — stale artifacts from old builds are detected and rejected
- Background build: the TUI remains responsive during graph construction; progress messages stream into the conversation display
- Fully **optional**: if no artifact exists, forge-osh behavior is identical to previous versions

---

## 🐝 Multithread Swarm Architecture & Recent Enhancements (v1.0.10 - v1.0.13+)

Starting from version 1.0.10 through 1.0.13, `forge-osh` received a major series of professional-grade architectural upgrades. These updates focus on context preservation, default model reliability, graceful execution management, and primarily, a completely new optional **Multithreaded Swarm Architecture** inspired by enterprise-grade agent harnesses.

### 1. The Coordinator-Worker Swarm Pattern (v1.0.13)

By default, `forge-osh` operates in a serial, **monolithic loop** — you ask a question, the agent plans, tools execute sequentially, and you get a final answer. While highly reliable, this can be slow for tasks that can be parallelized (e.g., researching three different API endpoints while simultaneously writing boilerplate code).

To solve this, v1.0.13 introduces the **Coordinator-Worker Swarm Architecture**.

#### How to Enable Multithreading
The multithreaded architecture is **100% opt-in** and completely preserves the existing stable monolithic workflow when turned off.
- Type `/multithread` (or `/mt`) in the prompt to toggle the Swarm mode on.
- The UI will explicitly notify you that subsequent prompts prefixed with `@worker` will spawn parallel background agents.
- When toggled off, the application seamlessly reverts to the standard linear execution model without any configuration overhead or restarts required.

#### What is a Worker?
A **Worker** is a self-contained, lightweight LLM execution unit operating on its own dedicated `tokio` asynchronous operating system thread. 
- **Isolated Memory:** Each worker maintains its own completely independent `ConversationHistory`. When a worker searches the web or executes tools, its tool calls and message history do not pollute your main visual conversation thread.
- **Independent Context Windows:** Workers do not share tokens. You can spawn a worker to read a massive 100K-token log file, and it will not consume the token budget of your main chat session snippet.
- **Trust Mode Authorization:** Because workers are authorized by the user via the Coordinator, they automatically run in **Trust Mode**, executing their toolchains without prompting you for `Y/n` confirmations.

#### Spawning and Managing Workers
When multithreading is ON, pinging a worker is as simple as tagging your prompt:
```text
@worker Deep dive into the Albot Video RAG ingestion pipeline and document the extraction logic.
```
Immediately, the Coordinator intercepts this, spawns the worker in the background, and gives control of the input line right back to you. You can immediately continue chatting with the monolithic loop or spawn additional workers:
```text
@worker Find out why the Windows build is complaining about missing MSYS2 dependencies.
@worker Write a python script to parse the nginx error logs in the /scratch directory.
```

The Coordinator manages these parallel threads via a message-passing Event Bus:
- **`⚡ Worker Spawned`**: The TUI notifies you as soon as the background thread spins up.
- **`Worker Tool Signals`**: In real-time (and quietly), you'll see brief indicators (e.g., `[worker-5b2a] running read_file...`) letting you know the background agent is actively working.
- **`✅ Worker Completed`**: Once the worker succeeds, it pushes its final summarized report directly to your main chat view. It also reports its independent token consumption (`Worker tokens: 1240 in / 850 out`).
- **`❌ Worker Failed`**: If a worker runs out of iterations or hits an API error, it gracefully halts and reports the exact failure stack trace to the coordinator without crashing your session.

#### Swarm Control Commands
You have full granular control over the swarm via dedicated commands:
- `/multithread status`: Lists all currently executing workers, their unique `uuid` hashes, and the truncated description of what task they are currently solving.
- `/multithread stop`: Broadcasts an abort signal (via tokio `JoinHandle::abort()`) to gracefully instantly kill all background workers running in the swarm.

---

### 2. Intelligent Context Compaction Rewrite (v1.0.10)

Earlier versions of the agent occasionally ran into truncations where issuing a `/compact` command would blindly strip the context window down to the last 16 messages without understanding semantic relevance. 

v1.0.10 entirely rewrote the Context Compaction engine:
- The system now uses LLM-powered dynamic summarization of historical message chains.
- Instead of raw slicing (which orphaned `ToolCall` and `ToolResult` pairs, leading to API rejection errors), the compaction system strips orphaned IDs automatically via a strict validation pass.
- By configuring `auto_summarize_at` in `config.toml`, the system will proactively trigger this dense summarization protocol seamlessly when your context window usage reaches 80%, guaranteeing you never hit a hard token wall mid-generation.

### 3. Smart Default Overrides & Provider Enhancements (v1.0.11)

To provide the absolute best out-of-the-box experience:
- The default provider router logic has been upgraded to prioritize **OpenAI** with `gpt-4o` if API keys are comprehensively available, superseding older faulty default cascades.
- Full support for the bleeding-edge OpenAI pathways has been integrated, including `gpt-4.1` (and `o1` architectures) for tasks requiring deep reasoning before output.
- Auto-routing now seamlessly falls back downward (e.g., Claude → GPT → Gemini) without hard-crashing if a specific endpoint experiences a timeout or rate-limit violation.

### 4. Graceful Execution Abortion (v1.0.12)

A major UX limitation in monolithic CLI tools is the inability to cancel a long-running generation short of killing the entire process architecture (which destroys unsaved history and token tracking metrics).

- **True `Ctrl+C` Interrupts:** The application now intercepts standard `SIGINT` signals correctly inside the TUI loop. 
- Pressing `Ctrl+C` while the agent is thinking (or spinning on a massive file read block) now gracefully aborts *only* the `tokio` sub-task running the agent loop.
- **Partial Stream Preservation:** Any partially streamed text generated before the abort signal was sent is captured, formatted, and permanently committed to the conversation history alongside a `[Execution cancelled by user]` tag.
- The UI spinner halts instantly, and control of the raw input line is immediately released back to the user without dropping the overall `forge-osh` application.

---

## 🛠️ CLI Commands Reference

```bash
# Configuration & Keys
forge-osh config keys set <provider> <key>   # Set API key
forge-osh config keys list                   # List configured keys
forge-osh config keys remove <provider>      # Remove a key
forge-osh config set <key> <value>           # Set a config value
forge-osh config get <key>                   # Get a config value

# Models & Providers
forge-osh providers list                     # List active providers
forge-osh providers test <provider>          # Test provider connection
forge-osh models list                        # List all available models
forge-osh models list groq                   # List models for a provider
forge-osh models set <provider> <model>      # Set default model

# Sessions
forge-osh sessions list                      # List saved sessions
forge-osh sessions export <id>               # Export session to Markdown
forge-osh sessions delete <name>             # Delete a session
forge-osh --session <name>                   # Start/Resume named session
forge-osh --resume                           # Resume the last session

# Execution Modes
forge-osh "your prompt here"                 # Single-task mode
echo "input" | forge-osh                     # Pipe mode
forge-osh --trust                            # Trust mode (no confirmations)
forge-osh --no-tools                         # Chat-only mode (no tools)
forge-osh --verbose                          # Enable debug logging
forge-osh --no-color                         # Disable all colors
forge-osh --theme dracula                    # Set color theme
```

---

## ⚙️ Configuration Reference

All configuration lives in `~/.forge-osh/config.toml`. Created automatically on first run with sane defaults.

```toml
[general]
theme = "dark"                    # dark | light | solarized | dracula | nord
default_provider = "anthropic"
trust_mode = false                # Skip permission prompts globally
auto_save_sessions = true
max_session_history = 100
verbose = false
system_prompt_extra = ""          # Appended to every system prompt

[agent]
max_tokens = 8192
temperature = 0.7
max_tool_iterations = 50          # Max loop iterations before forced exit
planning_mode = true              # Auto-plan for complex tasks
auto_summarize_at = 0.8           # Context compaction at 80% usage
max_output_per_tool = 50000       # Truncate long tool outputs

[tools.bash]
timeout_seconds = 30
max_timeout_seconds = 300
blocked_commands = ["rm -rf /", "sudo rm -rf /", "mkfs", ":(){:|:&};:"]

[tools.web]
enabled = true
timeout_seconds = 15
max_content_length = 50000

[ui]
show_token_count = true
show_cost = true
show_spinner = true
syntax_highlight = true
diff_before_apply = true          # Show diffs before applying edits
compact_tool_output = true
max_conversation_lines = 1000
```

---

## 🌐 Environment Variables

| Variable | Description |
|---|---|
| `FORGE_PROVIDER` | Override default provider |
| `FORGE_MODEL` | Override default model |
| `FORGE_TRUST` | `1` = trust mode (skip all prompts) |
| `FORGE_THEME` | Override UI theme |
| `FORGE_NO_COLOR` | `1` = disable all colors |
| `FORGE_CONFIG_DIR` | Override config directory (`~/.forge-osh/`) |
| `FORGE_DATA_DIR` | Override data directory (`~/.local/share/forge-osh/`) |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OPENAI_API_KEY` | OpenAI API key |
| `GEMINI_API_KEY` | Google Gemini API key |
| `GROQ_API_KEY` | Groq API key |
| `XAI_API_KEY` | xAI (Grok) API key |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `MISTRAL_API_KEY` | Mistral API key |
| `DEEPSEEK_API_KEY` | DeepSeek API key |
| `TOGETHER_API_KEY` | Together AI API key |
| `FIREWORKS_API_KEY` | Fireworks API key |
| `PERPLEXITY_API_KEY` | Perplexity API key |
| `COHERE_API_KEY` | Cohere API key |

---

## 🔮 Future Roadmap

1. **Advanced Code Generation & Diff Handling**
   - AST-aware code modifications instead of string replacement
   - Interactive unified diff preview before applying changes
   - Multi-file edit transactions with atomic rollback

2. **Token Usage & Context Optimization**
   - ✅ Semantic code graph with context-pack BFS (shipped v1.0.8)
   - Prompt caching integration (Anthropic, OpenAI)
   - Aggressive auto-summarization to reduce cost and latency

3. **Intelligent Checkpoint Structure**
   - Local state-machine checkpointing with timeline branching
   - Visually step back to any successful checkpoint and fork a new path
   - Similar to a localized Git tree for AI task history

4. **Next-Gen TUI Improvements**
   - Split-pane layouts with file preview alongside conversation
   - Floating modal windows and mini-maps for large file context
   - Richer visualization of the agent's internal thought process

5. **Non-Terminal Integrations & IDE Plugins**
   - Native integrations for VS Code, Cursor, and Antigravity as an agentic chat pane
   - Desktop companion application for visual-first workflows
   - REST API server mode for integration with custom tooling

---

## 🤝 Contributing

Contributions are welcome! Please:

1. Open an issue to discuss the change before large PRs
2. Run `cargo fmt` and `cargo clippy` before submitting
3. Add tests for new features (run the suite with `cargo test`)
4. Follow the existing code style and module structure

---

## 📄 License & Contact

This project is licensed under the **MIT License**.

**Author**: Om Shah  
**Email**: [omamitshah@gmail.com](mailto:omamitshah@gmail.com)  
**Repository**: [github.com/OmShah74/forge-osh](https://github.com/OmShah74/forge-osh)

> 💡 **Don't want to build from source?** Email [omamitshah@gmail.com](mailto:omamitshah@gmail.com) with your OS and architecture, and I'll send you a compiled binary directly.
