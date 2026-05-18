# Data Files, Persistence, and Runtime Paths

## Why runtime paths matter

forge-osh is a stateful terminal application. Understanding where it stores data is important for backup, debugging, security review, and deployment.

The config module centralizes most important directories.

## Core path functions

From `src/config/mod.rs`:

- `config_dir()` -> configuration root, default `~/.forge-osh`
- `data_dir()` -> local app data root, default platform-local app data directory plus `forge-osh`
- `log_dir()` -> logs subdirectory
- `sessions_dir()` -> session storage subdirectory

These can also be influenced by environment overrides like `FORGE_CONFIG_DIR` and `FORGE_DATA_DIR`.

## Configuration files

Typical config-root files include:

- `config.toml`
- `keys.json`
- `permissions.json`
- hooks config files
- custom LSP config
- user CLAUDE memory
- user skill directories

Not all were read directly in this session, but the code and project docs clearly rely on this config-root pattern.

## Data-directory contents

The app data directory stores more operational, mutable runtime state such as:

- logs
- session JSON files
- team boards and similar durable runtime data
- managed runtime assets such as installed LSP sidecars

This distinction between config and data is a good systems design choice.

## Session storage

Sessions are saved as JSON files named by session id:

```text
<sessions_dir>/<session_id>.json
```

Each session file stores:

- metadata like session name and provider/model
- full conversation history
- cost tracking
- working directory
- effort level
- skill invocation state

## Session export

Session export to markdown is generated on demand and is separate from the JSON persistence format.

This is useful because JSON is for machine resumption, while markdown is for humans.

## Logs

The application initializes tracing output into the log directory. This is useful for:

- diagnosing startup issues
- provider integration problems
- MCP or LSP failures
- runtime debugging in verbose mode

## Key storage

Persistent API keys are managed through `KeyStore` and stored under the config root. The project instructions emphasize that keys are never logged and environment variables take precedence over stored values.

## Permission rules storage

The permission system persists rules in a user-level JSON file under the config root. These rules survive across sessions, which is why forge-osh can learn repeated approvals or denials.

## Hooks storage

Hooks are configured from JSON under the config root. Because hooks can execute shell commands, these files are security-sensitive and should be treated accordingly.

## CLAUDE memory files

Memory may come from multiple locations:

- repository `CLAUDE.md`
- parent directory `CLAUDE.md`
- `~/.forge-osh/CLAUDE.md`
- `~/.claude/CLAUDE.md`

This means behavioral memory is distributed by scope rather than collapsed into one giant hidden database.

## Skill storage

Skills are stored as directories containing `SKILL.md` files.

There are three main roots:

- bundled skills in the binary/project distribution
- user skills in the user skill directory
- project skills under `.claude/skills` in the workspace

This layout makes skills easy to version, inspect, and edit manually.

## Team board persistence

The durable team system stores boards as JSON files. Boards include timestamps, task state, event history, artifacts, and phase information, allowing work to survive restarts.

## Goal persistence

The `/goal` subsystem includes dedicated persistence modules. The implementation is intended to support durable autonomous goals rather than ephemeral prompt-only execution.

## Graph artifact storage

The semantic code graph is stored differently from most other data.

Important graph persistence facts:

- it is serialized with `bincode`
- it is stored as `forge_graph_<project>.bin`
- it is versioned with `GRAPH_VERSION`
- it is placed near the executable directory via `artifact_dir()` logic

This is unusual but intentional: graph artifacts are treated more like reusable indexed binaries than normal config files.

## LSP managed assets

The LSP subsystem can install language servers into forge-osh-managed locations rather than relying only on global tools. These managed assets live in app-controlled directories under the data ecosystem.

## MCP secrets and state

MCP server config is persisted through config structures, while runtime connection state is held in memory by `McpManager`. Secrets are loaded from environment variables or key storage, not embedded in server definitions printed to the UI.

## Working directory significance

Many runtime artifacts are tied conceptually to the working directory:

- sessions may restore into that directory
- graph artifacts are resolved per root
- LSP project root detection starts there
- team boards are loaded for the same directory
- project skills and CLAUDE memory depend on that tree

This means forge-osh’s state is partly user-global and partly workspace-local.

## Data separation philosophy

The application uses different storage formats for different goals:

- TOML for human-edited config
- JSON for structured persistent runtime state
- Markdown for human-authored memory and skills
- binary bincode for high-performance graph artifacts

That is a pragmatic choice rather than forcing one format everywhere.

## What to back up

For a full user backup, the most important locations are:

- config directory
- data directory
- project-local `.claude/skills`
- project-local `CLAUDE.md`

This preserves settings, keys, sessions, memory, and extensions.

## Security-sensitive files

The most sensitive runtime files are:

- `keys.json`
- hook configs
- MCP secret-backed config state
- permission rules if they allow dangerous commands broadly

These should be protected and not committed accidentally.

## Why this persistence model is good

The runtime file design reflects a mature terminal app philosophy:

- user-global state is available across projects
- project-local state can travel with the repo when appropriate
- session state is durable and inspectable
- advanced subsystems like graph, LSP, skills, teams, and goals all have explicit storage surfaces

This makes forge-osh understandable and operable as a real development tool rather than a black-box chat client.
