# forge-osh Project Overview

## What forge-osh is

`forge-osh` is a Rust-based terminal coding agent whose binary name is `forge-osh` and crate name is `forge_agent`. It is designed as a provider-agnostic coding assistant that can run in interactive TUI mode, non-interactive single-prompt mode, or stdin pipe mode.

At a high level, the application combines:

- a CLI entrypoint built with `clap`
- a full-screen terminal UI built with `ratatui` and `crossterm`
- an autonomous agent loop that can call tools repeatedly
- a provider router that supports cloud and local LLM backends
- persistent sessions, memory, permissions, skills, hooks, and optional code intelligence

## Core product goals

The project is built around a few consistent design goals visible across the codebase:

- **Provider agnosticism**: users can switch between many cloud and local model providers.
- **Terminal-first workflow**: the application is meant to feel native in a shell rather than browser-based.
- **Agentic execution**: the assistant is not limited to chat; it can inspect files, run commands, edit code, and verify changes.
- **Safety controls**: permissions, diff review, file-state caching, undo, and worktree support reduce accidental damage.
- **Extensibility**: skills, MCP servers, hooks, LSP, and graph tooling are all pluggable layers.

## Entry flow

The startup path is defined in `src/main.rs`.

1. Parse CLI args with `Cli::parse()`.
2. Initialize logging in the app data log directory.
3. If a CLI subcommand is provided, build `App` and run the subcommand.
4. Otherwise create `App`, check whether at least one provider is configured, and if not run the first-time setup wizard.
5. Decide runtime mode:
   - non-interactive prompt mode if prompt args were supplied
   - stdin pipe mode when not attached to a TTY and stdin has data
   - interactive full-screen TUI mode otherwise

## Major runtime modes

### 1. Interactive TUI mode

Started by running:

```bash
forge-osh
```

This launches the full terminal application with:

- conversation rendering
- input editor
- permission modals
- pickers and management UIs
- live session, provider, model, token, cost, theme, and status information

### 2. Non-interactive one-shot mode

Started by passing a prompt directly:

```bash
forge-osh "Refactor the auth module"
```

This runs the agent once against the supplied prompt without opening the TUI.

### 3. Pipe mode

When stdin is not a TTY and contains data, forge-osh reads the entire stdin stream and uses it as the input prompt.

```bash
cat errors.log | forge-osh "Diagnose these build errors"
```

## Main top-level modules

The crate exposes the following top-level modules from `src/lib.rs`:

- `agent`
- `app`
- `cli`
- `config`
- `error`
- `graph`
- `lsp`
- `mcp`
- `provider`
- `session`
- `skills`
- `tools`
- `tui`
- `types`

These correspond closely to the application’s runtime layers.

## Architectural layering

A simplified view of the system is:

1. **CLI/TUI layer**: gathers user input, shows status, renders messages, hosts modals.
2. **App composition layer**: creates config, providers, tools, session state, graph, LSP, skills, and MCP manager.
3. **Agent loop layer**: builds prompts, sends model requests, executes tool calls, manages retries, context, permissions, and skill scope.
4. **Tool execution layer**: validates schemas, checks permissions, executes built-in and dynamically registered tools.
5. **Optional intelligence layers**: semantic code graph and LSP.
6. **Persistence layers**: sessions, config, keys, permissions, team boards, and goal state.

## App composition responsibilities

`App` in `src/app.rs` is the central composition root. It owns shared handles for:

- merged `Config`
- `ProviderRouter`
- `ToolRegistry`
- `Session`
- `KeyStore`
- `SharedGraph`
- `SharedLspManager`
- `SharedSkillRegistry`
- `McpManager`

`App::new()` is where most startup wiring happens:

- load config and environment overrides
- apply CLI overrides
- initialize the graph holder
- initialize keys
- build the provider router
- detect local providers
- create the session or resume one
- warm LSP in the background
- register tools, including graph and LSP tools
- load graph artifacts if present
- load and connect configured MCP servers in the background

## Session-centered runtime model

Most shared state in the program is built around the current session and working directory. The session determines:

- provider and model metadata shown in the UI
- conversation history sent to models
- token and cost tracking
- effort level
- active skill scope
- current working directory
- resume and export behavior

The working directory is also reused by:

- graph loading and building
- LSP workspace root detection
- skill loading
- MCP and team persistence in project context

## First-run behavior

When no providers are configured, `run_first_time_setup()` in `src/main.rs` opens a terminal prompt-based wizard. It lets the user choose a provider and store its API key in the key store, or choose Ollama / skip setup.

This is intentionally minimal and avoids blocking use with heavy setup requirements.

## Design characteristics visible in code

A few project-wide implementation choices stand out:

- async runtime is Tokio-based
- state sharing uses `Arc`, `Mutex`, `RwLock`, and `parking_lot`
- many subsystems are designed to self-disable gracefully when unavailable rather than crash
- dynamic capabilities are added at runtime through registries rather than hard-coded branching
- persistence uses human-readable JSON/TOML except for the binary graph artifact

## Documentation map for this docs set

This 10-file docs set is organized as follows:

1. `01_project_overview.md` - overall architecture and runtime model
2. `02_cli_config_and_providers.md` - CLI, config, keys, models, providers
3. `03_agent_loop_and_execution.md` - agent loop, context, planning, retries, permissions
4. `04_tools_and_safety.md` - built-in tools, tool executor, safety invariants
5. `05_tui_and_user_experience.md` - terminal UI, interaction model, commands, shortcuts
6. `06_sessions_memory_skills_and_hooks.md` - sessions, compaction, memory, skills, hooks
7. `07_graph_lsp_and_code_intelligence.md` - graph and LSP architecture
8. `08_mcp_workers_teams_and_goal_mode.md` - MCP, worker orchestration, team boards, goal support
9. `09_data_files_persistence_and_runtime_paths.md` - file layout and persistence behavior
10. `10_development_architecture_and_extension_guide.md` - development notes and extension points

## Source files most central to the architecture

If a developer wants to understand the application quickly, the most important files to read first are:

- `src/main.rs`
- `src/app.rs`
- `src/agent/loop.rs`
- `src/tools/executor.rs`
- `src/provider/router.rs`
- `src/tui/mod.rs`
- `src/config/mod.rs`
- `src/session/mod.rs`
- `src/mcp/manager.rs`
- `src/lsp/manager.rs`
- `src/graph/mod.rs`

These provide the clearest end-to-end picture of how forge-osh is assembled.
