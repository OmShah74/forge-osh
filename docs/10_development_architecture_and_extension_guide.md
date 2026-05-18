# Development Architecture and Extension Guide

## Technology stack

From `Cargo.toml`, forge-osh is built on a practical Rust-native stack.

### Core runtime and async

- `tokio`
- `tokio-util`
- `futures`
- `async-trait`
- `async-stream`

### CLI and terminal UI

- `clap`
- `ratatui`
- `crossterm`
- `syntect`
- `unicode-width`
- `textwrap`

### Networking and streaming

- `reqwest`
- `eventsource-stream`
- `url`
- `bytes`

### Serialization and storage

- `serde`
- `serde_json`
- `toml`
- `bincode`

### Filesystem and search

- `walkdir`
- `glob`
- `ignore`
- `dirs`
- `tempfile`

### Text and token handling

- `regex`
- `similar`
- `strsim`
- `unicode-segmentation`
- `strip-ansi-escapes`
- `html2text`
- `tiktoken-rs`

### Security and identifiers

- `sha2`
- `base64`
- `uuid`

### Logging and diagnostics

- `thiserror`
- `anyhow`
- `tracing`
- `tracing-subscriber`

### Code intelligence

- `petgraph`
- `rayon`

This stack shows the project aims to be self-contained, high-performance, and cross-platform.

## Crate structure for contributors

A developer extending forge-osh should think in terms of these top-level areas:

- `app` for composition and startup wiring
- `cli` for command-line surface
- `provider` for model backends
- `agent` for orchestration and autonomy
- `tools` for action surface
- `tui` for interaction and rendering
- `session` for persistence and context
- `skills` for reusable workflows
- `graph`, `lsp`, and `mcp` for advanced capabilities

## Best file to start with for each task

### Add a new provider

Start with:

- `src/provider/mod.rs`
- `src/provider/router.rs`
- relevant provider implementation files
- `src/config/models.rs`

### Add a new built-in tool

Start with:

- `src/tools/mod.rs`
- a relevant module under `src/tools/`
- `src/tools/executor.rs`
- `src/types.rs` for tool-related shared types if needed

### Change session behavior

Start with:

- `src/session/mod.rs`
- `src/session/history.rs`
- `src/session/checkpoint.rs`

### Change UI behavior

Start with:

- `src/tui/mod.rs`
- `src/tui/renderer.rs`
- `src/tui/input.rs`
- `src/tui/help.rs`

### Change agent reasoning or loop control

Start with:

- `src/agent/loop.rs`
- `src/agent/context.rs`
- `src/agent/compaction.rs`
- `src/agent/system_prompt.rs`

## How to add a new tool

A typical built-in tool extension flow is:

1. implement the `Tool` trait
2. define a JSON schema in `parameters_schema()`
3. choose a correct base permission level
4. optionally override effective permission classification
5. implement execution using `ToolContext`
6. register it in `ToolRegistry::with_config()`
7. ensure config enable/disable behavior is correct
8. verify it behaves well with permission prompts, cancellation, and output truncation

If the tool mutates files or performs shell/network work, extra safety review is needed.

## How to add a new provider

A provider must fit the router abstraction.

Typical steps:

1. implement the provider trait used by the router
2. add config surface for base URL/default model if needed
3. add model metadata to the catalog
4. update router construction to instantiate the provider when credentials are present
5. ensure tool-call support and streaming semantics are correct
6. verify token/cost/context metadata where possible

The project already shows two patterns to copy:

- dedicated native provider implementation
- OpenAI-compatible shared implementation

## How to add a new skill

For manual skill authoring:

1. create a directory under the correct skill root
2. add `SKILL.md`
3. provide frontmatter for name, description, allowed tools, execution mode, and optional hooks
4. reload or reopen the app

The loader handles precedence automatically.

## How to add a new MCP server

There are two main routes:

- add it to the built-in catalog
- define it as a custom server in config

A proper MCP integration should specify:

- id
- display metadata
- command and args
- required secrets
- safe spawn behavior

Because tools are registered dynamically, the rest of the app usually needs minimal extra changes.

## How to add or extend LSP support

LSP support is registry-driven.

Typical work involves:

- adding or modifying a server spec
- defining language id, extensions, root markers, candidates, and install hints
- ensuring path/language resolution works
- confirming diagnostics or symbol tools behave correctly

## How to extend the graph

To improve graph support, contributors would usually touch:

- parser logic
- builder logic
- graph query logic
- graph type definitions

Because artifacts are versioned, incompatible changes should bump `GRAPH_VERSION`.

## Testing and verification workflow

Project memory lists the standard commands:

```bash
cargo build
cargo build --release
cargo test
cargo test -- --test-threads=1
cargo clippy
cargo fmt
cargo run -- --help
```

For feature work, focused verification is better than broad claims. For example:

- run a targeted test or module-specific command when possible
- use LSP diagnostics after editing Rust code
- use the TUI manually for modal and interaction changes

## Important engineering patterns in this codebase

### 1. Graceful degradation

Many features are optional:

- no graph artifact -> graph tools self-disable
- no language server -> LSP tools report friendly unavailability
- no provider configured -> first-run setup kicks in
- no enabled MCP server -> the rest of the app still works

### 2. Shared registries

Several subsystems rely on registries:

- tool registry
- skill registry
- provider router
- LSP server specs
- MCP catalog

This keeps the code modular and extension-friendly.

### 3. Channel-based decoupling

The TUI and agent are connected through channels, not direct rendering calls. This is a good pattern to preserve.

### 4. Explicit persistence

Runtime state is stored in named files and typed structs rather than hidden caches. This helps debugging and user trust.

### 5. Safety before convenience

Permission prompts, diff previews, stale-write blocking, and undo all show that the project values safe autonomy.

## Contributor cautions

When extending the codebase, be especially careful around:

- permission logic
- shell command execution
- key/secrets handling
- MCP server spawn behavior
- session compatibility and persisted structs
- graph artifact versioning
- provider retry logic and error handling
- file mutation flows and undo behavior

These are cross-cutting systems where a small change can have large impact.

## Architectural identity of the project

forge-osh is best understood as a terminal-native orchestration platform for coding agents. Its architecture is not just "chat plus tools". It is a layered system with:

- pluggable model backends
- a safety-aware tool runtime
- durable user/workspace state
- customizable workflows via skills and hooks
- optional semantic and compiler-backed intelligence
- increasingly autonomous coordination layers

That identity is useful to keep in mind when contributing: new features should ideally strengthen the terminal-agent platform rather than add isolated one-off behavior.
