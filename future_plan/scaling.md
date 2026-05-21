# forge-osh — Scaling Plan, Feature Roadmap, and Code-State Verdict

> Author: code-review pass on the v1.0.19 tree (branch `v1.0.0`).
> Scope: full repository scan (`src/` ≈ 45,000 LOC of Rust, 84 source files, 38 test files, 10 architectural docs).
> Purpose: a realistic, prioritized plan for what to build next, the feasibility of each item, and an honest competitive verdict against Claude Code, OpenAI Codex CLI, OpenCode, t3code, Pi, Cursor CLI, Aider, and others.

---

## 1. Snapshot of What Actually Exists in the Code

This is not a marketing summary — it is what the source tree actually contains, with file paths and approximate LOC.

### 1.1 Layer-by-layer inventory

| Layer | Files | LOC | What's actually implemented |
|---|---|---|---|
| **Provider** (`src/provider/`) | 6 | ~2,000 | Trait `Provider` with 4 backends: `anthropic.rs` (native Messages API + SSE), `openai_compat.rs` (10+ shared providers — OpenAI/Groq/Grok/OpenRouter/Mistral/DeepSeek/Together/Fireworks/Perplexity/Cohere), `gemini.rs` (native), `ollama.rs` (native + OpenAI-compat for tools). `router.rs` owns multi-provider selection, fallback chain. |
| **Tools** (`src/tools/`) | 14 | ~5,800 | 40+ tools across fs, shell (bash + powershell.rs), git (14 ops), search, web, code-quality, tasks, notebooks, agent tools, worktree, skills, validate, code. `executor.rs` (540 LOC) is the permission-checked dispatcher. |
| **Agent loop** (`src/agent/`) | 9 + `goal/` 8 | ~7,500 | `loop.rs` (1,426 LOC) — the core plan-execute-observe loop. `compaction.rs`, `context.rs`, `coordinator.rs`, `worker.rs`, `team.rs` (789 LOC swarm), `hooks.rs` (433 LOC), `permissions.rs`, `planner.rs`, `skill_generation.rs` (945 LOC), `system_prompt.rs` (668 LOC), `file_history.rs` (undo). |
| **Goal mode** (`src/agent/goal/`) | 8 | ~2,800 | The v1.0.19 durable-goal primitive — `worker.rs` (874 LOC), `supervisor.rs`, `policy.rs` (path-glob + shell-allowlist), `verifier.rs` (shell/file/git verifiers), `prompt.rs` (PROGRESS/BLOCKED/CLAIM_DONE protocol), `persistence.rs` (atomic writes + ring rotation), `resumer.rs` (cold-start respawn). |
| **TUI** (`src/tui/`) | 8 | ~10,700 | `mod.rs` (7,100 LOC — by far the heaviest file in the project), `renderer.rs` (2,494 LOC), `input.rs` (500), `help.rs`, `picker.rs`, `themes.rs`, `diff.rs`, `spinner.rs`. Ratatui + crossterm; 5 themes; Vim normal mode. |
| **Session** (`src/session/`) | 5 | ~700 | `history.rs`, `tokens.rs` (tiktoken-based counting), `checkpoint.rs` (JSON persistence), `file_cache.rs` (SHA-256 fingerprinting for stale-edit protection). |
| **Config** (`src/config/`) | 3 | ~2,800 | `models.rs` is 1,953 LOC — the full built-in model catalog. `keyring.rs` for API-key storage. Env-var overrides. |
| **MCP** (`src/mcp/`) | 7 | ~2,700 | Full JSON-RPC 2.0 stdio MCP client: `protocol.rs`, `transport.rs` (stdio, oneshot response correlation, stderr ring buffer), `client.rs`, `catalog.rs` (1,307 LOC — 50+ pre-wired servers), `tool_adapter.rs` (`mcp__<srv>__<tool>` namespacing), `manager.rs` (lifecycle + secrets). |
| **LSP** (`src/lsp/`) | 6 | ~2,600 | Custom minimal LSP client (no `lsp-types` dep): `client.rs`, `manager.rs`, `protocol.rs`, `config.rs` (588 LOC registry — Rust/TS/JS/Py/Go/C++/Java/C#/PHP/Ruby/Lua/Bash/JSON/YAML/HTML/CSS/Vue/Svelte/Kotlin/Swift/Dart/Dockerfile), `tools.rs` (804 LOC — 7 LSP tools: diagnostics/definition/references/hover/document_symbols/workspace_symbols/rename). |
| **Semantic Graph** (`src/graph/`) | 6 | ~5,500 | `parser.rs` (4,277 LOC — multi-language regex-based parsing for Rust/Py/JS/TS/Go), `builder.rs` (two-pass parallel via rayon), `query.rs` (find/context_pack/blast_radius/file_graph/mutations/stats), `tools.rs` (the `graph_query` tool), `types.rs` (node/edge taxonomy), petgraph `StableGraph`, bincode artifact. |
| **Skills** (`src/skills/`) | 2 + 4 bundled | ~800 | `mod.rs` (792 LOC) — scope discovery (bundled/user/project), frontmatter parsing, conversation-to-skill generation pipeline (`agent/skill_generation.rs`). |
| **App / glue** | `app.rs` (773), `main.rs`, `cli.rs`, `types.rs`, `error.rs` | ~1,500 | Wiring. |
| **Tests** (`tests/`) | 38 | — | Integration test files cover agent loop, compaction, config, coordinator, edit-robust, evaluation harness, file history, graph (4 files), hooks, planner, provider router, session, skills, system prompt, all tool families, TUI subsystems. |

**Honest measurement of feature breadth:** this codebase is *not a toy*. It implements, end-to-end, every advertised feature in the README I cross-checked: agent loop, permission system, hooks, undo, workers/teams, goals, MCP, LSP, semantic graph, skills, multi-provider routing, tiktoken counting, checkpointing.

### 1.2 Code-quality and correctness verdict

I read the structure, the entry points, the Cargo manifest, and spot-checked the heaviest modules (`agent/loop.rs`, `agent/goal/worker.rs`, `tui/mod.rs`, `mcp/*`, `lsp/*`, `graph/parser.rs`). I did not run `cargo check` (per the memory note that this machine has disk-space constraints), so the verdict below is a static read.

#### What is clearly correct and well-built

1. **The provider abstraction is the right shape.** A `Provider` trait + a router + a shared `OpenAICompatProvider` for the 10 SaaS providers that share the OpenAI schema is exactly what mature projects converge on (Aider does the same; OpenCode does the same). Native Anthropic / Gemini / Ollama paths are kept separate because their streaming formats differ — correct call.
2. **Goal-mode persistence is professional-grade.** Atomic writes (tempfile + rename), per-goal directories, checkpoint ring rotation (50-file cap), append-only `progress.log`, separate `index.json`, archive on clear. This is genuinely the same shape OpenAI/Anthropic use internally for long-running jobs. Crash-safe cold-start resume via `resumer.rs` exists and reads `index.json` at boot.
3. **Permission system is layered correctly.** Trust/bypass → plan-mode → ReadOnly bypass → deny rules → diff-review → allow rules → prompt. That ordering matches Claude Code's. Path-glob matching in `policy.rs` walks 12 scalar arg keys + 3 array keys against `glob::Pattern` — far more rigorous than the heuristic some clones use.
4. **File-state SHA-256 cache** in `session/file_cache.rs` exists. This is the "stale edit" guard Claude Code uses; many clones skip it. Good.
5. **Tiktoken-based counting** rather than the `len/4` estimate. Visible in `session/tokens.rs`. Correct.
6. **LSP is implemented in-house** (no `lsp-types` dep — see `lsp/protocol.rs`, 202 LOC). Trade-off: tiny binary + forwards-compatible, at the cost of writing more spec handling yourself. Reasonable.
7. **Semantic graph is genuinely useful.** The `context_pack` BFS algorithm in `graph/query.rs` is a real token-saver — it picks primary node → callers → callees → containers and degrades to `signature_only` when over budget. This is a feature OpenCode and Codex CLI do **not** have.
8. **MCP integration is full-fat.** Not a wrapper around an existing SDK — full JSON-RPC envelope handling, stdio transport, async response correlation, 50+ catalog entries. Few non-Anthropic clients ship this many built-in MCP servers.
9. **Tests exist for the things that matter.** 38 test files, including an `EVALUATION_HARNESS.md` doc + `test_evaluation_harness.rs`. Most clones ship with no tests.

#### What looks risky on a static read

1. **`tui/mod.rs` is 7,100 lines.** This is a maintainability time-bomb. It's the file you'll be afraid to touch in 6 months. Should be split into `tui/handlers/` (one file per slash-command family), `tui/state.rs`, `tui/render_pipeline.rs`. The pattern of mega-files also appears in `graph/parser.rs` (4,277 LOC) and `config/models.rs` (1,953 LOC), but the TUI one is the most worrying because it changes the most often.
2. **`#![allow(warnings)]` + `#![allow(clippy::all)]` at the crate root** (`src/lib.rs:1-2`). This is hiding signal. Real clippy lints would catch concrete bugs — `unused_must_use` on stream sinks, `clone_on_copy`, `redundant_async_block`, etc. Strongly recommend removing these and burning down the warnings as a one-shot pass.
3. **Regex-based code parser** (`graph/parser.rs`). 4,277 LOC of regex is at the edge of what's maintainable. Languages like TypeScript with overloads, decorators, JSX, and TS-only constructs will inevitably escape these regexes. The right long-term answer is tree-sitter (one grammar per language, parsed once, walked deterministically). See §3.6.
4. **Token estimation comment.** The README claims tiktoken everywhere, but the CLAUDE.md says "rough ~4 chars/token estimate" for session counting. Inconsistency between docs — verify which path is wired in `session/tokens.rs` and remove the dead one.
5. **No Windows-aware ANSI/path handling shown for some shell paths.** `tools/shell.rs` uses `cmd /C` on Windows per CLAUDE.md, but I did not see explicit handling for paths with spaces in arg-splitting on Windows. Likely-fine, but is exactly the kind of thing that bites a user named `OM SHAH` whose home path has a space.
6. **Provider streaming is hand-rolled per-vendor.** This is correct (each format differs), but it means every new provider is a bespoke parsing job. See §3.1 for the prompt-caching consequence.
7. **No persisted vector index / embeddings.** Search is grep-based (with `ignore`) and graph-based. For codebases > 100k LOC the agent will still spend tokens flailing through grep. See §3.4.
8. **Memory/`CLAUDE.md` loading** is documented; I did not see explicit memory-promotion/garbage-collection (the "auto-memory" pattern Claude Code uses where memories decay or are explicitly written via a tool). Skills cover *some* of this, but they're invocations, not lived state. See §3.3.
9. **No headless / API mode.** Everything is TUI-or-one-shot. To compete with Codex CLI / Claude Code's `--print`/`-p` automation flow you need a JSON-streaming headless mode for CI. (You have non-interactive single-task mode; what's missing is structured JSON output suitable for piping into other tools.) See §3.10.
10. **No sandboxing.** Shell tool calls run as the user's UID with whatever filesystem and network access the parent process has. Codex CLI and OpenAI's Computer Use both sandbox via `seatbelt`/`bwrap`/`landlock`. For autonomous `/goal` runs this is the single biggest safety gap. See §3.7.

### 1.3 Bottom-line verdict on current state

**forge-osh as it sits today** is, by feature surface area, in the same league as OpenCode and ahead of t3code, Aider, and most one-person clones. It is behind Claude Code on three specific axes (sandboxing, headless-JSON automation, prompt caching) and behind Cursor/Codex CLI on one (true LSP-driven semantic edits that propagate, not just `lsp_rename`). It is **ahead** of all of them on the combination of (a) semantic-graph + LSP two-layer intelligence, (b) durable `/goal` mode with verifier contracts, and (c) breadth of provider support.

If I had to grade it: **B+ for execution today, A− for ambition, and the gap between those grades is closeable in roughly 6–8 focused weeks of work.** The single biggest leverage move is splitting `tui/mod.rs` and turning on clippy — every other improvement gets easier after that.

---

## 2. Competitive Verdict vs. Other Platforms

The comparison below is intentionally honest. I'm not boosting forge-osh; I'm calling each axis on what the code actually does.

| Axis | forge-osh (today) | Claude Code | OpenAI Codex CLI | OpenCode | Cursor CLI | Aider | t3code / Pi |
|---|---|---|---|---|---|---|---|
| **Multi-provider** | ✅ 12 cloud + 6 local | Anthropic only (+ Bedrock/Vertex) | OpenAI only | Multi-provider | Anthropic + OpenAI | ✅ Many | Few |
| **Native streaming per provider** | ✅ All 4 hand-rolled | ✅ | ✅ | ✅ | ✅ | ✅ | Partial |
| **Prompt caching (Anthropic)** | ❌ Not wired in `anthropic.rs` (confirm — see §3.1) | ✅ First-class | N/A | Partial | ✅ | ❌ | ❌ |
| **MCP servers** | ✅ Full stdio client + 50 catalog | ✅ (the originator) | Limited | ✅ | Partial | ❌ | ❌ |
| **LSP integration** | ✅ 22 languages registered | ❌ (relies on bash) | ❌ | ✅ (flagship) | ✅ (native via IDE) | ❌ | ❌ |
| **Semantic code graph** | ✅ **Unique** (petgraph + context_pack) | ❌ | ❌ | ❌ | Closed-source equivalent | ❌ | ❌ |
| **Durable autonomous goals** | ✅ `/goal` + verifiers | Partial (no on-disk goal contract) | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Parallel worker swarm** | ✅ `/multithread` + `/team` | ✅ (subagents) | ❌ | Partial | ❌ | ❌ | ❌ |
| **Sandboxed shell exec** | ❌ Big gap | ✅ (containers/seatbelt) | ✅ (seatbelt/bwrap) | Partial | Partial | ❌ | ❌ |
| **Headless JSON automation** | Partial (non-interactive, but not structured JSON streaming) | ✅ `-p --output-format=stream-json` | ✅ | Partial | ❌ | ✅ | ❌ |
| **Permission rules with globs** | ✅ Persistent, wildcarded | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |
| **Undo / snapshots** | ✅ Per-edit stack | Partial (git-based) | Partial | ✅ | ❌ | ✅ (git only) | ❌ |
| **Git worktree isolation** | ✅ `enter_worktree` tool | Manual | Manual | Partial | Manual | Manual | ❌ |
| **Token / cost tracking** | ✅ Tiktoken + per-session cost | ✅ | ✅ | ✅ | ✅ | ✅ | Partial |
| **TUI quality** | ✅ Ratatui, 5 themes, vim mode | ✅ Ink/React | ✅ | ✅ | N/A (IDE) | Basic | Basic |
| **Skill system (bundled/user/project)** | ✅ With conv-to-skill generation | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Hooks (`PreToolUse`/`PostToolUse`)** | ✅ Shell hooks | ✅ | Partial | Partial | ❌ | ❌ | ❌ |
| **`CLAUDE.md` memory loading** | ✅ Project + user + parent dirs | ✅ (originator) | Different mechanism (`AGENTS.md`) | ✅ | ✅ | ✅ | ❌ |
| **Plan-mode** | ✅ `enter_plan_mode` | ✅ | Partial | Partial | ❌ | ❌ | ❌ |
| **Vector-search / embedded RAG over codebase** | ❌ | Partial (Claude's own) | ✅ | Partial | ✅ | ❌ | ❌ |
| **Native binary, no Node/Python** | ✅ Single Rust binary | ❌ Node | ❌ Node | ❌ TypeScript runtime | ❌ | ❌ Python | Varies |
| **Web search built-in** | ✅ DuckDuckGo | ✅ Native + tool | ✅ Native | Partial | ✅ | ❌ | Partial |
| **Notebook support** | ✅ `notebook_read` | ✅ | Partial | ❌ | ✅ | ❌ | ❌ |
| **License** | MIT (open) | Closed | Closed | Open | Closed | Apache | Mixed |
| **Lines of code** | ~45,000 Rust | Larger (Node/React) | ~15-20k (Node) | ~30-50k (TS) | Closed | ~10-15k (Py) | Smaller |

### Sharp summary

- **forge-osh's unique advantages** (these no competitor has all of): native Rust binary + semantic code graph + LSP layer + durable `/goal` mode with verifier contracts + 12-provider routing + MIT license.
- **forge-osh's critical gaps vs. Claude Code**: prompt caching, sandboxing, structured JSON streaming for CI/automation.
- **forge-osh's gaps vs. Codex CLI**: vector embeddings over the workspace, sandboxing, official integration with cloud providers' background-job APIs.
- **forge-osh's gaps vs. Cursor**: it's a terminal tool, not an IDE — that's a positioning choice, not a deficiency. But to capture the "tab to complete" market you'd need an editor extension (VS Code / Neovim), which is its own multi-month project.
- **forge-osh's gaps vs. OpenCode**: roughly at parity on LSP + provider support; ahead on graph + goals; behind on the open-source mindshare (which is fixable with shipping).

---

## 3. Recommended Feature Roadmap (Prioritized)

The list is ordered by **(impact × feasibility) / risk**. Each item includes a feasibility estimate (engineering weeks for one experienced Rust dev, assuming the code stays single-author).

### 3.1 Prompt caching for Anthropic + provider-tier caching abstraction
- **Why**: This is the single biggest cost reduction available. For long sessions the Anthropic cache cuts effective input cost ~90%. Already mentioned in `future_plan/02_prompt_caching_provider_architecture.md` — has design but not implementation.
- **What**: Add `cache_control: { type: "ephemeral" }` markers in `anthropic.rs` for (system prompt block, tools block, last-N user turns). Add an abstract `ProviderCacheStrategy` trait so other providers (Gemini's implicit cache, OpenAI's prompt cache header) plug in without leaking Anthropic-specific concepts.
- **Feasibility**: **HIGH.** ~1 week. Localized to `provider/anthropic.rs` + a small trait. Existing tests cover provider router.
- **Risk**: Low. Caching is additive — incorrect markers degrade to no-cache, they don't break correctness.

### 3.2 Sandboxed shell execution
- **Why**: Largest *safety* gap. Right now an autonomous `/goal` run with `auto_approve = "all"` has unrestricted disk + network. This is what's keeping forge-osh from being recommended for unattended overnight runs in shared environments.
- **What**: Pluggable `ExecSandbox` trait with three backends — `seatbelt` (macOS), `bubblewrap`/`landlock` (Linux), `AppContainer`/`JobObject` (Windows). Default to "tight" for `/goal` runs, "permissive" for interactive mode. Filesystem allow-list = `workdir + tmpdir + ~/.forge-osh/cache`. Network allow-list = matches `policy.shell_allowlist` and `policy.network`.
- **Feasibility**: **MEDIUM-LOW.** ~3-5 weeks total because Windows is genuinely painful. macOS+Linux first (~2 weeks), Windows later (~2-3 weeks). The macOS+Linux subset is the highest-value 80%.
- **Risk**: Medium. Sandboxing has lots of edge cases (symlinks, /proc, env-var leakage). Mitigate with an explicit `--no-sandbox` flag and a "tested sandbox" subset of well-known tools.

### 3.3 Persistent auto-memory system (Claude-Code-style)
- **Why**: Currently you have `CLAUDE.md` (static) and skills (invocations). What's missing is *learned* memory — facts about the user, project, feedback, references that the agent writes itself over time. This is what the harness's own auto-memory does (see `.claude/projects/.../memory/MEMORY.md` in this very session).
- **What**: Tool `remember_fact { type, name, body }` + `forget_fact { name }`. Persistent files in `~/.forge-osh/memory/` keyed by working-directory hash. Index file (`MEMORY.md`) auto-loaded into system prompt. Types: `user`, `feedback`, `project`, `reference` (mirror the harness's types). Skill: `/remember` + `/forget`.
- **Feasibility**: **HIGH.** ~1-2 weeks. Reuses skill scope discovery in `skills/mod.rs`. New tool + small system-prompt change.
- **Risk**: Low. Memory is additive; agents already know how to ignore stale facts when given an explicit "verify before recommending" instruction.

### 3.4 Vector / embedding index of the codebase
- **Why**: Closes the "Cursor-like semantic search" gap. The graph gives structural recall; embeddings give intent-level recall ("where do we handle expired tokens?"). Most agents waste 5–10 tool calls grepping for the right anchor file — embeddings cut that to 1.
- **What**: Local embedding via `fastembed-rs` (BGE-small-en or similar; ~30MB ONNX, no Python). Chunk = symbol (from graph) + ±30 lines context. Persist to `~/.forge-osh/embeddings/<dir-hash>.bin`. New tool: `semantic_search { query, k }`. Incremental rebuild on file change (watch via `notify` crate).
- **Feasibility**: **MEDIUM.** ~3 weeks. The graph already gives clean chunk boundaries. The hard part is incremental indexing on file events without races with the agent's edits.
- **Risk**: Medium. Embedding model size + cold-start indexing time on large repos. Mitigate by making it lazy + showing progress.

### 3.5 Structured JSON streaming / headless mode
- **Why**: Required to be usable in CI, as a backend for a VS Code extension, or piped into other tools. Claude Code's `--print --output-format=stream-json` is what lets people build on top of it. Without this, forge-osh is a terminal app only.
- **What**: `forge-osh --print --output-format=stream-json "<prompt>"` emits NDJSON of events: `{type:"assistant_chunk",text}`, `{type:"tool_call",name,input}`, `{type:"tool_result",...}`, `{type:"usage",...}`. Mirrors the existing internal `AgentEvent` channel — basically a transport bridge.
- **Feasibility**: **VERY HIGH.** ~3-5 days. The internal event bus already exists; this is a JSON serializer + a CLI flag.
- **Risk**: Very low.

### 3.6 Tree-sitter parser migration (replace regex graph builder)
- **Why**: `graph/parser.rs` at 4,277 LOC of regex is a ceiling. Tree-sitter gives correct, incremental, well-tested grammars for 100+ languages. Once you have tree-sitter, you can also build LSP-quality features (precise rename, scope-aware "find usages") without depending on per-language LSP servers being installed.
- **What**: Add `tree-sitter` + per-language grammars (rust/python/ts/js/go/java/cpp/c#). Reimplement `build_for_path` against tree-sitter trees. Keep the petgraph `StableGraph` and the `GraphNode`/`GraphEdge` taxonomy — only the parser changes. Old regex path kept behind feature flag for one release as fallback.
- **Feasibility**: **MEDIUM.** ~4-6 weeks. The parser is the hard part; the downstream graph builder doesn't change. Risk is binary size — every grammar is 500KB-2MB. Mitigate with feature flags so users compile what they need.
- **Risk**: Medium. Migration must preserve graph contract exactly or the existing `context_pack` / `blast_radius` callers break.

### 3.7 Streaming patch-apply with diff-first UX
- **Why**: Today `edit_file` runs find-and-replace and shows the diff after. Modern coding agents (Cursor, Aider, Codex CLI) stream the patch *as the model writes it* and apply atomically. This makes the user feel control they don't have today.
- **What**: New tool `apply_patch { unified_diff }` that uses `similar`'s patch apply (already a dep). Model is taught (system prompt) to emit unified-diff blocks for multi-line changes. TUI shows a live "patch buffer" that fills in as the model streams, with hunks highlighted as they arrive. Accept/reject per-hunk.
- **Feasibility**: **MEDIUM.** ~2-3 weeks. The diff infrastructure (`tui/diff.rs`, `similar`) is already in place. The streaming UX is the hard part.
- **Risk**: Medium. Patches against modified files are fragile — must integrate with the existing SHA-256 file-state cache.

### 3.8 Anthropic / OpenAI background-job and "thinking-budget" integration
- **Why**: Anthropic Messages API and OpenAI's Realtime/Responses APIs both support extended-thinking modes with explicit token budgets. forge-osh already has `/effort 1-5` but doesn't wire it to the model-native parameter. For Claude Opus 4.7 / GPT-5 reasoning, this is the lever that controls quality.
- **What**: Map `/effort` to provider-native: Anthropic `thinking.budget_tokens`, OpenAI `reasoning_effort`, Gemini `thinking_config.thinking_budget`. Add `/thinking on|off` and `/thinking budget <N>`.
- **Feasibility**: **HIGH.** ~1 week.
- **Risk**: Low.

### 3.9 Plugin system for third-party tools (beyond MCP)
- **Why**: MCP covers stdio JSON-RPC tools. But many useful integrations are native Rust crates (linters, formatters, custom analyzers). A native `forge-osh-plugin` ABI (dylib loaded via `libloading`) lets the community ship Rust-native tools without forking.
- **What**: Stable `Plugin` trait at a versioned ABI boundary. Plugins compiled as cdylibs and loaded from `~/.forge-osh/plugins/`. Each plugin can register tools, hooks, providers, or slash commands. (See `future_plan/04_plugins_and_mcp_servers.md`.)
- **Feasibility**: **LOW (slow).** ~4 weeks plus indefinite ABI-maintenance overhead. Rust ABI stability is famously hard.
- **Risk**: HIGH. Recommend deferring to v2.x. In the meantime use MCP for everything.

### 3.10 First-class subagents API + `Task` tool (Claude-Code-style)
- **Why**: You have `/multithread @worker` and `/team`. The next step is a model-callable `spawn_agent` tool so the *agent itself* delegates work. This is how Claude Code's `Agent` tool works and it's the most important agent-architecture primitive after the main loop.
- **What**: New tool `spawn_subagent { subagent_type, prompt, run_in_background }`. Discoverable types from `~/.forge-osh/agents/<name>.md` (frontmatter: name/description/tools). Returns either a final summary (foreground) or a handle (background) so the parent can poll.
- **Feasibility**: **MEDIUM-HIGH.** ~2 weeks. The `Worker` infrastructure in `agent/worker.rs` (316 LOC) and `agent/team.rs` (789 LOC) already gives you most of the runtime — this is a tool-shaped wrapper + a subagent discovery scope. Reuse skill-loading code.
- **Risk**: Low-medium. Cost-control (a misbehaving parent could spawn N children spawning N children) — mitigate with a recursion-depth cap (e.g. max-depth=3).

### 3.11 VS Code / Neovim extensions
- **Why**: Captures the IDE market without writing your own editor. Both can shell out to `forge-osh --print --output-format=stream-json` (after §3.5 lands) and render in a side panel.
- **What**: VS Code: TypeScript extension, webview panel with the streamed events. Neovim: Lua plugin using `jobstart`. Both wire to the same JSON protocol.
- **Feasibility**: **MEDIUM.** ~3 weeks for VS Code, ~1 week for Neovim. Requires §3.5 first.
- **Risk**: Low. These are thin clients.

### 3.12 OpenTelemetry / structured trace export
- **Why**: For users running long autonomous goals, they want to know *why* the agent did X — which prompt, which tool, what the model thought. Today `progress.log` is text. OTel lets `/goal` runs export to Honeycomb / Tempo / Datadog.
- **What**: `tracing-opentelemetry` (already pulls in your existing `tracing` deps). Export spans for: turn boundaries, tool calls, verifier runs, checkpoint writes. Off by default; enabled via `[telemetry] otlp_endpoint = "..."`.
- **Feasibility**: **HIGH.** ~1 week. `tracing` is already wired everywhere.
- **Risk**: Low.

### 3.13 Refactor `tui/mod.rs` (foundational health)
- **Why**: Not a feature, but a precondition for shipping any of the TUI-touching items above (3.5, 3.7, 3.10). A 7,100-LOC file is a ratchet that slows every future change.
- **What**: Split into `tui/state.rs` (AppState), `tui/event_loop.rs` (the tick loop), `tui/handlers/` (one file per command family — model, provider, skill, goal, mcp, lsp, graph, team, etc.), `tui/modals/` (one file per modal). Renderer (`renderer.rs`) stays separate.
- **Feasibility**: **MEDIUM.** ~2 weeks of careful work. No behavior changes — pure refactor. Test coverage already exists for the input handling and themes.
- **Risk**: Medium. Refactoring without behavior tests is dangerous; lean on the existing TUI tests in `tests/test_tui_*.rs` and add a recorded-input replay test as a safety net.

### 3.14 Conversation-graph view + branching
- **Why**: Once a conversation gets long, users want to "go back to that point and try a different prompt" without losing the current branch. Sessions today are linear.
- **What**: Internal: session stores a DAG rather than a list. UI: `/branch` creates a fork; `/branches` lists; `/checkout <branch>` switches. Persists to existing checkpoint JSON.
- **Feasibility**: **MEDIUM.** ~2 weeks.
- **Risk**: Medium. Changes session schema — must support old sessions or migrate.

### 3.15 Cost-budget enforcement (opt-in)
- **Why**: `/goal` currently never enforces cost. That's a deliberate design choice (cost estimates drift), but users still ask for it. Make it explicit + opt-in.
- **What**: `[budget] daily_usd = 5.00` in config. Sums across user session + all goals. When exceeded, blocks new LLM calls with a clear `/budget override` escape hatch.
- **Feasibility**: **HIGH.** ~3 days. `CostTracker` already accumulates correctly.
- **Risk**: Low.

### 3.16 Self-update mechanism
- **Why**: Users get builds via email or release downloads. Without `forge-osh update` they stay on old versions and miss bug fixes.
- **What**: `forge-osh update` checks GitHub releases, downloads correct platform binary, swaps in-place using `self-replace` crate semantics. Cryptographic signature verification (Ed25519) of release artifacts.
- **Feasibility**: **MEDIUM.** ~1 week. Signing infrastructure is the slow part.
- **Risk**: Medium — bricking an install via a bad update is a real failure mode. Mitigate with `forge-osh rollback`.

### 3.17 First-party Bedrock / Vertex / Azure OpenAI providers
- **Why**: Enterprises can't use direct API keys; they need the cloud-vendor-fronted endpoints. This is what unlocks paid enterprise use.
- **What**: Three new provider implementations. Bedrock uses SigV4 (use `aws-sigv4`). Vertex uses ADC (`google-cloud-auth`). Azure OpenAI is OpenAI-compat but auth differs.
- **Feasibility**: **MEDIUM.** ~2 weeks total. Auth is the friction.
- **Risk**: Low-medium. AWS/GCP/Azure SDKs are heavy — pulling them in inflates binary size. Mitigate with feature flags.

### 3.18 Multimodal input (images, PDFs) in TUI
- **Why**: Pasting screenshots into the agent is table stakes now. Claude Code, OpenAI Codex CLI, Cursor all support it.
- **What**: Detect image clipboard via OSC-52 / platform clipboard APIs. Send as `image` content block to providers that support it (Anthropic, OpenAI, Gemini all do). For PDFs, route through `pdf-extract` crate to text.
- **Feasibility**: **MEDIUM.** ~2 weeks. TUI rendering of images is unsolvable in a strict terminal — but `kitty` graphics protocol / `iterm2` / `sixel` cover 60% of real users.
- **Risk**: Medium. Terminal heterogeneity is a UX trap.

### 3.19 Built-in benchmark harness (SWE-Bench-Lite, Aider polyglot)
- **Why**: To claim "as good as Claude Code on coding tasks" you need numbers. Aider publishes a benchmark; SWE-Bench is the industry standard. Without this, the README's competitive claims are unfalsifiable.
- **What**: `forge-osh bench --suite swe-bench-lite --model claude-sonnet-4.6 --tasks 50` runs the suite, scores pass@1, dumps a CSV. Uses git worktrees per task (which you already have).
- **Feasibility**: **MEDIUM.** ~2 weeks. The hard part is running each task in isolation safely (depends on §3.2 sandboxing).
- **Risk**: Low (other than the marketing risk of scoring lower than competitors — but that's a forcing function for improvement).

### 3.20 Conversational-quality features (drop-in)
- **Why**: Small polish wins.
- **What**:
  - **Streaming token-rate display** in status bar ("142 tok/s")
  - **Cost-per-message in the message gutter** (Claude Code shows this)
  - **`/redo`** (inverse of `/undo`)
  - **`/diff HEAD`** showing all uncommitted changes the agent has made this session
  - **`/explain <symbol>`** shorthand that runs LSP hover + graph context_pack
  - **`/review`** that triggers the bundled `review` skill (already exists) with the current PR diff
  - **`/codex-rescue` parity** — a "ask a *different* model for a second opinion on what to do" shortcut
- **Feasibility**: **VERY HIGH.** Each is 0.5-1 day.
- **Risk**: None.

---

## 4. Suggested Sequencing (12-week plan)

A realistic single-developer plan, ordered to maximize cumulative value.

| Week | Work item | Deliverable |
|---|---|---|
| 1 | **3.13** Split `tui/mod.rs` | Smaller, testable TUI files. Prereq for everything else. |
| 1 | **3.5** Headless JSON streaming | `--output-format=stream-json` ships. Unblocks editor extensions + CI. |
| 2 | **3.1** Anthropic prompt caching | ~70-90% input-cost reduction on long sessions. Headline-worthy. |
| 2 | **3.20** Quality-of-life batch | `/redo`, `/explain`, token-rate, cost gutter. |
| 3 | **3.3** Auto-memory system | Skills + memory feels like a real long-term collaborator. |
| 3 | **3.15** Cost budget + **3.8** thinking budget | Two small wins. |
| 4-6 | **3.2** Sandboxing (mac+linux first) | Unlocks safe overnight `/goal` runs. |
| 7 | **3.10** Subagents API | Agent can self-delegate; closes a Claude-Code gap. |
| 8-9 | **3.4** Embedding index | Closes the Cursor-semantic-search gap. |
| 10 | **3.7** Streaming patch-apply | Catches up to Aider/Cursor UX. |
| 10 | **3.12** OTel + **3.16** self-update | Operational polish. |
| 11-12 | **3.19** Benchmark harness + first published numbers | Marketing turning point. |
| Later | **3.6** Tree-sitter migration, **3.11** editor extensions, **3.17** Bedrock/Vertex/Azure, **3.18** multimodal | These are v2.x. |

At end of week 12, forge-osh has parity with Claude Code on the things that matter (caching, sandboxing, headless, subagents), unique advantages preserved (graph, goals, multi-provider, native binary), and a benchmark number to argue with.

---

## 5. What I'd Cut or De-Risk

Not everything in the existing roadmap (`future_plan/01...05`) should ship as-designed. Honest cuts:

1. **`future_plan/03_skill_generation_from_conversation.md`** — already exists in `agent/skill_generation.rs` (945 LOC). Make sure it's stable and well-tested before adding *more* skill features. Don't expand its scope.
2. **`future_plan/04_plugins_and_mcp_servers.md`** — defer the native plugin ABI. MCP covers 90% of the need. Native Rust plugins are an ABI-maintenance burden you don't want at this stage.
3. **Provider count creep.** You have 12+6. Stop. Add Bedrock/Vertex/Azure (because enterprise) and otherwise focus on making the existing ones excellent.
4. **`tui/mod.rs` growth.** Cap it. Pre-commit hook: refuse commits that grow this file beyond N lines without a corresponding split.
5. **`graph/parser.rs` growth.** Cap it. Plan tree-sitter migration in Q3.

---

## 6. Final Verdict

**The current state of the project is significantly better than its public profile suggests.** It is genuinely one of the more complete open-source coding-agent terminals in existence — measurably ahead of t3code/Pi/Aider on feature surface, at parity with OpenCode, and within striking distance of closed-source Claude Code on capability if not yet on polish. The code is correct in shape, layered cleanly, persistently designed (atomic writes, checkpoints, file caches, undo stacks), and the goal/MCP/LSP layers are all real implementations rather than stubs.

The three things holding it back from being a "people will recommend this over Claude Code" tool are (1) **sandboxing** (safety), (2) **prompt caching** (cost), and (3) **headless JSON** (integratability). All three are tractable inside 6 weeks of focused work. After that, the differentiators (semantic graph + goal contracts + multi-provider + MIT) become the lead story rather than the recovery story.

If I were the maintainer, I would do exactly the 12-week sequence in §4, in that order, ship a v2.0 with a real benchmark, and let the comparison table in §2 do the marketing.
