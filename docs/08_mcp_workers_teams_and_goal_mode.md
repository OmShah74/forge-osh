# MCP, Workers, Teams, and Goal Mode

## MCP subsystem overview

The Model Context Protocol integration lives under `src/mcp/`. The central runtime component is `McpManager` in `src/mcp/manager.rs`.

Its responsibilities include:

- loading configured server records
- resolving built-in catalog metadata
- collecting required secrets
- spawning MCP servers
- performing handshake and tool listing
- dynamically registering/unregistering tools in the shared tool registry
- exposing status snapshots for the TUI

This turns MCP into a native runtime extension mechanism.

## Why MCP matters in forge-osh

MCP lets the agent call authenticated external tools through structured child processes rather than generic web scraping or ad hoc shell glue.

In this repository’s runtime model, MCP is not separate from the agent tool system. MCP tools are normalized into the same registry the agent already uses.

## MCP server state model

`McpManager` tracks servers in an internal map of `ServerState` records containing:

- display metadata
- spawn command and args
- secret specs
- enabled flag
- status
- last error
- client handle
- registered tool names
- server version

## MCP status model

Server status is represented by `ServerStatus` variants:

- `Disabled`
- `Disconnected`
- `Connecting`
- `Active`
- `Error(String)`

This state model feeds directly into the UI manager modal and status reporting.

## Catalog plus config overlay model

`load_from_config()` first preloads every built-in catalog entry in a disabled state, then overlays user config.

This is a thoughtful design because it supports both:

- discoverability of known MCP servers even before they are enabled
- custom overrides and fully custom server definitions

## Secrets handling

For each server, the manager builds environment variables from either:

- matching environment variables
- stored secrets in the key store using an MCP-specific key namespace

Required secrets that are missing move the server into an error state rather than spawning a broken process.

This is a security-conscious and user-friendly approach.

## Connection lifecycle

Connecting a server involves:

1. marking the server as connecting
2. preparing environment variables and effective args
3. checking for missing required secrets
4. spawning the MCP client process over stdio
5. performing handshake
6. listing server tools
7. registering those tools into the shared registry
8. updating server state to active

On failures, the server state records the error and avoids partial activation.

## Dynamic tool registration

One of the most important MCP architecture choices is that server tools are registered into the shared `ToolRegistry` at runtime.

This means:

- newly connected servers immediately expand the model’s capability surface
- disconnected servers can remove their tools cleanly
- the rest of the agent stack does not need custom branches per server

## TUI-facing snapshots

`ServerSnapshot` exposes user-visible fields such as:

- id
- display name
- description
- category
- enabled flag
- current status
- tool count
- server version
- recent stderr
- required secret status
- last error

This is what makes a serious in-terminal MCP manager feasible.

## Worker/coordinator subsystem overview

Parallel and durable multi-agent work is managed by `Coordinator` in `src/agent/coordinator.rs`.

The coordinator owns:

- shared provider router
- tools
- config
- graph
- session
- event channels
- worker notification channels
- active worker handles
- optional team board

## Ad-hoc workers

The coordinator can spawn classic background workers for multithread mode. These preserve lightweight `@worker` workflows without requiring the heavier team-board system.

## Worker notifications

The coordinator drains notifications from workers and converts them into `AgentEvent`s for the UI.

It handles outcomes such as:

- completed with result and token usage
- failed with error
- stopped
- running state transitions

This makes background execution visible and debuggable.

## Durable team boards

Team mode is implemented in `src/agent/team.rs`.

A `TeamBoard` records:

- id
- goal
- current phase
- bus configuration
- working directory
- tasks
- event log
- timestamps

This gives team workflows durable state on disk rather than relying only on ephemeral worker memory.

## Team phases

Team boards move through phases such as:

- `Planning`
- `Running`
- `Reviewing`
- `Completed`
- `Failed`
- `Conflict`
- `Stopped`

This is much richer than simple worker success/failure and is designed for parallel coordination with review.

## Team tasks

Each `TeamTask` tracks:

- id
- kind (`Worker` or `Review`)
- title
- prompt
- status
- worker id
- result
- artifacts
- review notes
- error
- duration
- timestamps

This structure lets forge-osh manage parallel subtasks as explicit work items.

## Artifact reporting and conflicts

Team tasks can report `TeamArtifact` values with path and summary information. The board can detect overlapping artifact paths and enter conflict-oriented flows.

This is an important safeguard against parallel agents trampling the same files.

## Team persistence

Boards are saved as JSON and can be reloaded for the same working directory. If an old running board is loaded without live workers, it is marked stopped. That is a thoughtful recovery behavior for crashed or restarted sessions.

## Review worker concept

The team system distinguishes ordinary worker tasks from review tasks. This encourages a workflow where parallel work is later synthesized and checked, rather than blindly merged mentally by the user.

## Goal mode overview

Goal support lives in `src/agent/goal/` and is explicitly documented as a staged implementation.

The goal system introduces durable autonomous objectives with:

- a goal specification
- stopping conditions
- verifiers
- budgets
- policy controls
- persistence
- supervision

## Goal specification model

`GoalSpec` includes:

- `id`
- `objective`
- `stopping_condition`
- `verifiers`
- `budget`
- `policy`
- `created_at`
- `seed_files`
- `workdir`

This is much more formal than a normal conversation request.

## Verifiers

Goal verifiers can include:

- shell command checks
- file existence checks
- file content checks
- clean git tree checks
- custom verifier commands

This is how goal mode moves toward verifiable completion rather than the model merely claiming success.

## Budget model

The budget tracks limits such as:

- max turns
- max wall time
- max input tokens
- max output tokens

Per code comments, cost is tracked but not enforced as a hard ceiling.

## Goal policy model

Goal policy includes:

- network enabled/disabled
- auto-approve strategy
- allowed write globs
- deny globs
- shell allowlist

This is critical for unattended execution.

## Why these subsystems fit together

MCP, workers, teams, and goals are all about controlled expansion of agent autonomy.

- MCP expands what the agent can act on
- workers expand how much it can do in parallel
- teams expand how it coordinates complex work durably
- goal mode expands how long and how autonomously it can pursue an objective

Together they show that forge-osh is evolving from a single-thread coding assistant into a broader terminal agent platform.
