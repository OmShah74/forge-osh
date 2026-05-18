# Tools, Tool Execution, and Safety Model

## Tool system overview

The built-in tool framework lives under `src/tools/`. Every tool implements the `Tool` trait defined in `src/tools/mod.rs`.

Each tool provides:

- `name()`
- `description()`
- `parameters_schema()`
- `permission_level()`
- optional `effective_permission_level(input)`
- optional `is_concurrency_safe()`
- async `execute(input, ctx)`

This trait design gives forge-osh a uniform execution contract for built-in tools and dynamically added MCP tools.

## Tool registry design

`ToolRegistry` stores tools in a `parking_lot::RwLock<HashMap<String, Arc<dyn Tool>>>`.

This is significant for two reasons:

1. the registry can be shared widely without ownership problems
2. tools can be added and removed at runtime

That second property is essential for MCP integration, where tool availability changes when external servers connect or disconnect.

## Built-in tool categories

The registry registers built-ins from several modules:

- file system tools in `fs`
- shell tools in `shell` and `powershell`
- git tools in `git`
- search tools in `search`
- web tools in `web`
- code quality tools in `code`
- task tools in `tasks`
- agent tools in `agent_tools`
- skills tool in `skills`
- notebook tool in `notebook`
- worktree tools in `worktree`

Graph and LSP tools are registered by `App::new()` so they can be wired with shared graph/LSP handles.

## Config-aware registration

`ToolRegistry::register_enabled()` checks `config.is_tool_enabled(tool.name())` before registration.

This means the set of tools visible to the model is the result of runtime configuration, not just compile-time code.

## Permission levels

Tools use `PermissionLevel` values to express risk.

From the code and docs, the important levels are:

- `ReadOnly`
- `Mutating`
- `Destructive`
- `Shell`
- `Network`

A critical detail is that `effective_permission_level()` can vary by input. For example, shell tools may classify a harmless listing command differently from a destructive mutation command.

## Concurrency safety

The tool trait includes `is_concurrency_safe()`.

Read-only operations can opt into parallel execution when safe. This helps the agent efficiently perform multiple inspections in a single turn without introducing mutation races.

## Tool executor responsibilities

`ToolExecutor` in `src/tools/executor.rs` is the enforcement layer between the agent and the tools themselves.

It performs:

- tool lookup
- JSON-schema validation
- permission decisioning
- diff preview handling for file mutations
- permission prompting
- cancellation racing
- panic capture
- output truncation

This is one of the most security- and correctness-sensitive components in the whole application.

## Input validation

Before running a tool, the executor validates the provided JSON input against `parameters_schema()`.

If validation fails, the tool does not run.

This protects against:

- malformed tool inputs from the model
- accidental omission of required fields
- invalid argument shapes reaching filesystem or shell code

Production paths enable validation by default; tests can opt out using `new_unvalidated()`.

## Permission decision order

The executor documents the effective order explicitly.

1. bypass/trust mode allows everything
2. plan mode denies non-read-only tools
3. read-only tools auto-allow
4. stored permission rules are consulted
5. diff review can still force explicit review for file mutations
6. `AcceptEdits` can allow mutating operations
7. otherwise the user is prompted

This layered decision system is one of forge-osh’s defining safety features.

## Skill-scope restrictions

The executor also enforces active skill scope allowlists before persistent allow rules are considered.

That means a skill can narrow allowed tools even if the broader app config would otherwise permit them.

This is a strong containment feature for reusable workflows.

## Diff-before-apply review

When `ctx.diff_review` is active and the tool is a file mutation tool, the executor forces a review prompt even if a stored allow rule exists.

It tries to generate a preview using `fs::preview_file_tool_change()` and includes that diff or summary in the permission description.

This matters because it lets users approve the actual patch rather than blindly approving a tool name.

## Cancellation behavior

The executor races actual tool execution against the cancellation token.

If cancellation wins, the tool returns an explicit error rather than silently disappearing.

This is important for interactive reliability, especially with:

- long-running shell commands
- network operations
- LSP requests
- complex file operations

## Panic isolation

Tool execution is wrapped with `catch_unwind()`. If a tool panics, the executor returns a structured error like:

- tool name
- panic message if available

This keeps a single bad tool from crashing the entire agent process.

## Output truncation

The executor truncates oversized tool output to `max_output_chars` and appends a truncation notice.

This prevents extremely large command or file outputs from exploding the session context while still preserving useful feedback.

## File system safety model

The file tools are central to forge-osh’s coding-agent use case, so they are wrapped in multiple protection layers.

### Read-before-write discipline

The application strongly prefers reading before mutation, and the file-state cache enforces stale-view protection.

### File-state cache

`FileStateCache` fingerprints files using:

- file size
- modified time
- SHA-256 of file contents

When a file was previously read and later changes externally, a write/edit is blocked until the file is re-read.

This prevents the model from overwriting newer human or external changes based on stale context.

### Snapshot-based undo

Mutating file operations snapshot the original state so `/undo` can restore it.

This is a very practical safety feature for real terminal workflows.

## Shell and PowerShell tools

forge-osh supports both:

- `bash` for shell command execution
- `powershell` for Windows-native PowerShell execution

Key safety characteristics include:

- blocked-command protections
- permission classification by command content
- timeouts
- copied terminal prompt marker handling

This makes the shell tools convenient without being totally unconstrained.

## Git tools

The git tool set includes both read-only and mutating operations.

Examples:

- status, diff, log, blame, show
- add, commit, branch, checkout, stash
- reset, fetch, pull, push

The safety philosophy is visible both in docs and code comments:

- inspect before staging or committing
- avoid destructive resets casually
- preserve unrelated user changes
- treat forceful operations as high-risk

## Search and navigation tools

The native search tools provide a cheaper, safer alternative to broad shell usage:

- `search_files` for grep-like search
- `find_files` for file discovery

These respect ignore rules by default and provide fine-grained filters, which is especially useful in agentic execution where broad scans can waste time and context.

## Code-quality tools

The code tools allow the model to verify work autonomously:

- `run_linter`
- `run_tests`
- `run_formatter`

These tools are important because they let the agent close the loop by checking its own edits rather than only asserting success.

## Task and orchestration tools

Task tools and orchestration tools help the model manage multi-step work:

- TODO tracking
- session task tracking
- asking for clarification
- entering/exiting plan mode
- invoking skills

These tools are unusual compared with many coding agents because they expose planning state as a first-class capability rather than hiding it entirely in prompt text.

## Worktree isolation tools

The worktree tools provide an escape hatch for risky changes:

- create isolated worktrees
- inspect existing worktrees
- remove temporary worktrees

This is a high-leverage safety feature for experimentation without destabilizing the main checkout.

## Dynamic MCP tools

MCP server tools are registered into the same tool registry at runtime. That means from the agent’s perspective, built-in tools and MCP-backed tools are normalized under the same tool-calling abstraction.

This is a powerful extension architecture choice.

## Safety philosophy in one sentence

forge-osh does not rely on a single safety mechanism. It layers schema validation, permission modes, stored rules, diff review, skill scope restrictions, file-state caching, undo snapshots, and cancellation so that one mistake does not become a destructive action.
