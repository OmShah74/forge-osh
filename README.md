# 🛠️ forge-osh (Open Source Harness)

<div align="center">
  <h3>The Universal, Provider-Agnostic Coding Agent for the Terminal</h3>
  <p>An autonomous AI coding assistant that works with <strong>any LLM provider</strong> — cloud or local.<br/>
  Built in Rust for speed. Designed for developers who live in the terminal.</p>
  <br/>
  <code>v1.0.19</code> &nbsp;·&nbsp;
  <strong>MIT License</strong> &nbsp;·&nbsp;
  <a href="mailto:omamitshah@gmail.com">Request Binary</a>
</div>

---

<p align="center">
  <img src="media/img1.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img2.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img3.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img8.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img4.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img5.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img6.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img7.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img18.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img17.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img22.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img20.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img19.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img21.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img12.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img13.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img14.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img15.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img16.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img11.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img23.png" width="800" style="margin-bottom: 16px;"><br><br><br>
  <img src="media/img24.png" width="800"><br><br>

</p>

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
20. [LSP Code Intelligence](#-lsp-code-intelligence)
21. [CLI Commands Reference](#-cli-commands-reference)
22. [Configuration Reference](#-configuration-reference)
23. [Environment Variables](#-environment-variables)
24. [v1.0.19 — `/goal` Primitive (Durable, Autonomous, Verifiable Goals)](#-v1019--goal-primitive-durable-autonomous-verifiable-goals)
    - [What `/goal` is and why it matters](#what-goal-is-and-why-it-matters)
    - [Goal Architecture](#goal-architecture)
    - [The Goal Contract — GoalSpec](#the-goal-contract--goalspec)
    - [Verifiers — turning self-report into a contract](#verifiers--turning-self-report-into-a-contract)
    - [Policy — autonomous permission gating](#policy--autonomous-permission-gating)
    - [Budget — turns, wall, tokens (no cost limit)](#budget--turns-wall-tokens-no-cost-limit)
    - [Line protocol — PROGRESS / BLOCKED / CLAIM_DONE](#line-protocol--progress--blocked--claim_done)
    - [The autonomous worker loop](#the-autonomous-worker-loop)
    - [The `/goal` slash command surface](#the-goal-slash-command-surface)
    - [Multi-goal & cold-start resume](#multi-goal--cold-start-resume)
    - [Status-bar indicator](#goal-status-bar-indicator)
    - [On-disk layout](#goal-on-disk-layout)
    - [Examples & worked recipes](#goal-examples--worked-recipes)
    - [Enabling /goal — the feature flag](#enabling-goal--the-feature-flag)
25. [v1.0.18 — MCP (Model Context Protocol) Integration](#-v1018--mcp-model-context-protocol-integration)
    - [What MCP is and why it matters](#what-mcp-is-and-why-it-matters)
    - [Architecture](#mcp-architecture)
    - [Catalog of built-in servers](#mcp-catalog-of-built-in-servers)
    - [Custom servers](#mcp-custom-servers)
    - [Secrets handling](#mcp-secrets-handling)
    - [The `/mcp` manager UI](#the-mcp-manager-ui)
    - [Connection lifecycle & errors](#mcp-connection-lifecycle--errors)
    - [Cross-platform spawn (Windows / macOS / Linux)](#mcp-cross-platform-spawn-windows--macos--linux)
    - [Paste routing inside the MCP modal](#paste-routing-inside-the-mcp-modal)
    - [Authenticated-identity rules for the model](#authenticated-identity-rules-for-the-model)
    - [Examples per service](#mcp-examples-per-service)
    - [Configuration & file layout](#mcp-configuration--file-layout)
26. [v1.0.15 — Architecture & Skills Overhaul](#-v1015--architecture--skills-overhaul)
    - [Permission Modes](#permission-modes-plan--accept-edits--bypass--default)
    - [Extended Thinking](#extended-thinking-thinkingconfig)
    - [Tool Executor Rewrite](#tool-executor-rewrite)
    - [JSON-Schema Input Validation](#json-schema-input-validation)
    - [Cancellation Tokens & Ctrl+C](#cancellation-tokens--ctrlc-semantics)
    - [Tool Concurrency](#tool-concurrency-is_concurrency_safe)
    - [File-State Cache](#file-state-cache-sha-256-fingerprinting)
    - [Tiktoken Token Counting](#tiktoken-token-counting)
    - [Compaction Rewrite](#compaction-rewrite--structured-prompt--scaled-budget)
    - [Expanded Hooks Lifecycle](#expanded-hooks-lifecycle)
    - [Failure Circuit-Breaker](#failure-circuit-breaker)
    - [Fuzzy `--resume` & Session Browser](#fuzzy---resume--session-browser)
    - [Skills Architecture](#skills-architecture-project--user--bundled)
    - [Skills UX — Commands & Status Bar](#skills-ux--commands--status-bar)
    - [How to Use, Add, Modify & Delete Skills](#how-to-use-add-modify--delete-skills)
27. [Future Roadmap](#-future-roadmap)
28. [Contributing](#-contributing)
29. [License & Contact](#-license--contact)

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
| **Input** | Bracketed paste, multiline prompts, long-paste context preflight, overflow warnings |
| **Safety** | Per-tool permission rules with glob patterns, blocked-command lists, trust mode |
| **Sessions** | Auto-save, named sessions, resume, export to Markdown |
| **Context** | LLM-based context compaction, token counting, cost tracking in real-time |
| **Skills** | Bundled, user, and project skills, plus conversation-to-skill generation with review before saving |
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
| `list_directory` | ReadOnly | List directory contents with recursive traversal, path-aware filters, ignore handling, and result limits |
| `move_file` | Mutating | Move or rename files |
| `copy_file` | Mutating | Copy files |

Every mutating file operation automatically **snapshots** the file before modifying it, enabling `/undo`.

### Shell Execution (2 tools)

| Tool | Permission | Description |
|---|---|---|
| `bash` | Varies | Run shell commands such as `rg`, `git`, `cargo`, `ls`, `cat`. Read-only commands are **auto-allowed**. Copied prompt markers like `$ rg ...` are ignored. |
| `powershell` | Varies | Run PowerShell commands/scripts such as `Get-Content`, `$lines=...`, `for(...) { ... }`, `Select-Object`. `Get-*` cmdlets are auto-allowed; copied `$ ` / `PS> ` prompts are ignored. |

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
| `search_files` | Native grep-style content search with regex/fixed-string modes, context lines, path globs, exclude globs, file type filters, hidden/ignored controls, and output modes |
| `find_files` | Glob-pattern file discovery across the project tree; matches file names and relative paths while respecting ignore files by default |

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

### LSP Code Intelligence (7 tools)

| Tool | Permission | Description |
|---|---|---|
| `lsp_diagnostics` | ReadOnly | Compiler-grade errors / warnings / type issues for a source file (rust-analyzer, typescript-language-server, pyright, gopls). |
| `lsp_definition` | ReadOnly | Jump to canonical definition of the symbol at `(line, column)`. |
| `lsp_references` | ReadOnly | Find every usage of a symbol — scope-aware, catches re-exports / trait impls that grep misses. |
| `lsp_hover` | ReadOnly | Type signature and doc-comments for a symbol — the same payload IDEs show on hover. |
| `lsp_document_symbols` | ReadOnly | List all symbols (functions, types, methods) declared in a file with their line ranges. |
| `lsp_workspace_symbols` | ReadOnly | Search the whole workspace for symbols matching a query string. |
| `lsp_rename` | ReadOnly (`dry_run=true`) / Mutating (`dry_run=false`) | Compiler-safe rename across the workspace. Defaults to a preview; flip `dry_run=false` to apply. |

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

### Long Prompt Paste & Input Handling

The input box is designed for both short prompts and large copied context blocks:

- `Enter` submits the prompt immediately.
- `Shift+Enter` inserts a newline for multiline prompts.
- Bracketed terminal paste is captured as one paste event, so multiline clipboard content is inserted into the input box instead of being submitted line-by-line.
- Long pasted text is preserved with its original newlines and Unicode content.
- The input box becomes a scrollable viewport for very large prompts; use `Alt+Up/Down` or `Ctrl+Up/Down` to scroll inside the input without scrolling the conversation pane.
- Very large pasted prompts are intentionally kept out of prompt history to avoid bloating session memory with accidental huge clipboard dumps.

Before inserting or submitting a large non-command prompt, `forge-osh` estimates whether it can fit into the active model's remaining context window. If the paste fits, it is inserted normally. If it is close to the limit, the TUI shows a warning while still allowing the insert. If it is likely to overflow, a confirmation modal lets you cancel or insert anyway. On submit, clearly oversized prompts are blocked before the LLM call so the request does not fail after spending time or tokens.

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
| `/team start <goal>` | Start a durable Agent Team board with parallel subtasks and review |
| `/team status` | Open the scrollable Agent Team task board |
| `/team stop` | Stop team workers and save the board |

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

### Skills
| Command | Description |
|---|---|
| `/skills` | Browse available bundled, user, and project skills |
| `/skill <name> [args]` | Invoke a skill manually with optional arguments |
| `/skill generate <name> <task>` | Generate a project skill from the current conversation using the active model |
| `/skill gen <name> <task>` | Short alias for `/skill generate` |
| `/skill generate-from-conversation <name> <task>` | Explicit alias for conversation-based skill generation |
| `/skill show <name>` | Preview frontmatter and body before invoking or editing |
| `/skill edit <name>` | Open an existing project/user skill in `$EDITOR` |
| `/skill delete <name>` | Delete a project skill directory |
| `/skill reload` | Reload all skill directories |
| `/skill path` | Print the scanned skill locations |
| `/skill off` | Clear the currently active skill scope |

### Semantic Code Graph
| Command | Description |
|---|---|
| `/forge-graph` | Build a semantic code graph for the current project and save as a `.bin` artifact |
| `/forge-graph rebuild` | Force a full graph rebuild (discards existing artifact) |
| `/forge-graph status` | Show graph info: node count, edge count, build time, file count |
| `/forge-graph query <name>` | Search the graph for a symbol by name |
| `/forge-graph clear` | Remove the artifact file and unload the graph from memory |

### LSP Code Intelligence
| Command | Description |
|---|---|
| `/lsp` | Show LSP status — supported languages, which servers are installed, which are running |
| `/lsp status` | Same as `/lsp` |
| `/lsp install` | Install/start detected project language servers into forge-osh's managed cache |
| `/lsp install <lang>` | Install/start one built-in language server, e.g. `typescript`, `python`, `rust`, `go` |
| `/lsp shutdown` | Stop every running language server (they will respawn lazily on next use) |
| `/lsp shutdown <lang>` | Stop a single language server (`rust`, `typescript`, `python`, or `go`) |

### MCP (Model Context Protocol)
| Command | Description |
|---|---|
| `/mcp` | Open the MCP server manager modal (catalog + secrets + connect / disconnect) |
| `/mcp list` | Print a compact text list of all known MCP servers and their status |
| `/mcp reconnect`, `/mcp refresh` | Re-spawn every currently enabled server in the background |

### `/goal` (Durable autonomous objectives — v1.0.19, gated by `[features] goals = true`)
| Command | Description |
|---|---|
| `/goal` | List every live goal — id, state, turns, cost, objective |
| `/goal <objective>` | Spawn a new goal with a one-liner objective |
| `/goal --from <path.toml>` | Spawn from a TOML `GoalSpec` (regenerates id + created_at) |
| `/goal-check [<id>]` | Status card (state, tokens, cost, last checkpoint, files touched, recent progress); never blocks the worker |
| `/goal pause <id>` | Cooperative pause at the next tool boundary |
| `/goal resume <id>` | Re-enter the loop with a continuation message |
| `/goal clear <id>` | Cancel mid-stream, archive to `_archive/`, remove from `index.json` |
| `/goal complete <id>` | Admin override — force `Completed` without running verifiers |
| `/goal verify <id>` | Run verifiers now without changing state |
| `/goal metrics <id>` | Pretty-print full `GoalMetrics` |
| `/goal logs <id> [N]` | Tail the last N lines of `progress.log` (default 50) |
| `/goal budget <id> [--max-turns N] [--max-wall <secs>] [--max-input-tokens N] [--max-output-tokens N]` | Persist new caps to `spec.toml`; worker picks them up at the next turn boundary |

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
| `Alt+Up/Down` | Scroll inside a long prompt |
| `Ctrl+Up/Down` | Scroll inside a long prompt |
| Paste from clipboard | Insert pasted text as one prompt, preserving newlines |

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
| `Up/Down` or `j/k` | Scroll long diff previews |
| `PgUp` / `PgDn` | Page long diff previews |

### Patch / Diff Review

When `ui.diff_before_apply = true`, mutating file tools (`write_file`, `edit_file`, `create_file`, `delete_file`, `copy_file`, `move_file`) show a patch preview before they touch disk. The confirmation modal includes a unified diff or a destructive-operation summary, so you approve the actual proposed change rather than only the tool name.

This review gate intentionally overrides stored allow rules and `accept-edits` for file mutations. `trust` / `bypass` mode remains the explicit no-prompt escape hatch. After approval, normal protections still apply: file-state cache checks can block stale edits, snapshots are taken for `/undo`, and tool output includes the final diff/result.

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

1. **Trust / bypass mode** bypasses prompts intentionally.
2. **Plan mode** blocks mutating tools.
3. **ReadOnly tools** (`read_file`, `list_directory`, `search_files`) never prompt.
4. **Deny rules** block matching mutating or shell tool calls.
5. **Patch / diff review** prompts for file mutations when `ui.diff_before_apply = true`.
6. **Allow rules** skip prompts for matching non-file-review calls.
7. If no rule matches, the user is prompted.

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

### Paste Context Preflight

Large pasted prompts are checked against the same token-counting path used by the rest of the session accounting. The estimate combines the current conversation context, the system prompt and tool overhead, a response reserve, a safety margin, and the new pasted text. This makes the warning reflect the actual prompt that would be sent to the active model rather than only the pasted text in isolation.

If a large paste is near the available context budget, `forge-osh` warns before insertion. If it is likely to overflow, the TUI asks for confirmation before inserting and blocks submission if the final prompt still cannot fit. If auto-compaction is enabled, the latest user message is protected so a freshly pasted request is not summarized away before the model has a chance to answer it.

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

## 🧠 LSP Code Intelligence

`forge-osh` integrates the **Language Server Protocol** so the agent can ask real compilers / type-checkers — not regexes — for definitions, references, diagnostics, and renames. This is the same plumbing that powers VS Code, Neovim, and Helix, wrapped as agent tools so the LLM can use it the way a senior engineer would.

LSP fills the gap that `forge-graph` (parser-based) cannot: live type information, borrow-check errors, scope-aware references, trait-impl resolution, generics, and safe rename. Together they form a two-layer intelligence stack — `forge-graph` for project-wide structure, `lsp_*` for compiler-grade precision.

### Supported Languages

forge-osh ships a broad built-in LSP registry, prefers bundled sidecar servers from `lsp/bin` beside the executable, auto-provisions many built-in servers into its managed data cache when it knows a safe installer command, and also loads user-defined servers from `~/.forge-osh/lsp.toml`. The table below lists the core language set; `/lsp` shows the full live registry, including extra built-ins such as C/C++, Java, C#, PHP, Ruby, Lua, Bash, JSON/YAML, HTML/CSS, Vue, Svelte, Kotlin, Swift, Dart, and Dockerfile.

| Language | Extensions | Servers tried (first found wins) | Project markers |
|---|---|---|---|
| Rust | `.rs` | `rust-analyzer` | `Cargo.toml`, `rust-project.json` |
| TypeScript / JavaScript | `.ts .tsx .js .jsx .mjs .cjs` | `typescript-language-server --stdio` | `package.json`, `tsconfig.json`, `jsconfig.json` |
| Python | `.py .pyi` | `pyright-langserver --stdio`, `pylsp`, `jedi-language-server` | `pyproject.toml`, `setup.py`, `requirements.txt`, `Pipfile` |
| Go | `.go` | `gopls` | `go.mod`, `go.work` |

### Installing the Servers

For built-in languages, forge-osh first looks for release-provided sidecars in `lsp/bin` and `lsp/node/node_modules/.bin` beside the executable. If a sidecar is not present and a known package-manager route exists, forge-osh attempts to install and start the server automatically when it detects matching project files. Node-based servers are installed into forge-osh's own data directory instead of requiring global `npm -g` installs. If a platform/package manager is missing or a language has no safe universal installer, the tool returns a friendly install hint instead of failing your conversation.

```bash
# Rust
rustup component add rust-analyzer

# TypeScript / JavaScript
npm install -g typescript-language-server typescript

# Python (pick one)
pip install pyright            # recommended — fastest
pip install python-lsp-server  # alternative

# Go
go install golang.org/x/tools/gopls@latest
```

Run `/lsp` inside the TUI to open a scrollable status view showing configured languages, installed/running servers, install hints, and custom-server instructions.

### Custom Language Servers

Create `~/.forge-osh/lsp.toml` to add or override language servers without rebuilding:

```toml
[[servers]]
language = "zig"
language_id = "zig"
extensions = ["zig"]
command = "zls"
args = []
root_markers = ["build.zig", ".git"]
install_hint = "Install zls and put it on PATH"
```

### How It Works

```
AgentLoop ─► ToolRegistry ─► lsp_* Tool ─► SharedLspManager
                                              │
                                              ▼
                                    ┌──── Per-language cache ────┐
                                    │  rust → LspClient(stdio)   │
                                    │  ts   → LspClient(stdio)   │
                                    │  py   → LspClient(stdio)   │
                                    └────────────────────────────┘
```

- **Bundled sidecars, managed provisioning, warm-up, and lazy fallback.** At startup, forge-osh scans the project lightly, prefers release-provided sidecars, installs known built-in language servers into its managed cache when missing, and warms detected servers in the background. If a server was not warmed yet, the first `lsp_*` tool use still spawns it lazily.
- **Per-language root detection.** When a server is spawned, forge-osh walks up from the working directory looking for that language's project markers (e.g. `Cargo.toml`) so the server indexes the right workspace.
- **Document sync.** Tools that operate on a file open it via `textDocument/didOpen` (or `didChange` if the on-disk text changed since the last call). You don't manage this — every tool that takes a `path` calls it transparently.
- **Diagnostics cache.** Servers push `publishDiagnostics` asynchronously; forge-osh stores the latest snapshot per file, so `lsp_diagnostics` returns immediately if it's already arrived and otherwise polls briefly (default 2.5s, configurable via `wait_ms`).
- **Post-edit diagnostics.** After successful file writes/edits/copies/moves, forge-osh tries a short LSP diagnostic check for the changed source file and appends the result to the tool output when a server is available.
- **Pure stdio JSON-RPC.** No external `lsp-types` dependency — forge-osh ships its own minimal protocol implementation, which keeps the binary small and forwards-compatible with quirky servers.

### When to Use LSP Tools

Use LSP tools instead of plain text search when you care about **correctness**, not just textual matches:

| You want to… | Use | Why not just `search_files`? |
|---|---|---|
| Verify code still compiles after an edit | `lsp_diagnostics` | Catches type errors, unused imports, borrow-check, missing methods — text search can't. |
| Find every caller of a function | `lsp_references` | Scope-aware. Skips comments, strings, lookalike names in unrelated scopes. Includes re-exports. |
| Jump to a symbol's true definition | `lsp_definition` | Resolves trait impls / generics / re-exports correctly. |
| Read a function's signature and docs | `lsp_hover` | Returns the resolved, fully-qualified type — not a fragile regex over the source. |
| Outline a file | `lsp_document_symbols` | Cheaper than `read_file` when you only need the structure. |
| Search the workspace by symbol name | `lsp_workspace_symbols` | Returns only real declarations, not random text matches. |
| Rename a symbol everywhere | `lsp_rename` | Compiler-safe. Ignores accidental name collisions; updates re-exports automatically. |

### Example Calls (as the agent sees them)

```jsonc
// Did the last edit break the build?
{ "tool": "lsp_diagnostics", "input": { "path": "src/agent/loop.rs" } }

// Where is `AgentLoop::run` defined? (line/column are 1-based)
{ "tool": "lsp_definition", "input": { "path": "src/agent/loop.rs", "line": 142, "column": 9 } }

// Who calls this method?
{ "tool": "lsp_references", "input": { "path": "src/agent/loop.rs", "line": 142, "column": 9 } }

// What's the type of the variable under the cursor?
{ "tool": "lsp_hover", "input": { "path": "src/tui/mod.rs", "line": 380, "column": 12 } }

// Outline a file before reading it.
{ "tool": "lsp_document_symbols", "input": { "path": "src/types.rs" } }

// Workspace search across the Rust crate.
{ "tool": "lsp_workspace_symbols", "input": { "query": "AgentLoop", "language": "rust" } }

// Preview-only rename (default). dry_run=false applies the edits.
{ "tool": "lsp_rename",
  "input": { "path": "src/agent/loop.rs", "line": 142, "column": 9, "new_name": "drive" } }
```

### Why This Improves the System

1. **Fewer "looks fine but doesn't compile" answers.** The agent can now self-check edits with `lsp_diagnostics` before claiming success — the largest source of bad PRs from LLM agents.
2. **Refactors that don't break the build.** `lsp_rename` (preview-first) replaces fragile sed/regex rewrites with a compiler-validated workspace edit set.
3. **Token-efficient navigation.** `lsp_definition` / `lsp_hover` / `lsp_document_symbols` answer most "what does X do?" questions without dumping whole files into context.
4. **Closes the OpenCode / Claude-Code parity gap.** OpenCode shipped LSP integration as a flagship feature; forge-osh now matches that capability and pairs it with the unique forge-graph layer.
5. **Safe by default.** All read-only operations bypass permission prompts; `lsp_rename` defaults to `dry_run=true` and only escalates to `Mutating` permission when the agent explicitly asks to apply edits.

### Caveats & Things to Know

- **Servers must be available before LSP tools can answer.** forge-osh checks bundled sidecars beside the executable, its managed LSP cache, and `PATH`. `/lsp install` can provision languages with known installers; otherwise the relevant `lsp_*` tool returns a friendly install hint and the agent falls back to text search.
- **First request per language is slow.** Server spawn + `initialize` + workspace index can take 2–30 seconds (especially `rust-analyzer` on a cold cargo target dir). Subsequent calls are sub-second.
- **`lsp_diagnostics` may need a longer wait_ms on large projects.** Some servers stream diagnostics over multiple seconds. If you get an empty result, retry with `"wait_ms": 8000`.
- **Line/column are 1-based.** All forge-osh `lsp_*` tools accept human-friendly 1-based coordinates and convert internally — keep that in mind when scripting.
- **Rename applies in-place when `dry_run=false`.** No git commit, no backup. Run inside a clean working tree (or a `/enter_worktree`) so you can `git diff` / `git checkout` if anything looks wrong. Any `Mutating` rename also goes through the standard diff-review and permission flow before touching disk.
- **Multi-file file-rename / create operations from the server are not honoured.** Only `TextEdit`s within existing files are applied — moving / creating files via LSP rename is reserved for a later iteration.
- **One server per language per session.** Switching projects mid-session: run `/lsp shutdown <lang>` so the next call respawns the server against the new workspace root.
- **Servers are killed on process exit.** forge-osh spawns them with `kill_on_drop`, so a hard quit won't leave orphan processes.
- **Conflict-free with `forge-graph`.** Both layers are independent and complementary — you can run with neither, either, or both.

### Diagnostics & Troubleshooting

- `/lsp` — see what's installed and what's running
- `/lsp install` — install/start servers for detected project languages
- `/lsp install typescript` — install/start one built-in language server
- `/lsp shutdown` — kill every server (forces re-init on next use; useful if a server gets wedged)
- `/lsp shutdown rust` — restart only `rust-analyzer`
- Set `RUST_LOG=forge_agent::lsp=debug` in your environment to see protocol traffic in the tracing logs

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

### 1.1 Agent Teams / Parallel Task Boards

For larger tasks, `forge-osh` now adds a durable Agent Team layer on top of the worker runtime. Use it when a request should be split into independent workstreams, reviewed, and integrated cleanly instead of spawning loose background workers.

```text
/team start refactor the auth module; update tests; review regression risk
/team status
/team stop
```

What the team board adds:
- **Coordinator plan:** the goal is converted into explicit subtasks. Semicolon-separated or multiline goals become direct task seeds; otherwise forge creates context-mapping, implementation, and review-oriented subtasks.
- **Shared bus contract:** every team worker receives the same team id, goal, roster, conflict strategy, and artifact-reporting format.
- **Durable status:** the board is saved under the forge data directory as JSON, so task status, results, artifact paths, and recent events are inspectable after the run.
- **Peer review phase:** after worker subtasks finish, a review worker synthesizes outputs, checks conflicts, identifies missing verification, and produces an integration verdict.
- **Conflict handling:** if multiple workers report the same artifact path, the board enters `conflict` instead of pretending the merge was clean.

Use `/team status` to open the scrollable board modal. Use `/multithread` + `@worker` for quick one-off background tasks; use `/team start` for production-grade multi-agent work that needs lifecycle tracking and review.

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

## 🎯 v1.0.19 — `/goal` Primitive (Durable, Autonomous, Verifiable Goals)

Version 1.0.19 lands the **`/goal` primitive** — a way to hand the agent a *durable contract* and walk away. Instead of prompting turn-by-turn, you write down what "done" looks like, submit it once, and a background worker iterates until the goal is verifiably complete, the budget runs out, or you cancel it. Multiple goals run concurrently, each in its own scoped session with its own cost tracker and its own checkpoint trail. Verifiers turn the model's `CLAIM_DONE` self-report into an empirical contract: until shell commands, files, and git state actually satisfy them, the worker keeps going.

> **Bottom line**: durable autonomous loops, atomic checkpointing every 5 tool calls or 60s, multi-goal concurrency from day 1, exact path-glob and shell-allowlist policy enforcement (no user prompts mid-run), shell / file / git verifiers with full stdout-on-failure feedback to the model, cold-start resume after a kill, `/goal --from PLAN.md` spec files, live `/goal budget` adjustments, status-bar indicator, and `/goal-check` that never blocks the worker. Gated behind `[features] goals = true`.

### What `/goal` is and why it matters

A regular prompt asks for the next response. **`/goal` flips that.** You define the stopping condition once, then the agent drives itself toward it. The shift is from *prompting* (you steering every turn) to *assigning* (the agent driving toward a target you defined).

A goal is **not a long prompt**. It is a **durable contract** consisting of:

| Element | Role |
|---|---|
| **Objective** | What to do, in free text. |
| **Stopping condition** | What "done" looks like, in plain English. |
| **Verifiers** | Empirical checks (shell command, file existence, git tree clean…) that prove "done". |
| **Budget** | `max_turns`, `max_wall`, `max_input_tokens`, `max_output_tokens`. **No `max_usd` — cost is observed, never enforced.** |
| **Policy** | Auto-approval rules for tool calls so the worker runs unattended. |
| **Checkpoint trail** | Atomic snapshots so `/goal-check` is cheap and crash-safe. |

The goal stays active until it's achieved, paused, blocked, cleared, or it runs out of budget.

The model is told about the contract through a `## /goal mode` block appended to its system prompt — same agent loop, same provider, same tools, same MCP servers; just a different driver in charge.

### Goal Architecture

The TUI and the goal workers are completely decoupled — they share only an `mpsc::UnboundedSender<(GoalId, GoalEvent)>` for live notifications and the on-disk layout for everything else. **Checking status never touches the running worker.**

```
            ┌──────────────── TUI (src/tui/mod.rs) ────────────────┐
            │  /goal …    /goal-check    /goal pause/resume/clear  │
            │  status bar:  ● 2 goal(s) — 1 running, 1 verifying   │
            └──────┬──────────────────────────────────▲────────────┘
       GoalControl │                                  │ GoalEvent
                   ▼                                  │
        ┌──────────────────── GoalSupervisor ─────────┴───────────┐
        │  registry of active goals (HashMap<GoalId, Handle>)     │
        │  events fan-in, deps injection, respawn from disk       │
        └─────┬──────────────────────────┬──────────────────────┬─┘
              │ spawn                    │ spawn                │
              ▼                          ▼                      ▼
       ┌────────────┐             ┌────────────┐         ┌────────────┐
       │ GoalWorker │  …          │ GoalWorker │   …     │ GoalWorker │
       │ (own loop) │             │ (own loop) │         │ (own loop) │
       └──────┬─────┘             └──────┬─────┘         └──────┬─────┘
              │ uses                     │                       │
              ▼                          ▼                       ▼
   provider + tools + session (each goal has its OWN session, not the user's)
              │                                                  │
              └──────── disk: spec.toml, transcript.jsonl, ──────┘
                       checkpoints/*.json, progress.log, metrics.json
```

**Key invariant: the user's conversation session and each goal's session are different sessions.** This means:

- `/goal-check` doesn't pollute your transcript with progress noise.
- You can keep chatting with the agent while goals run in the background.
- Pausing or clearing a goal doesn't disturb your input cursor.
- Each goal's cost is isolated — you can see exactly what `goal#a3f` cost without grepping through your conversation log.

The source layout, all under `src/agent/goal/`:

| File | Role |
|---|---|
| `mod.rs` | Public types: `GoalId`, `GoalSpec`, `GoalState`, `GoalEvent`, `GoalControl`, `GoalMetrics`, `Verifier`, `Policy`, `Budget`, `AutoApprove`, `Checkpoint`, `StatusSnapshot`, `GoalSummary`. |
| `persistence.rs` | Atomic-write helpers (`tempfile + rename`), `IndexFile`, per-goal directory helpers, checkpoint ring rotation (50 files max), progress.log appender, `archive_goal`. |
| `prompt.rs` | Builds the `## /goal mode` system-prompt block from a `GoalSpec`. Parses streamed text for `PROGRESS:` / `BLOCKED:` / `CLAIM_DONE:` markers. |
| `policy.rs` | `Decision { Allow, Deny(reason) }`. `evaluate_with_args` walks raw JSON args; `evaluate` is the summary-heuristic fallback. |
| `verifier.rs` | Runs Shell / FileExists / FileContains / NoUncommittedFiles / Custom verifiers, captures stdout/stderr/exit, persists report to `verifier_runs/`. |
| `worker.rs` | The autonomous loop: scoped `AgentLoop`, event drain, marker scan, budget enforcement, checkpointing, verification phase. |
| `supervisor.rs` | `GoalSupervisor` — multi-goal registry, `spawn`/`respawn`/`pause`/`resume`/`clear`/`status`/`list`/`set_budget`/`verify_now`/`force_complete`. |
| `resumer.rs` | Cold-start resume: reads `index.json` at TUI boot, respawns every non-terminal goal. |

### The Goal Contract — GoalSpec

The persisted form of a goal lives at `~/.forge-osh/goals/<id>/spec.toml` and round-trips through serde. A `/goal --from path.toml` invocation reads exactly this shape (modulo `id` and `created_at`, which are regenerated):

```toml
[id]
0 = "lws7g-3a5e1f02"            # auto-generated <base36ts>-<hex>

objective = "Migrate src/provider/openai to the new v2 SDK and keep tests green."
stopping_condition = "cargo test --package forge_agent -- openai is green and no uncommitted unrelated files"
created_at = "2026-05-17T14:00:00Z"
workdir = "C:/Users/OM SHAH/Desktop/forge-osh"
seed_files = ["src/provider/openai/mod.rs", "src/provider/openai/stream.rs"]

[[verifiers]]
type = "shell"
cmd = "cargo build --release"
expect_exit = 0

[[verifiers]]
type = "shell"
cmd = "cargo test --package forge_agent -- openai"
expect_exit = 0
expect_stdout_contains = "test result: ok"

[[verifiers]]
type = "no_uncommitted_files"
except = ["target/**", "*.log"]

[budget]
max_turns = 200
max_wall = 14400          # seconds (4h)
max_input_tokens = 800000
# (No max_usd — cost is observed, never enforced.)

[policy]
network = true
auto_approve = "allowed_tools"   # "read_only" | "allowed_tools" | "all"
write_globs = ["src/provider/openai/**", "tests/**"]
deny_globs = [".git/**", "**/keys.json", "**/.env"]
shell_allowlist = [
  "^cargo\\s+(build|check|test|clippy|fmt)\\b",
  "^git\\s+(status|diff|log|add|commit)\\b",
]
```

Sensible defaults (kick in when fields are omitted): `Budget { max_turns = 200, max_wall = 4h }`, `Policy { network = true, auto_approve = AllowedTools, deny_globs = [".git/**", "**/keys.json", "**/.env"], shell_allowlist = [cargo build|check|test|clippy|fmt, git status|diff|log|add|commit, npm test|run build|run lint, pnpm test|build|lint, pytest, ls] }`.

### Verifiers — turning self-report into a contract

When the model emits `CLAIM_DONE:`, the worker does **not** trust it. It transitions to `Verifying`, runs every configured verifier sequentially (they share workdir state — parallel runs would race), and either:

- All pass → state `Completed`, `GoalEvent::Completed { metrics }` emitted, worker exits.
- Any fail → the failure output (exit code + stdout/stderr excerpt per check) is captured into a synthetic user turn — `"Verification failed after your CLAIM_DONE. Fix and re-claim: ✗ tests → exit 101 …"` — and the worker loops with that message as the next prompt. The model fixes and re-claims.

Each verifier has a strict **5-minute wall clock** and is run with `Stdio::piped` so its stdout/stderr are captured (truncated to a 4 KiB excerpt). Every run persists atomically to `~/.forge-osh/goals/<id>/verifier_runs/<iso_ts>.json`.

| Verifier kind | Pass condition | Implementation |
|---|---|---|
| `Shell { cmd, expect_exit, expect_stdout_contains }` | Exit matches `expect_exit` AND (if set) stdout contains the substring | `cmd /C` on Windows, `sh -c` elsewhere, run inside `spec.workdir` |
| `FileExists { path }` | `tokio::fs::metadata(workdir.join(path))` succeeds | tokio fs |
| `FileContains { path, needle }` | File exists and bytes contain `needle` | tokio fs + bytewise scan |
| `NoUncommittedFiles { except }` | `git status --porcelain` filtered by `except` glob list is empty | `git` child process |
| `Custom { name, cmd }` | Exit 0 | shell exec |

**When no verifiers are configured**, the worker trusts the model's `CLAIM_DONE` and transitions to `Completed` directly. This is the safe-by-default behaviour for simple "do X" goals — you only get the verifier contract when you actually write one.

`/goal verify <id>` (while the worker is running) sets a `pending_verify_now` flag; the worker runs verifiers between the current and next turn, emits one `GoalEvent::VerifierResult` per check, persists the report, then restores its prior state. It does **not** flip a Running goal to Completed even if all verifiers pass — it's a diagnostic, not an admin override (use `/goal complete <id>` for that).

### Policy — autonomous permission gating

A goal worker runs in `PermissionMode::Default` (not `Bypass`) — but instead of prompting the user, a **policy responder task** drains every `PermissionRequest` and answers automatically based on the goal's `Policy`. The user is **never** prompted mid-run.

The decision tree:

| Auto-approve level | Effect |
|---|---|
| `ReadOnly` | Only `PermissionLevel::ReadOnly` tools allowed. Everything else denied. |
| `AllowedTools` (default) | Read-only always allowed. Mutating requires every path-typed arg to match `write_globs` (empty = "allow everything in workdir"). Shell requires the command to match a `shell_allowlist` regex. Destructive always denied. Network honors `policy.network`. MCP tools allowed. |
| `All` | Everything allowed (except `deny_globs` paths, which are still hard-denied — `.git/**`, keystore, `.env`). |

Path-glob matching is **exact**: phase-4 plumbed the raw `serde_json::Value` of the tool call into `PermissionRequest::input`, so `policy::evaluate_with_args` walks 12 scalar arg keys (`path`, `file_path`, `filename`, `filepath`, `dir`, `directory`, `target`, `target_file`, `src`, `source`, `dst`, `dest`, `destination`) plus three array keys (`paths`, `files`, `targets`) and matches each through `glob::Pattern`. No heuristic guessing.

Every denial surfaces as a system-message line `policy DENY: <tool> (<summary>) — <reason>`, a `progress.log` entry `POLICY: deny …`, and a `GoalEvent::Progress` event. The model sees the denial in its tool-result stream and is expected to adapt — it never gets to "ask the user" because the user has walked away.

### Budget — turns, wall, tokens (no cost limit)

Tracked at the top of every iteration:

```rust
if turns      >= max_turns      { Block("turns ({max})"); }
if elapsed     > max_wall       { Block("wall ({secs}s)"); }
if in_tokens  >= max_input_tokens  { Block("input tokens ({max})"); }
if out_tokens >= max_output_tokens { Block("output tokens ({max})"); }
```

On any breach: `GoalEvent::BudgetWarn { kind, used, limit }`, state transitions to `Blocked("budget exhausted: …")`, metrics flushed, worker exits. The goal persists; `/goal budget <id> --max-turns 400` raises the cap and `/goal resume <id>` continues from the last checkpoint.

**Cost is recorded but never enforced.** `GoalMetrics::cost_usd` accumulates from the goal session's `CostTracker` and shows up in `/goal metrics` and `/goal-check`. The goal will never auto-stop because of spend — that's a deliberate design decision: you should be able to set a goal running and trust that it won't be killed by a stale budget estimate.

### Line protocol — PROGRESS / BLOCKED / CLAIM_DONE

The model is told (via the system prompt) to emit three classes of markers on their own lines:

| Marker | What the worker does |
|---|---|
| `PROGRESS: <one-line description>` | Appends `PROGRESS: …` to `progress.log`, increments `metrics.progress_lines`, emits `GoalEvent::Progress { line }`. Each PROGRESS line is what `/goal-check` and `/goal logs` show you. |
| `BLOCKED: <reason>` | Cancels the in-flight tool stream, transitions state to `Blocked(reason)`, persists, exits the worker. User must `/goal resume` after addressing the block. |
| `CLAIM_DONE: <one-paragraph summary>` | Triggers the **Verifying** phase. See above. |

Markers must appear at the start of a line (after any leading whitespace). The scanner uses byte offsets in the per-turn text buffer to handle markers that span chunk boundaries — partial lines aren't re-emitted on the next chunk.

### The autonomous worker loop

Per turn:

1. **Drain control signals** (`Pause`/`Resume`/`Clear`/`VerifyNow`/`ForceComplete`/`StatusReq`) non-blockingly.
2. **Run pending verify-now** if requested between turns.
3. **Check budget**. Break to `Blocked` on breach.
4. **Compose user message**: first turn = `initial_user_message(spec)`; verifier-failure follow-up = the failure-feedback message; otherwise = `continuation_message()`.
5. **Fresh cancel token** so `/goal clear` can interrupt mid-stream.
6. **Spawn `AgentLoop::run(message)`** as a background task; **drain its event stream concurrently** with a 200 ms timeout poll.
7. For every chunk: scan accumulated text for protocol markers; on `PROGRESS:` append + emit; on `BLOCKED:` cancel and return `Blocked`; on `CLAIM_DONE:` return `ClaimDone`. For every `ToolStart`, extract path-typed args into the cumulative `files_touched: Vec<PathBuf>`. For every `ToolEnd`, append `TOOL: <name> (ok|error)` to `progress.log` and bump the checkpoint counter.
8. **Checkpoint** every 5 tool calls OR 60 wall-seconds (whichever first). Atomic write of `Checkpoint { at, turn, phase, last_action, files_touched: snapshot, progress_blurb, metrics }`.
9. After the run returns, refresh metrics from the goal session's `CostTracker`, write a per-turn checkpoint, persist `index.json`.
10. Dispatch on outcome:
    - `ClaimDone` → run verifiers; all pass → `Completed`; any fail → continuation override; no verifiers → trust and `Complete`.
    - `Blocked` → state `Blocked(reason)`, exit.
    - `Paused` → park on `Notify`; on wake check for `Cleared`.
    - `Cancelled` → state `Cleared`, exit.
    - `Errored(e)` → state `Blocked("agent loop errored: e")`.
    - `Finished` → no marker yet; loop with continuation.

### The `/goal` slash command surface

All subcommands (gated by `[features] goals = true` for **spawn**; `list/status/control` commands work even with the flag off if goals already exist in `index.json`):

| Command | Effect |
|---|---|
| `/goal` | List every live goal — id, state, turns, cost, objective. |
| `/goal <objective>` | Spawn a new goal with a one-liner objective. Stopping condition defaults to the objective text. |
| `/goal --from <path.toml>` | Spawn from a TOML spec file. Regenerates `id` + `created_at`. |
| `/goal-check [<id>]` | Render a status card (state, objective, stopping condition, tokens, cost, last checkpoint, files touched, last 5 progress lines). Reads from disk — never touches the worker. If exactly one goal is live, the id is optional. |
| `/goal pause <id>` | Cooperative pause at the next tool boundary. |
| `/goal resume <id>` | Re-enter the loop with a continuation message. |
| `/goal clear <id>` | Cancel mid-stream, archive to `_archive/`, remove from `index.json`. |
| `/goal complete <id>` | Admin override — force-mark `Completed` without running verifiers. |
| `/goal verify <id>` | Run verifiers now without changing state. |
| `/goal metrics <id>` | Pretty-print full metrics. |
| `/goal logs <id> [N]` | Tail the last N progress entries from `progress.log` (default 50). |
| `/goal budget <id> [--max-turns N] [--max-wall <secs>] [--max-input-tokens N] [--max-output-tokens N]` | Persist new caps to `spec.toml`. Worker picks them up at the next outer-loop boundary (after a turn finishes). |

**Note on `/goal budget` live-effect**: budget changes write through to `spec.toml` immediately but the in-memory `Arc<GoalSpec>` on the running handle is immutable — caps apply on the next turn after a checkpoint (≤ 60s). For an instant effect, pause, edit budget, resume — the worker re-reads from disk on respawn (cold-start path).

### Multi-goal & cold-start resume

**Multi-goal from day 1.** The supervisor holds `HashMap<GoalId, Arc<GoalHandle>>` — there is no single-active gate. You can run 5 goals concurrently across different workdirs/branches/repos. The "file collision" risk is left to your discretion (scope each goal with tight `write_globs` or run them in `git worktree` branches).

**Crash-safe resume.** If forge-osh is killed while a goal is in `Running` / `Paused` / `Verifying` / `Blocked`, the next launch:

1. Reads `~/.forge-osh/goals/index.json`.
2. For every entry whose `state.is_terminal()` is false, loads its `spec.toml`.
3. Calls `supervisor.respawn(spec, seed_state)` — `Paused` stays paused (user has to `/goal resume`); everything else respawns as `Running`.
4. Surfaces `Resumed N goal(s): <ids>` as a one-shot system message at boot.
5. Re-summarises the status-bar blurb so the indicator reflects the resumed goals.

Metrics are seeded from `metrics.json` on disk (not zero), so token / cost / turn counts carry over.

### Goal status-bar indicator

The TUI chrome shows a one-line summary right after the `🪄 skill` indicator:

```
● 2 goal(s) — 1 running, 1 verifying
● 1 goal(s) — 1 paused
● 3 goal(s) — 2 running, 1 blocked
```

Empty when no live goals → indicator disappears cleanly. The blurb is recomputed on every incoming `GoalEvent` (the event drainer task already runs, so it costs nothing extra) and stored in a `parking_lot::Mutex<String>` so the sync render path doesn't block on tokio.

### Goal on-disk layout

```
~/.forge-osh/goals/
  index.json                 # IndexFile { goals: Vec<GoalSummary> }
  <goal_id>/
    spec.toml                # GoalSpec, human-readable, round-trips through serde
    transcript.jsonl         # append-only message history (separate from user session)
    progress.log             # plain-text, append-only, timestamped
    metrics.json             # rolling counters, atomic-rewrite
    checkpoints/
      latest.json            # pointer
      2026-05-17T14-11-02.347Z.json
      …                      # ring-rotated to 50 files max
    verifier_runs/
      2026-05-17T14-02-11.001Z.json
      …                      # one per /goal verify or post-CLAIM_DONE run
  _archive/
    <goal_id>/…              # `/goal clear` moves the whole dir here
```

Every persisted file is written atomically through `write_atomic(path, bytes)` (tempfile + `fsync` + `rename`), so `/goal-check` reads can race with the worker without ever seeing a torn file.

### Goal examples & worked recipes

**Smoke test — confirm wiring without any LLM cost**:

```text
> /goal hello
Goal lws7g-3a5e1f02 spawned (phase-1 placeholder worker).
Check status:  /goal-check lws7g-3a5e1f02
Pause:         /goal pause lws7g-3a5e1f02
Clear:         /goal clear lws7g-3a5e1f02
```
(With `[features] goals = true`. The full worker calls the LLM — this is just to confirm wiring.)

**A concrete migration goal**:

```text
> /goal --from ./migrations/openai-v2-spec.toml
Goal mws8h-7b2f1c44 spawned from ./migrations/openai-v2-spec.toml — /goal-check mws8h-7b2f1c44
```

**Check status without disturbing the worker**:

```text
> /goal-check mws8h-7b2f1c44
goal#mws8h-7b2f1c44 · running · turns=12
  Objective:  Migrate src/provider/openai to v2 SDK
  Stopping:   `cargo test --package forge_agent -- openai` green
  Tokens:     in 184392 / out 41028   Cost: $0.4127
  Last ckpt:  14:11:02 · running · tool: edit_file
  Files (3): src/provider/openai/mod.rs, src/provider/openai/stream.rs, tests/openai_v2.rs
  Recent progress:
    14:11:02  TOOL: edit_file (ok)
    14:10:54  PROGRESS: extracted SseFrame helper
    14:10:33  TOOL: read_file (ok)
    14:09:12  PROGRESS: starting migration of stream.rs
```

**Raise the budget mid-flight**:

```text
> /goal budget mws8h-7b2f1c44 --max-wall 28800 --max-turns 500
Budget for mws8h-7b2f1c44 updated and persisted to spec.toml — new turn caps: turns=Some(500) wall=Some(28800)s in=… out=…
(Note: the running worker will pick this up on its next outer-loop iteration after a turn boundary.)
```

**Tail the progress log**:

```text
> /goal logs mws8h-7b2f1c44 20
Last 20 progress entries for mws8h-7b2f1c44:
2026-05-17T14:11:02.347Z  PROGRESS: extracted SseFrame helper
2026-05-17T14:11:02.401Z  TOOL: edit_file (ok)
…
```

**Verification failure → model adapts**:

After `CLAIM_DONE:`, suppose `cargo test` fails. The worker emits each verifier result, persists the report, and feeds the model:

```
Verification failed after your CLAIM_DONE. Fix and re-claim:

  ✗ shell `cargo test --package forge_agent -- openai`
    exit 101 (expected 0)
    exit: 101
    stdout:
    ---- openai::stream_parser_handles_partial_chunk stdout ----
    thread '…' panicked at 'assertion failed'
    stderr:
    (empty)

Continue working until every verifier passes, then emit CLAIM_DONE: again.
```

The next turn, the model reads the failure and fixes the test — without you typing a thing.

**Cold-start resume after `Ctrl+C`**:

```text
$ forge-osh
Resumed 2 goal(s): mws8h-7b2f1c44, lws7g-3a5e1f02
● 2 goal(s) — 2 running                                              ↑ status bar
```

### Enabling /goal — the feature flag

`/goal` is gated by an opt-in flag in `~/.forge-osh/config.toml`:

```toml
[features]
goals = true
```

When the flag is `false` (the default), `/goal <objective>` and `/goal --from` print an error explaining how to enable it. The other subcommands (`/goal` list, `/goal-check`, `/goal pause/resume/clear/complete/verify/metrics/logs`, `/goal budget`) work regardless — so if you had goals running with the flag on and later toggled it off, you can still manage them.

The flag mirrors Codex CLI's `[features] goals = true` convention so documentation and screenshots transfer between tools.

---

## 🔌 v1.0.18 — MCP (Model Context Protocol) Integration

Version 1.0.18 adds first-class support for **Model Context Protocol** servers — the open standard from Anthropic for connecting an LLM agent to external tools, APIs, and data sources over JSON-RPC. With one toggle the agent can list your GitHub repos, send Slack messages, query your Linear issues, fetch Gmail threads, search Notion pages, run a sandboxed filesystem, hit your internal API — anything anyone has shipped (or you can ship) as an MCP server.

> **Bottom line**: 50+ pre-wired services in the catalog, a custom-server form for anything not in the catalog, encrypted-at-rest secrets reusing the same store as your API keys, a TUI modal with proper scrolling and search, runtime tool registration into the existing tool registry, and a system-prompt block that teaches the model how to use MCP servers as the *authenticated user* (no more `user:me` query disasters).

---

### What MCP is and why it matters

MCP is a thin JSON-RPC 2.0 protocol that lets an LLM call structured tools exposed by a separate program (the "MCP server"). The server runs as a child process; the agent talks to it over stdin/stdout with line-delimited JSON frames. Each server publishes a list of tools at handshake time — forge-osh registers every one of them into its tool registry under the prefix `mcp__<server>__<tool>`, and from that point on the model can call them exactly like its built-in tools (`read_file`, `bash`, `git_diff`, …).

What this unlocks in one sentence: **the agent stops being a code-and-shell tool and becomes a universal action layer over every cloud service you can authenticate to**, while leaving the existing forge-osh permission system in charge of mutating actions.

---

### MCP Architecture

```
┌───────────────────────────────────────────────────────────────┐
│                       forge-osh process                       │
│                                                               │
│  ┌─────────────┐   register   ┌─────────────────────────────┐ │
│  │  McpManager │─────────────▶│        ToolRegistry         │ │
│  │  (1 per     │              │  (now interior-mutable —    │ │
│  │   session)  │              │   tools added/removed live) │ │
│  └──────┬──────┘              └─────────────┬───────────────┘ │
│         │ spawn child                       │ all_definitions │
│         ▼                                   ▼                 │
│  ┌─────────────┐  ┌──────────┐    ┌──────────────────────┐    │
│  │ StdioTrans- │  │ McpClient│    │   Provider request   │    │
│  │ port (PIPE) │◀▶│ (RPC)    │    │   (every model call) │    │
│  └──────┬──────┘  └──────────┘    └──────────────────────┘    │
│         │ stdin/stdout JSON-RPC                               │
└─────────┼─────────────────────────────────────────────────────┘
          ▼
┌──────────────────────────────────────────────────────┐
│  MCP server child process (npx / uvx / docker / …)   │
│  e.g.  npx -y @modelcontextprotocol/server-github    │
└──────────────────────────────────────────────────────┘
```

Source layout under `src/mcp/`:

| File | Role |
|---|---|
| `protocol.rs` | JSON-RPC 2.0 envelopes — `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcNotification`, `InboundFrame`, `InitializeParams`, `CallToolResult`, `ContentBlock::flatten()` |
| `transport.rs` | `StdioTransport::spawn()` — child process, piped stdin/stdout/stderr, async response correlation (`HashMap<i64, oneshot::Sender>`), stderr ring buffer (200 lines), per-request timeout |
| `client.rs` | `McpClient::connect_stdio()` — performs initialize handshake → `notifications/initialized` → exposes `tools/list`, `tools/call` |
| `catalog.rs` | 50+ catalog entries: `CatalogEntry { id, display_name, description, category, command, args, secret_specs }` + `SecretSpec` |
| `tool_adapter.rs` | `McpTool` — `impl Tool` so MCP tools satisfy the same trait as native ones; local name `mcp__<server>__<tool>`; schema validation; delegates to `McpClient` |
| `manager.rs` | Lifecycle owner — `load_from_config`, `connect_all_enabled`, `connect`, `disconnect`, `set_enabled`, `save_secret`, `delete_secret`, `snapshot`, `add_custom_server`, `remove_custom_server`, `export_to_config` |
| `mod.rs` | Public API re-exports |

Everything is async (Tokio). The TUI never blocks on a connect — connections run in background tasks that push success/error back into a shared `mcp_status_msgs: Arc<Mutex<VecDeque<String>>>` queue that the main loop drains each tick and surfaces as system messages.

---

### MCP catalog of built-in servers

50+ entries shipped in `src/mcp/catalog.rs`. Each is one constant — no code change is needed to enable one, just toggle it in the modal. Categories include:

- **Dev platforms** — GitHub, GitLab, Git (local repo), Docker
- **Communication** — Slack, Discord, Gmail
- **Productivity** — Linear, Notion, Confluence, Jira, Asana, Trello
- **Cloud / infra** — AWS, Cloudflare, Google Drive, Google Calendar, Google Maps
- **Data** — Postgres, MySQL, SQLite, ClickHouse, DuckDB, Elasticsearch, Neo4j, Pinecone, Chroma
- **Search & retrieval** — Brave Search, DuckDuckGo Search, Exa Search, Tavily, Perplexity
- **Web** — Fetch (generic HTTP-to-markdown), Browserbase, Puppeteer, Apify
- **Files** — Filesystem (sandboxed), Memory (k/v scratchpad)
- **Media / creative** — Figma, EverArt (image gen), Spotify
- **News / data** — Hacker News, Reddit
- **Many more** — see `/mcp` in the running app for the live list with descriptions

For every entry the catalog declares:
- The exact command + args used to spawn the server (almost always `npx -y <pkg>` for Node servers and `uvx <pkg>` for Python ones)
- The set of secrets the server needs (e.g. `GITHUB_PERSONAL_ACCESS_TOKEN`) along with the human-readable label and a help string pointing at where to create the token

---

### MCP custom servers

Anything not in the catalog can be added through the **Add Custom MCP Server** form (`n` from the list view). The form takes:

| Field | Notes |
|---|---|
| ID (slug) | Lowercase ascii + `-`/`_`; must not collide with a catalog id |
| Display name | What appears in the modal |
| Description | One-line summary shown in the list |
| Category | Free-text; defaults to `Custom` |
| Command | Binary to run (e.g. `npx`, `uvx`, `docker`, or an absolute path) |
| Args | Space-separated arguments — passed verbatim |
| Required secret env vars | Comma-separated names (e.g. `MYCORP_TOKEN, MYCORP_REGION`); each one is collected from the user, encrypted under `mcp:<id>:<KEY>`, and exported into the child's environment at spawn |
| Enabled on save | If checked, the manager connects immediately |

The custom server persists to `~/.forge-osh/config.toml` under `[[mcp.servers]]` and is restored on every launch. Tip line in the form: *"command + args are split on spaces; each named secret is stored encrypted-at-rest under `mcp:<id>:<KEY>` and exposed to the server process as an env var."*

---

### MCP secrets handling

MCP secrets use the **same encryption-at-rest store as your provider API keys** — `~/.forge-osh/keys.json`. They are namespaced with the prefix `mcp:<server_id>:<KEY>` so a single keystore file can hold:

- `anthropic` → your Anthropic API key
- `openai` → your OpenAI API key
- `mcp:github:GITHUB_PERSONAL_ACCESS_TOKEN`
- `mcp:slack:SLACK_BOT_TOKEN`
- `mcp:gmail:GMAIL_OAUTH_TOKEN`
- `mcp:<custom>:<KEY>` …

Resolution priority when spawning a server (`manager.rs`):

1. **Environment variable** with the bare name (e.g. `$GITHUB_PERSONAL_ACCESS_TOKEN`) — useful for CI / one-shot use
2. **Stored value** under `mcp:<id>:<KEY>` in the encrypted keystore

If a required secret is missing on connect attempt, the server is marked `Error: missing required secrets: …`, no child process is spawned, and the status appears in the modal.

Secrets never appear in logs, are masked in the modal UI (`[saved]` / `[MISSING (required)]` / `[from env]`), and the SecretInput view masks the value while you're typing it.

---

### The `/mcp` manager UI

Open with `/mcp`. Four views, each rendered by `src/tui/renderer.rs`:

**List view** — every catalog entry + every custom server, sorted by display name. Each row shows:
- Status pill: `[enabled]` / `[disabled]` (background color follows your theme)
- Live status word: `active` / `connecting…` / `disconnected` / `error: <reason>`
- Tool count: `tools=N` (filled only after handshake succeeds)
- Secrets state: `secrets=N/M` (filled out of needed; `need!` if any required are missing)
- One-line description

Keybindings (List view):

| Key | Action |
|---|---|
| `↑ / k`, `↓ / j` | Move selection |
| `PgUp / PgDn` | Page navigation |
| `Home / End` | First / last |
| `Space` or `t` | Toggle enabled (auto-connects on enable) |
| `c` | Connect (auto-enables + connects if needed) |
| `x` | Disconnect (also unregisters the server's tools from the registry) |
| `Enter` / `→` / `l` / `Tab` | Open Detail view |
| `n` | Open the **Add Custom MCP Server** form |
| `D` (capital) | Delete a custom server (catalog entries can never be deleted) |
| `r` | Refresh / reconnect all enabled |
| `Esc` / `q` | Close modal |

**Detail view** — per-server breakdown. Shows server ID, category, tool count, version returned by the handshake, full description, and the secrets table. The last 5 lines of the server's stderr are rendered live so you can see exactly what went wrong with a failing spawn (e.g. `npm warn deprecated`, missing scopes on a token, port conflicts).

Detail keys: `Enter` / `e` on a secret row opens **SecretInput**; `d` / `Delete` clears a stored secret; `Space` / `t` toggle; `c` / `x` connect / disconnect; `←` / `h` / `Esc` back to List.

**SecretInput view** — single-line masked input. Type or paste your token, press `Enter` to save (writes to `~/.forge-osh/keys.json`), `Esc` to cancel.

**CustomForm view** — the eight-field form described above. `Tab` / `↑↓` move between fields; `Ctrl+S` saves; `Esc` cancels.

---

### MCP connection lifecycle & errors

When you press `c` or toggle a server on:

1. Manager calls `set_enabled(id, true)` and persists `enabled=true` to `config.toml`
2. Manager builds the secret env-var map (env var first, then keystore)
3. If any required secret is missing → status `Error: missing required secrets: …`, return
4. `StdioTransport::spawn()` starts the child process. On Windows non-`.exe` commands are routed through `cmd /C` so PATHEXT resolves `npx.cmd` and similar
5. `McpClient::initialize` handshake (with the 45-second connect timeout)
6. `tools/list` — every returned tool is wrapped in an `McpTool` adapter and inserted into the shared `ToolRegistry`
7. Status flips to `Active`, tool count updates, the system-message bar in the main TUI logs `MCP: '<id>' connected — N tool(s) registered.`

Failure modes that get reported back as system messages:

- `MCP: '<id>' connect failed: spawn failed: <program>: program not found` — binary isn't on PATH (install Node / `uvx` / Docker)
- `MCP: '<id>' connect failed: tools/list failed: …` — handshake succeeded but the server crashed mid-discovery
- `MCP: '<id>' connect failed: timed out waiting for response` — server hung for >45 s during initialize
- `MCP: '<id>' connect failed: missing required secrets: GITHUB_PERSONAL_ACCESS_TOKEN`

All non-fatal warnings from the server (e.g. `npm warn deprecated …`) stream into the stderr ring buffer and are visible in the Detail view's "Recent stderr (last 5 lines)" block.

---

### MCP cross-platform spawn (Windows / macOS / Linux)

`StdioTransport::spawn()` is platform-aware:

| OS | Behaviour |
|---|---|
| **macOS / Linux** | `Command::new(program).args(args).spawn()` — direct exec |
| **Windows** | If `program` is a non-absolute path **and** does not end in `.exe`, the call is rewritten as `cmd /C <program> <args…>` so that `PATHEXT` is honoured (resolves `npx.cmd`, `uvx.exe`, `docker.cmd`, custom `.bat` shims). For absolute paths or `.exe`, it spawns directly. `CREATE_NO_WINDOW` is set so no console flashes on Windows. |

This single change makes `npx -y @modelcontextprotocol/server-github` work on Windows, Windows Terminal, WezTerm, cmd.exe, and macOS / Linux terminals, with no per-server configuration.

---

### Paste routing inside the MCP modal

Special handling — because the modal binds single-letter shortcuts (`h` back, `n` new, `t` toggle, `c` connect, `x` disconnect, `D` delete-custom, `q` close) — a token pasted at the wrong time would otherwise trigger destructive shortcuts one keypress at a time.

The TUI does two things:

1. **Burst detection** — `collect_fallback_paste_burst` (in `src/tui/mod.rs`) treats any rapid keystroke burst as a paste and assembles it. On Windows legacy terminals where bracketed-paste support is unreliable, an enlarged 120 ms initial grace + 120 ms quiet-gap window catches even slowly-arriving pasted characters before they leak into the key handler. On macOS / Linux / Windows Terminal / WezTerm, `Event::Paste` fires natively and the burst path is never entered, so there is no added latency.
2. **Modal-aware routing** — once a paste is collected, `handle_clipboard_paste_text` looks at the active view:
   - **SecretInput** → text is appended to the secret buffer
   - **CustomForm** → text is appended to the currently focused text field
   - **List / Detail** → paste is ignored with a hint ("press Enter on a secret row to open the input field first") so a stray paste never opens the new-server form or toggles a random server

---

### Authenticated-identity rules for the model

A connected MCP server runs with **your** credentials. Without help the model treats every tool as a generic public API — it asks for your username, then pastes English words like "me" into search qualifiers, and a search like `user:me` happens to match a real GitHub account named "me" (owned by someone else). To prevent this class of failure, two things happen at runtime:

**1. Every MCP tool's description is rewritten at registration time** with an authentication-context tag, generic across every server:

> `[mcp:<id>] (authenticated as the user's own account on this service — do NOT pass placeholder values like USERNAME / OWNER / ME / YOUR_TOKEN; the server already knows the user's identity from its credential.) <original description>`

**2. The system prompt gains an "MCP Servers" block** whenever at least one MCP tool is registered. Five numbered rules:

| Rule | Principle |
|---|---|
| 1 | First-person words (`my`, `me`, `mine`, `I`, `myself`) refer to the credential-holder, never to a literal field value |
| 2 | Prefer the no-argument authenticated-user tool over any `search_*` tool when the user refers to their own data |
| 3 | Concrete worked anti-pattern showing `query: "user:me"` returning the wrong user, with ❌ wrong vs ✅ right side-by-side |
| 4 | Never substitute literal placeholders (`USERNAME`, `OWNER`, `EMAIL`, `YOUR_TOKEN`); use an authenticated-user tool or `ask_user` instead. A 422 Validation Error almost always means a placeholder was passed |
| 5 | Don't fall back to `web_search` / `web_fetch` for user data when a connected MCP server covers the domain |

The block is generated from the live registry — every connected server (built-in or custom) is listed with its tool count and prefix, and the rules apply uniformly. None of this is server-specific code.

---

### MCP examples per service

What the rules mean in practice. **None of these are hardcoded** — the model learns to do this from the per-tool tag + system-prompt rules:

| You say | ❌ Wrong (the trap) | ✅ Right |
|---|---|---|
| "list **my** repos" | `mcp__github__search_repositories` with `query: "user:me"` | `mcp__github__list_repositories_for_authenticated_user` (no args) |
| "what's in **my** inbox" | `mcp__gmail__search_messages` with `from:me` | `mcp__gmail__list_messages` (the OAuth token = you) |
| "**my** Slack DMs" | `mcp__slack__search_messages` with `user:me` | `mcp__slack__conversations_list` with `types: im` |
| "**my** Linear issues" | `mcp__linear__search_issues` with `assignee:me` | `mcp__linear__list_my_issues` / viewer.assignedIssues |
| "today on **my** calendar" | `mcp__google_calendar__search_events` with `attendee:me` | `mcp__google_calendar__list_events` with `calendarId: primary` |
| "**my** GitLab projects" | `mcp__gitlab__search_projects` with `owner:me` | `mcp__gitlab__list_projects` with `membership: true` |
| "**my** Notion pages" | `mcp__notion__search` with `created_by:me` | `mcp__notion__users_me` then list-by-owner |

Same wording covers Confluence, Jira, Asana, Trello, Reddit, Spotify, Drive, and any custom server you wire up.

---

### MCP configuration & file layout

Per-user files (created on first use):

| Path | Purpose |
|---|---|
| `~/.forge-osh/config.toml` | TOML config; the `[mcp]` section persists every server's `enabled` flag plus any custom entries (id, display_name, description, category, command, args, secret_specs) |
| `~/.forge-osh/keys.json` | Encrypted-at-rest secret store; MCP secrets live under `mcp:<server_id>:<KEY>` keys |

Example `[mcp]` block in `config.toml` after enabling GitHub and adding a custom server:

```toml
[[mcp.servers]]
id = "github"
enabled = true
command = ""   # empty — falls back to the catalog command so package updates flow in automatically
args = []
secret_specs = []

[[mcp.servers]]
id = "mycorp-tools"
enabled = true
display_name = "MyCorp Internal Tools"
description = "Internal RAG over the design docs"
category = "Custom"
command = "uvx"
args = ["mycorp-mcp-server", "--region", "eu-west-1"]

  [[mcp.servers.secret_specs]]
  key = "MYCORP_TOKEN"
  label = "MYCORP_TOKEN"
  help = "Custom secret — set as env var 'MYCORP_TOKEN' for the server"
  required = true
```

The TUI never edits `config.toml` directly; it builds a complete new `[mcp.servers]` list from the live manager state and writes it back via `Config::load_raw` + section replacement, so all your other settings stay untouched on every change.

---

## 🧬 v1.0.15 — Architecture & Skills Overhaul

Version 1.0.15 is the largest single-release hardening pass the codebase has seen: a top-to-bottom re-plumbing of the tool execution pipeline, a rewrite of compaction, a new permission-mode system, and a brand-new Skills subsystem that lets you bundle, share, invoke, and author reusable workflows the agent can call by name. Everything below is shipped and live in the binary — each subsection names the files touched so you can read the code directly.

> **Why this matters**: every earlier release assumed "one agent, one loop, one tool at a time, trust the LLM to validate its own inputs, keep a rough char-count for tokens." Those assumptions were all replaced with real primitives — concurrency-safe tools, JSON-schema validation, precise BPE token counting, a file-state cache that detects external edits, first-class permission modes, extended thinking, cancellation tokens, and structured compaction. The Skills system then sits on top of all of this.

---

### Permission Modes (`Plan` / `AcceptEdits` / `Bypass` / `Default`)

Previously the harness had a single boolean `trust_mode`. That binary choice is now a four-state machine modelled after Claude Code's permission model.

| Mode | Effect |
|---|---|
| `Default` | ReadOnly tools auto-allow. Everything else prompts — or is auto-allowed by a persistent `PermissionStore` rule. |
| `Plan` | **ReadOnly tools only.** Every mutating/shell/network/destructive tool is hard-denied with an explanatory message. The agent is also told to exit plan mode via `exit_plan_mode` when it has a plan to present. |
| `AcceptEdits` | File mutations (`write_file` / `edit_file` / `create_file`) auto-approve. Destructive, Shell, and Network tools still prompt. Perfect for guided refactor sessions. |
| `Bypass` | All tools auto-approve. Same effect as the legacy `trust_mode`. |

**How to switch**:
```
/mode <plan|accept-edits|bypass|default>
/plan             # shortcut for /mode plan
/accept-edits     # shortcut
/bypass           # shortcut
/default          # shortcut
```

The current mode is embedded in the system prompt so the LLM itself understands the restrictions (e.g., under `Plan` it is told "You may ONLY use ReadOnly tools").

**Files**: `src/types.rs` (`PermissionMode` enum), `src/tools/executor.rs` (`decide_permission` layered logic), `src/agent/loop.rs` (system-prompt injection), `src/tui/mod.rs` (slash commands).

---

### Extended Thinking (`ThinkingConfig`)

Anthropic's extended-thinking API is now a first-class knob.

```
/think              # toggle thinking on/off
/think 8000         # set a token budget (integer)
```

`ChatRequest` carries a `thinking: ThinkingConfig` field that flows through the provider layer. The Anthropic provider wires it into the API call (`thinking: { type: "enabled", budget_tokens: N }`) and forces `temperature = 1.0` as required. Non-Anthropic providers ignore it.

**Files**: `src/types.rs` (`ThinkingConfig`), `src/provider/anthropic.rs`.

---

### Tool Executor Rewrite

The tool executor went from a thin dispatch helper into a real gate. It now performs five distinct stages per tool call:

1. **Cancellation check** — honours the agent-level `CancellationToken` before it starts expensive work.
2. **JSON-schema validation** — validates `ToolCall.input` against the tool's declared `parameters_schema()` via the `jsonschema` crate (Draft-7). Bad inputs short-circuit with a readable error instead of reaching the tool.
3. **Effective permission level** — asks the tool for its permission level given the specific input (e.g. `bash("ls")` is less dangerous than `bash("rm -rf /")`).
4. **Layered permission decision** — `Bypass > Skill-scope > Plan > ReadOnly > PermissionStore > AcceptEdits > Ask` (see next section).
5. **Structured-span tracing** — every execution is wrapped in a `tracing::instrument` span so you get end-to-end tool telemetry when tracing is enabled.

**Files**: `src/tools/executor.rs`, `src/tools/validate.rs`.

---

### JSON-Schema Input Validation

A new `tools/validate.rs` module uses the `jsonschema` crate with explicit Draft-7 validation. The error list is capped at 5 entries so a broken input produces a concise diagnostic. If the schema itself fails to compile (a tool authoring bug), validation returns `Ok` rather than blocking real work — a graceful-degradation posture.

This catches entire classes of provider hallucinations (e.g. an LLM inventing a `files` argument that was never in the schema) **before** the tool touches disk.

---

### Cancellation Tokens & Ctrl+C Semantics

`AgentLoop` now holds an `Arc<parking_lot::RwLock<CancellationToken>>`. Ctrl+C in the TUI does three things atomically:

1. Calls `.read().cancel()` on the current token to abort in-flight provider streams, tool executions, and backoff sleeps.
2. Emits an "aborted" event to the TUI.
3. Installs a fresh token via `.write()` so the next turn isn't born cancelled.

Every provider call, tool execution, and backoff-sleep is wrapped in `tokio::select!` against the token — no more hung turns when the user changes their mind.

**Files**: `src/agent/loop.rs` (`cancel_current_turn`, `reset_cancel`, `cancel_token`), `src/tui/mod.rs` (Ctrl+C handler).

---

### Tool Concurrency (`is_concurrency_safe`)

Previously, if the LLM returned four `read_file` calls in one assistant turn, they ran strictly serially. Now:

```rust
fn is_concurrency_safe(&self) -> bool { false }  // default — opt-in
```

`ReadFileTool`, `ListDirectoryTool`, `task_get`, `task_list` and other side-effect-free tools opt in. The loop partitions tool calls into `safe` (run via `join_all`) and `serial` (run sequentially, preserving ordering for the LLM). Result order is preserved so the LLM sees the tool_results in the order it requested them.

**Files**: `src/tools/mod.rs` (trait default), `src/tools/fs.rs`, `src/tools/tasks.rs`, `src/agent/loop.rs` (`execute_tool_calls` partition).

---

### File-State Cache (SHA-256 fingerprinting)

A new `FileStateCache` at `src/session/file_cache.rs` maintains SHA-256 fingerprints + mtime for every file the agent reads. Before a write/edit, the cache checks that the on-disk content still matches the last read — if not, the mutation is refused with a message telling the agent to re-read before editing. This eliminates the entire class of "agent overwrote your manual edit" bugs.

- `record_read(path, content)` — called by `read_file` on success
- `check_unchanged(path)` — called by `write_file` / `edit_file` before mutating
- `record_write(path, content)` — called after a successful mutation
- `invalidate(path)` — called by `delete_file`

The cache is session-scoped (`Arc<FileStateCache>` on `AgentLoop`) and threaded through `ToolContext.file_cache`.

---

### Tiktoken Token Counting

The previous estimator was `chars / 4`. It was inaccurate enough that cost numbers were unreliable and the auto-compact trigger fired either too early or too late. Replaced with `tiktoken_rs::cl100k_base` behind a `OnceLock` so the BPE tables are built once per process.

- `count_tokens(text)` — precise BPE counting
- `count_request(messages, system)` — pre-flight request-size estimate used by the context manager

**Files**: `src/session/tokens.rs`.

---

### Compaction Rewrite — Structured Prompt + Scaled Budget

Three specific auto-compact failures were all fixed together:

1. **"Invalid model" errors after `/model` switch** — compaction used to read `session.model_id`, which was set at session creation and never updated. Auto-compact now always reads `router.active_model_id()` and `router.active()` so the user-visible provider is used.
2. **Two-to-three-line summaries that erased context** — the summarizer prompt is now a mandatory 7-section structure (Context & Goal, Files Touched, Key Decisions, Commands Run, Errors & Resolutions, Identifiers, Current State & Next Step). Target length is scaled to the transcript size (`target_min_words`/`target_max_words` clamped to `[250, 1500]` / `[400, 3500]`). If the transcript is >2000 chars and the summary comes back <80 words or <400 chars, it's rejected with an error so the fallback-truncation path can run.
3. **TUI showed "removed N messages" but didn't delete them** — a new `AgentEvent::HistoryCompacted { kept, removed, summary_preview, succeeded }` event bridges the loop-level compaction to the TUI. The TUI now drains its rendered message pane and refreshes the context-% bar immediately.

`DEFAULT_KEEP_LAST = 0` (no 16-message minimum) so the summarizer sees the full transcript. `max_tokens_budget` scales from 1500 to 8000 based on transcript size.

**Files**: `src/agent/compaction.rs`, `src/agent/loop.rs`, `src/tui/mod.rs` (`drain_rendered_for_compaction`, `refresh_context_display`), `src/session/history.rs`.

---

### Expanded Hooks Lifecycle

The hooks system now fires on seven events (was three):

| Event | Fires when |
|---|---|
| `PreToolUse` | Before every tool call. Can veto by returning non-zero exit. |
| `PostToolUse` | After every tool call. Observes output. |
| `Stop` | When the agent's turn ends normally. |
| `Notification` | When a permission prompt is raised. |
| `UserPromptSubmit` | **NEW** — Before the user's prompt is submitted. Can veto the entire turn. |
| `SessionStart` | **NEW** — When a session begins (fresh or resumed). |
| `SessionEnd` | **NEW** — When a session exits. |
| `PreCompact` | **NEW** — Before auto-compaction runs. Useful for external archiving. |

A `blocking` field was added to `HookEntry`: when `true`, the hook's exit status gates the event. Non-blocking hooks run fire-and-forget. `stdout`/`stderr` are captured so hook veto reasons can be surfaced in the TUI.

**Files**: `src/agent/hooks.rs`.

---

### Failure Circuit-Breaker

If the same tool fails three times in a row on the same input (typically `edit_file` on a file whose contents drifted), the loop injects a circuit-breaker message instructing the agent to read the current file contents and use `write_file` to replace the whole thing. Breaks the "retry the same broken edit forever" pattern that kills turns on long sessions.

**Files**: `src/agent/loop.rs` (`ConsecutiveFailureTracker`).

---

### Fuzzy `--resume` & Session Browser

```
forge-osh --resume                 # load most recent
forge-osh --resume <uuid>          # exact session id
forge-osh --resume <prefix>        # id prefix match
forge-osh --resume <name>          # exact name match
```

Implemented as a three-tier match in `src/app.rs`: exact id → id prefix → name. The `/resume` slash command opens an interactive session browser that lets you navigate sessions with arrow keys and load/delete via Enter/Del.

**Files**: `src/cli.rs` (`resume: Option<String>` with `default_missing_value = "__latest__"`), `src/app.rs`.

---

## Skills Architecture (project / user / bundled)

Skills are **reusable named workflows** — markdown files with YAML frontmatter that tell the agent *how* to do something (debug a regression, review a diff, refactor safely, capture project memory). When a skill is invoked, its body becomes a materialized prompt injected into the conversation, and the session enters a **skill scope** that can narrow which tools are allowed for the duration.

### Three sources, clear precedence

Skills are discovered from three locations, with strict precedence:

| Source | Location | Precedence |
|---|---|---|
| **Project** | `./.claude/skills/<name>/SKILL.md` | Highest — overrides everything |
| **User** | `~/.forge-osh/skills/<name>/SKILL.md` | Middle — personal skills across projects |
| **Bundled** | Compiled into the binary (`src/skills/bundled/*.md`) | Lowest — safe fallbacks |

A project skill named `review` fully replaces the bundled `review` skill — no merging. This lets teams check project-specific skills into `./.claude/skills/` alongside the code.

### The four bundled skills

Ships with the binary, always available, can be overridden by creating a project/user skill with the same name:

- **`debug`** — Structured debugging workflow for reproducing issues and isolating root causes
- **`review`** — Review changes for bugs, regressions, and missing tests before shipping
- **`refactor`** — Plan and apply safe refactors while preserving behavior and verifying each step
- **`project-memory`** — Capture durable project guidance into `CLAUDE.md` without overwriting existing intent

### Skill file format (SKILL.md)

```markdown
---
name: deploy-helper
description: Shown in /skills and in the agent's system prompt
when_to_use: Triggers the agent looks for before invoking
allowed_tools:
  - read_file
  - bash
  - git_diff
model: claude-sonnet-4-6        # optional: override the conversation model
execution_mode: inline            # inline (default) or fork
user_invocable: true              # false hides from /skills picker
hooks:                            # optional per-skill hooks
  PreToolUse:
    - matcher: bash
      command: echo "running in deploy-helper scope"
  Stop:
    - matcher: "*"
      command: notify-send "deploy helper finished"
---

# deploy-helper

Describe the workflow here. This body becomes the materialized prompt injected
when the skill is invoked. Use ${ARGS} to reference any arguments passed as
`/skill deploy-helper <args>`. Use ${FORGE_SESSION_ID} and ${FORGE_SKILL_DIR}
for session/skill introspection.
```

### Execution modes

- **`inline`** (default) — The skill body is injected into the *current* conversation. The session enters a skill scope: tools outside `allowed_tools` are hard-denied at the executor layer (`executor.rs::decide_permission` stage 1.5). Scope clears when the agent's turn ends.

- **`fork`** — The skill runs in an isolated `Worker` (separate conversation history, separate system prompt). The worker's result is spliced back into the main conversation as a `[Skill Result: <name>]` message. The main loop is **not** blocked — the worker is spawned via `tokio::spawn`.

### How the LLM discovers skills

When `agent.skills_enabled = true` and `agent.include_skills_in_system_prompt = true` (both default), the system prompt contains:

```
## Skills
The following skills are available. When one matches the task, use the
`invoke_skill` tool instead of re-inventing the workflow.
- `debug` — Structured debugging workflow... Use when: the user needs bug diagnosis.
- `review` — Review changes for bugs... Use when: the user asks for a review.
...
```

The LLM calls `invoke_skill { skill: "debug", args: "null pointer in parser" }`. The tool returns the materialized prompt as its content (so the LLM receives the full workflow text). In inline mode, the loop also installs a scope that narrows tool access. In fork mode, a worker is spawned.

### Skill scope enforcement

When an inline skill is active, the permission decision order becomes:

1. **Bypass mode** → always allow
2. **Skill scope allowlist** → deny tools not in `allowed_tools`
3. **Plan mode** → allow only ReadOnly
4. ReadOnly tools → allow
5. `PermissionStore` rule → allow / deny
6. **AcceptEdits** → auto-allow Mutating tools
7. Otherwise → ask

An empty `allowed_tools: []` is treated as "no restriction." A non-empty list narrows enforcement.

### Internal types

| Type | Role |
|---|---|
| `SkillDefinition` | Parsed skill (frontmatter + body + source + path) |
| `SkillRegistry` | In-memory collection of parsed skills |
| `SharedSkillRegistry` | `Arc<RwLock<SkillRegistry>>` — hot-reloadable, thread-safe |
| `ActiveSkillScope` | Current scope (allowlist + model override + hooks + mode) on the `Session` |
| `SkillInvocationRecord` | Persisted history of invocations on the session (capped at 32) |
| `InvokeSkillTool` | The `invoke_skill` tool the LLM calls |
| `AgentEvent::SkillScopeChanged` | Broadcast to TUI when scope enters/exits |

**Files**: `src/skills/mod.rs`, `src/skills/bundled/*.md`, `src/tools/skills.rs`, `src/session/mod.rs` (`active_skill_scope`, `invoked_skills`), `src/types.rs` (`ToolContext.active_skill_scope`, `ToolContext.skill_registry`), `src/agent/loop.rs` (`apply_special_tool_effects`), `src/agent/system_prompt.rs` (skill listing).

---

## Skills UX — Commands & Status Bar

The Skills subsystem is fully interactive from inside the TUI. Nothing requires leaving the terminal or hand-editing JSON.

### Slash commands

```
/skills                        List all skills (grouped by source, active marked ●)
/skill <name> [args]           Invoke a skill. Hot-reloads registry first.
/skill show <name>             Print the skill's frontmatter summary + full body
/skill new <name>              Scaffold ./.claude/skills/<name>/SKILL.md and open $EDITOR
/skill generate <name> <task>  Generate a reviewed project skill from conversation
/skill gen <name> <task>       Alias for /skill generate
/skill generate-from-conversation <name> <task>
                               Explicit alias for conversation-based generation
/skill edit <name>             Open an existing skill in $EDITOR
/skill delete <name>           Remove a project skill directory (bundled cannot be deleted)
/skill reload                  Force re-scan of all three skill directories
/skill path                    Print the three search paths
/skill off                     Clear the currently-active skill scope
```

Aliases: `/skill clear` = `/skill off`, `/skill rm` = `/skill delete`, `/skill refresh` = `/skill reload`.

### Status-bar indicator

When any skill scope is active (whether from an LLM-triggered `invoke_skill` or a user-typed `/skill <name>`), the status bar gains a `🪄 <skill-name>` indicator. It appears immediately and clears when the turn ends or when `/skill off` is typed. The indicator is driven by the new `AgentEvent::SkillScopeChanged { name: Option<String> }` event so TUI state never drifts from session state.

### Tab-completion

Tab now autocompletes skill names in four contexts:

```
/skill rev<Tab>           → /skill review
/skill show re<Tab>       → /skill show review
/skill edit deb<Tab>      → /skill debug
/skill delete dep<Tab>    → /skill delete deploy-helper
```

On multiple matches it extends to the common prefix and prints the full candidate list as a system message.

### The `/skills` pretty list

Instead of the old flat dump, `/skills` now groups by source and marks the active skill:

```
Available skills (7):

[bundled]
  /skill debug                  Structured debugging workflow...
                                ↳ use when: the user needs bug diagnosis.
  /skill refactor               Plan and apply safe refactors...
  /skill review                 Review changes for bugs, regressions...  ● ACTIVE
  /skill project-memory         Capture durable project guidance...

[user]
  /skill my-commit-style        Commit message formatting I like...

[project]
  /skill deploy-helper          Deploys this service to staging...
  /skill review                 Project override of the bundled reviewer

Subcommands: /skill <name> [args] | show | new | generate | edit | delete | reload | path | off
```

### Conversation-to-skill generation

Use skill generation when a conversation has produced a reusable workflow that you want the agent to remember as an invocable project skill. Good examples are repeatable build/release procedures, repo-specific debugging playbooks, review checklists, migration recipes, or domain workflows that will come up again. Avoid generating a skill for one-off facts, secrets, temporary credentials, or vague preferences that would be better stored in project memory.

```
/skill generate release-build "Build and release the Windows binary using the project instructions"
/skill gen alphaevolve-sim "Generate and validate AlphaEvolve-style data-center simulations"
/skill generate-from-conversation dp-helper "Solve dynamic-programming coding tasks with tests"
```

Generation uses the currently active provider and model. The prompt is built from the current conversation, including compacted summaries when the original detailed messages have already been replaced by `/compact` or auto-compaction. The generated draft is sanitized, validated as `SKILL.md`, and shown in a review modal before anything is written to disk.

During review:

- `Y` or `Enter` creates the skill after a final validation pass.
- `E` toggles the raw `SKILL.md` view so you can inspect frontmatter, body, `allowed_tools`, and safety notes.
- `Esc` cancels without creating files.

Generated project skills are saved under `./.claude/skills/generated-<name>/SKILL.md`, then loaded into the normal skill registry. They appear in `/skills`, can be inspected with `/skill show generated-<name>`, edited with `/skill edit generated-<name>`, and invoked with `/skill generated-<name>`.

Treat generated skills as security-sensitive. The generator removes or narrows unsafe instructions and high-risk tool allowlists, but you should still review the final `allowed_tools`, `description`, `when_to_use`, and body before accepting. If a generated skill blocks a tool that the workflow genuinely needs, edit the skill deliberately instead of broadening the allowlist blindly.

### Configuration knobs

```toml
# ~/.forge-osh/config/config.toml
[agent]
skills_enabled = true                      # master switch
include_skills_in_system_prompt = true     # show skills to the LLM
max_skill_listed_in_prompt = 12            # cap on how many skills are listed
```

Set `skills_enabled = false` to fully disable the subsystem (no system-prompt listing, `/skill` and `/skills` report disabled, `invoke_skill` tool still registered but disabled skills can't be invoked).

---

## How to Use, Add, Modify & Delete Skills

A practical walkthrough. Everything below works from inside the running TUI.

### 1. See what's available

```
/skills
```

Output is grouped by source (bundled / user / project) with descriptions and any active marker. If nothing matches, you'll see a message pointing to the two writeable skill directories and instructions to run `/skill new <name>`.

### 2. Invoke a skill two ways

**You invoke it explicitly:**
```
/skill review
/skill debug "null pointer in parser.rs"
```
Args are passed via `${ARGS}` substitution inside the skill body. The registry is refreshed on every invocation so any edit you made seconds ago applies without a manual reload.

**The agent invokes it autonomously:**
When you describe a task that matches a skill's `description` + `when_to_use`, the LLM calls the `invoke_skill` tool on its own. You'll see `🪄 <name>` appear in the status bar. The skill scope auto-clears when the agent's turn ends.

### 3. Create a new skill

```
/skill new deploy-helper
```

This:
1. Creates `./.claude/skills/deploy-helper/SKILL.md` with a complete starter template (name, description, when_to_use, allowed_tools, execution_mode, user_invocable, and a markdown body scaffold)
2. Opens it in `$EDITOR` (falls back to `$VISUAL`, then `notepad` on Windows, `vi` on Unix)
3. Refreshes the shared registry so the skill is discoverable immediately

Edit the file:
- Tighten `description` and `when_to_use` — these are what the LLM sees and uses to decide whether to invoke the skill
- Narrow `allowed_tools` to just what the workflow needs — this is a real safety boundary
- Pick `execution_mode: inline` for skills that should continue the current conversation, `fork` for skills that should run in isolation

Save and exit. Invoke with `/skill deploy-helper`.

### 4. Generate a skill from the current conversation

```
/skill generate deploy-helper "Deploy this service to staging using the procedure we just validated"
```

This is the fastest way to convert a successful session into a reusable project workflow:

1. Finish the task or discuss the workflow until the conversation contains the important steps, files, commands, caveats, and validation criteria.
2. Run `/skill generate <name> <task description>` with a narrow task description.
3. Review the generated preview carefully. Use `E` to inspect raw markdown, `Y` or `Enter` to create, or `Esc` to cancel.
4. Confirm the saved skill with `/skill show generated-<name>`.
5. Make any refinements with `/skill edit generated-<name>`, then run `/skill reload` if needed.

Generated skills are intentionally project-local by default. Check them into Git only after review, especially if the conversation included private paths, deployment details, or security-sensitive procedures.

### 5. Modify an existing skill

```
/skill edit deploy-helper
```

Opens the file in `$EDITOR`. After save, the next `/skill <name>` invocation picks up the change automatically (or run `/skill reload` to refresh explicitly).

**Editing bundled skills**: the bundled versions are compiled into the binary and cannot be edited in place. But you can **shadow** them: `/skill new review` creates a project-level `review` that fully overrides the bundled one. Copy the bundled body as a starting point by running `/skill show review` first and pasting the output.

### 6. Inspect a skill before invoking

```
/skill show debug
```

Prints the frontmatter summary (description, source, path, allowed_tools, mode) and the full markdown body. This is exactly what the agent will receive when the skill is invoked — no hidden transformation.

### 7. Delete a project skill

```
/skill delete deploy-helper
```

Removes `./.claude/skills/deploy-helper/` entirely. Bundled skills cannot be deleted (they live in the binary), but a project override can be deleted and the bundled version takes back over.

### 8. Leave a skill scope early

If the agent has entered an inline skill scope and you want to release the tool-allowlist restriction before the turn finishes:

```
/skill off
```

Clears `session.active_skill_scope`, removes the status-bar indicator, and emits a `SkillScopeChanged { name: None }` event.

### 9. Find where skills live

```
/skill path
```

Prints the three paths the loader scans. Useful for checking in project skills via Git (commit `./.claude/skills/`) or syncing user skills across machines (rsync `~/.forge-osh/skills/`).

### 10. Author a skill — best practices

- **Be specific in `description`** — this is what the LLM reads to decide whether to invoke
- **Write `when_to_use` as triggers, not tutorials** — e.g., "the user mentions a failing test" is better than "use this for debugging"
- **Narrow `allowed_tools`** — start with the minimum and widen only when the workflow demands it
- **Prefer `inline` over `fork`** — inline participates in the conversation, fork starts cold with only the skill body as context
- **Use `${ARGS}`, `${FORGE_SESSION_ID}`, `${FORGE_SKILL_DIR}`** in the body for dynamic content
- **Check your skill into version control** at the project level so the team benefits
- **Promote stable skills to user-level** at `~/.forge-osh/skills/` so every project gets them
- **Test with `/skill <name>`** before trusting the agent to find them — if `/skill show` looks right, the system prompt will show the LLM the same text

### 11. Config gates

```toml
[agent]
skills_enabled = true                   # master on/off
include_skills_in_system_prompt = true  # let the LLM auto-invoke
max_skill_listed_in_prompt = 12
```

Turn off `include_skills_in_system_prompt` if you want skills to be usable manually (via `/skill <name>`) but invisible to the LLM's autonomous selection.

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
