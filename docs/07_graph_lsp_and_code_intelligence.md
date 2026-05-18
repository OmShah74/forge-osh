# Semantic Graph, LSP, and Code Intelligence

## Two-layer intelligence strategy

forge-osh has two distinct code intelligence systems:

1. an internal semantic code graph in `src/graph/`
2. live compiler-backed Language Server Protocol support in `src/lsp/`

These solve different problems.

- the graph provides deterministic project structure and token-efficient navigation
- LSP provides live definitions, references, diagnostics, and renames from real language servers

Together they form a stronger system than either alone.

## Semantic graph overview

The graph subsystem is centered on `CodeGraph` in `src/graph/mod.rs`.

A graph contains:

- `meta`
- a `petgraph::StableGraph<GraphNode, EdgeType, Directed>`
- three in-memory indices rebuilt on load:
  - `fqdn_index`
  - `file_index`
  - `name_index`

## Shared graph handle

The application shares the graph through:

```rust
type SharedGraph = Arc<RwLock<Option<CodeGraph>>>;
```

This means the graph is optional at runtime and can be loaded or absent without changing the rest of the application’s behavior.

## Why the graph is optional

The optional design is important:

- users do not need to build the graph to use forge-osh
- graph-aware tools can fail gracefully when no artifact is loaded
- startup cost is avoided unless the feature is used

## Graph artifact persistence

The graph is serialized with `bincode` as a binary artifact.

Important graph persistence behaviors:

- artifact filename is deterministic per project root
- artifacts are stored near the executable directory
- `try_load()` auto-loads a matching artifact if present
- artifacts are version-gated by `GRAPH_VERSION`
- indices are rebuilt after deserialization

This gives fast startup reuse without requiring a rebuild on every launch.

## Graph node insertion and indexing

`CodeGraph::add_node()` updates the graph plus all indices at insertion time.

That enables efficient lookup by:

- fully qualified name
- file path
- short name

This is one of the reasons the graph can act as a token-saving navigation layer for the agent.

## Graph edges and deduplication

`CodeGraph::add_edge()` avoids duplicate parallel edges, which helps keep the structure cleaner and query behavior more predictable.

## Graph build feedback

`GraphBuildMsg` supports:

- progress messages
- successful completion including graph and artifact path
- error messages

This allows background graph build operations to report progress back to the TUI cleanly.

## Supported graph query shape

The graph tool supports operations like:

- `find`
- `context_pack`
- `blast_radius`
- `file_graph`
- `callers`
- `mutations`
- `stats`

This moves the agent from text search toward semantic navigation.

## Why graph queries matter

Compared with repeatedly reading or grepping files, the graph can:

- find symbols deterministically
- identify dependency impact
- gather focused context packs
- reduce token waste in large repositories

That makes it especially valuable in large codebases where prompt budget is expensive.

## LSP manager overview

The live LSP subsystem is centered on `LspManager` in `src/lsp/manager.rs`.

It stores:

- a workspace root
- loaded server specs
- a per-language client cache

The shared type is:

```rust
type SharedLspManager = Arc<LspManager>
```

## Per-language client cache

LSP clients are cached by language. This prevents duplicate server spawns and allows concurrent tools to reuse one warm language server.

This is a major performance feature.

## Lazy startup with warm-up

LSP servers are:

- warmed in the background at app startup when possible
- still spawned lazily on first direct use if not already running

This hybrid strategy balances responsiveness with convenience.

## Path-based server resolution

`client_for_path()` determines the correct language server for a file based on file extension and registry metadata.

If no server is registered, the system returns a friendly error rather than failing opaquely.

## Language-based server resolution

`client_for_language()` supports operations that target a language explicitly, such as workspace symbol search or installation flows.

## Root detection

When spawning a server, the manager calls `detect_project_root()` using language-specific root markers.

Examples include:

- `Cargo.toml`
- `package.json`
- `pyproject.toml`
- `go.mod`

This is important because correct project-root selection is often the difference between useful and broken LSP results.

## Installation support

The LSP layer can attempt built-in installation commands for some languages. If a server is missing and a safe installer is known, forge-osh can install it into managed locations rather than forcing the user to globally preinstall everything.

This is a notable usability improvement over many terminal tools.

## Workspace language detection

`detect_project_languages()` walks the workspace using ignore-aware traversal and infers which languages are present. This powers warm-up and auto-install/start flows.

## Running clients inspection

`running_clients()` returns a summary with:

- language
- root
- initialization status

This is useful for `/lsp status` style UI and diagnostics.

## Supported language registry

The manager exposes supported language information based on loaded server specs. This means support is registry-driven, not hardcoded only into command logic.

## LSP tools

The app registers LSP-backed tools for:

- diagnostics
- definition lookup
- references lookup
- hover
- document symbols
- workspace symbols
- rename

These tools elevate the agent from text-level operations to compiler-grade understanding.

## Why LSP is different from the graph

The graph is parser-based and project-structural.

LSP is live and type-aware.

LSP can answer questions involving:

- type resolution
- real symbol binding
- diagnostics after edits
- scope-aware references
- safe renaming

The graph is better for:

- cheap global structure
- dependency exploration
- stable project artifact reuse
- navigation even when language servers are absent

## Post-edit diagnostics

The architecture notes indicate that after successful file edits, forge-osh may run a short LSP diagnostic check and append results. This is an excellent product decision because it catches issues immediately after mutation.

## Graph and LSP coexistence

Both graph and LSP tools are registered in the main tool registry by `App::new()`. They self-disable gracefully if unavailable.

This means the agent can be prompted with a consistent tool surface and decide the best available mechanism at runtime.

## Why this subsystem matters

Most coding agents stop at file search and shell commands. forge-osh goes further by embedding two complementary code intelligence layers directly into its tool system.

That improves:

- correctness
- token efficiency
- confidence in refactors
- navigation speed
- verification quality after edits

This is one of the project’s strongest technical differentiators.
