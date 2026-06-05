# forge-osh — Comprehensive Improvements & Competitive Architecture Plan

> Status: planning document. Nothing here is implemented yet unless cross-linked
> to an existing module. This is a decision-making aid: it audits what forge-osh
> already ships, compares it feature-by-feature against the leading terminal /
> agentic coding tools, and proposes concrete, module-grounded improvements with
> effort, risk, and sequencing so we can make good architecture calls.

Last updated: 2026-06-05 · Target baseline: forge-osh v1.0.22

---

## 1. Methodology

- **Baseline** taken from the actual source tree (`src/…`), not the README, so we
  never re-propose something that already exists.
- **Competitors surveyed:** Claude Code (Anthropic), Codex CLI (OpenAI),
  opencode (SST), T3 Chat / "t3 code" (theo), Cursor (agent + CLI), Antigravity
  CLI (Google), GitHub Copilot CLI. Features attributed to a competitor reflect
  their publicly documented behaviour at time of writing; treat as directional.
- Each gap is rated **P0** (table-stakes / users notice it missing), **P1**
  (strong differentiator, expected by power users), **P2** (valuable, not
  urgent), **P3** (nice-to-have / niche).
- Effort is **S** (≤2 days), **M** (≤1 week), **L** (≤1 month), **XL** (multi-month
  / architectural).

---

## 2. What forge-osh already ships (do NOT re-propose)

This is genuinely strong; the gaps below are mostly polish, modality, and
ecosystem — not missing fundamentals.

**Core agent loop & providers**
- Provider-agnostic router (`src/provider/`): Anthropic (native), OpenAI-compat
  (10+ providers via one impl), Gemini (native), Ollama. Manual SSE parsing.
- Prompt caching across Anthropic + OpenRouter (`future_plan/02_…`).
- Streaming tokens, extended-thinking config, retries/backoff.

**Tooling** (`src/tools/`): file I/O (read/write/edit/create/delete/move/copy),
`bash`, `powershell`, 15 git tools, `search_files`/`find_files` (ignore-aware),
`web_fetch`/`web_search`, `run_linter`/`run_tests`/`run_formatter`,
`notebook_read`, worktrees, skills, the live planner (`update_plan`),
session tasks, `ask_user`, plan-mode, and the new team tools (`team_post`,
`team_read`, `spawn_team`).

**Code intelligence**
- Semantic code graph (`src/graph/`): AST-level `graph_query` (find, context_pack,
  blast_radius, file_graph, mutations, stats).
- LSP integration (`src/lsp/`): diagnostics/definition/references/hover/symbols/
  rename across ~20 languages, bundled + auto-provisioned servers.

**Autonomy & orchestration**
- `/goal` (`src/agent/goal/`): durable, verifiable, autonomous objectives with
  checkpoints, verifiers (shell/file/git-clean), policy gating, budgets, a
  cold-start resumer, and an interactive goal-manager modal.
- Agent Teams + Coordinator + Worker (`src/agent/{team,coordinator,worker}.rs`):
  multithread mode, **orchestrator vs swarm** modes, a live shared **blackboard**
  message bus, and model-callable `spawn_team` for dynamic sub-team orchestration.
- Anti-reward-hacking integrity contract in `/goal`.

**Ecosystem & persistence**
- Skills (`src/skills/`): bundled/user/project, inline + fork execution, model
  override, allowed-tool scoping, hooks, generation-from-conversation.
- MCP client (`src/mcp/`): catalog, manager, stdio transport, tool adapter.
- Sessions (`src/session/`): history, JSON checkpoints, file-state cache (stale-
  edit guard), token/cost tracking, compaction/auto-summarize.
- Hooks (`src/agent/hooks.rs`): UserPromptSubmit, Pre/PostToolUse, Stop,
  SessionStart/End, PreCompact.
- Permissions (`src/agent/permissions.rs`): modes (Default/Plan/AcceptEdits/
  Bypass) + stored `tool(pattern)` allow/deny rules.

**Surfaces**
- ratatui TUI (`src/tui/`): Molten-Rust theme system (6 fluid themes), rounded
  modals, pickers, live plan panel, diff review, vim mode, goal manager.
- JSON-RPC `stream-json` bridge (`src/jsonrpc/`) + VS Code extension.
- CLI non-interactive mode; CLAUDE.md memory loading.

---

## 3. Competitor profiles (standout capabilities)

| Tool | Standout strengths relevant to us |
|---|---|
| **Claude Code** | Named **subagents** (`.claude/agents/*.md`), **custom slash commands** (`.claude/commands/*.md`), **image/paste-screenshot** input, **background Bash** + monitoring, queued/steerable input, **output styles** & statusline customization, checkpoint **rewind**, mature hooks, headless `-p`/stream-json, sandboxed bash (OS perms). |
| **Codex CLI** | **OS-level sandboxing** (Seatbelt on macOS, Landlock/seccomp on Linux), approval policies, `AGENTS.md`, multimodal (screenshots), config **profiles**, Codex **cloud** task offload, apply-patch diff protocol. |
| **opencode** | **Client/server architecture** (headless core + multiple front-ends), **session sharing via URL**, multiple concurrent sessions, rich theming, custom agents/modes, strong LSP, fully provider-agnostic. |
| **T3 Chat** | Instant **multi-model switching**, BYOK, **cloud sync across devices**, **chat branching**, image generation, very low latency UX. |
| **Cursor** | **Vector/embedding codebase index** (semantic retrieval), `@`-symbol mentions (files/docs/web/git), **background agents**, Bugbot/PR review, `.cursor/rules`, multi-root workspaces, apply-in-place edits. |
| **Antigravity CLI** | **Agent Manager / mission-control** over many parallel agents, **browser/computer use**, artifacts (task lists, walkthroughs, screenshots), deep long-context Gemini integration, cross-task knowledge base. |
| **Copilot CLI** | Native **GitHub** integration (issues/PRs/Actions context), `suggest`/`explain`, MCP, enterprise policy controls, CI-friendly. |

---

## 4. Feature gap matrix

Legend: ✅ has · ⚠️ partial · ❌ missing.

| Capability | forge-osh | Claude Code | Codex | opencode | Cursor | Antigravity | Copilot CLI |
|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| Provider-agnostic / BYOK | ✅ | ⚠️ | ⚠️ | ✅ | ⚠️ | ❌ | ⚠️ |
| Prompt caching | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Image / screenshot input** | ❌ | ✅ | ✅ | ⚠️ | ✅ | ✅ | ⚠️ |
| MCP client | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| LSP integration | ✅ | ❌ | ❌ | ✅ | ✅ | ⚠️ | ❌ |
| AST/semantic code graph | ✅ | ❌ | ❌ | ⚠️ | ❌ | ⚠️ | ❌ |
| **Vector/embedding retrieval** | ❌ | ⚠️ | ⚠️ | ❌ | ✅ | ✅ | ⚠️ |
| **`@`-mention file/symbol autocomplete** | ❌ | ✅ | ⚠️ | ✅ | ✅ | ✅ | ⚠️ |
| **Custom user slash commands** | ⚠️(skills) | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ | ⚠️ |
| **Named subagent definitions** | ⚠️(skills/teams) | ✅ | ❌ | ✅ | ⚠️ | ✅ | ❌ |
| Multi-agent parallelism | ✅ | ✅ | ❌ | ⚠️ | ✅ | ✅ | ❌ |
| Durable autonomous goals | ✅ | ⚠️ | ⚠️ | ❌ | ⚠️ | ✅ | ❌ |
| **OS-level sandboxing** | ❌ | ⚠️ | ✅ | ❌ | ⚠️ | ⚠️ | ⚠️ |
| **Background process mgmt** | ⚠️(live stream) | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ |
| Hooks | ✅ | ✅ | ⚠️ | ✅ | ❌ | ⚠️ | ❌ |
| Checkpoints / undo | ✅ | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ❌ |
| **Rewind/time-travel UX** | ⚠️ | ✅ | ❌ | ⚠️ | ✅ | ✅ | ❌ |
| **Client/server + session sharing** | ❌ | ⚠️ | ❌ | ✅ | ⚠️ | ✅ | ❌ |
| IDE integration | ✅(VS Code) | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ✅ |
| Headless / stream-json | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| **Native GitHub/GitLab (PR/issues)** | ❌ | ⚠️ | ⚠️ | ❌ | ✅ | ⚠️ | ✅ |
| **Browser / computer use** | ⚠️(via MCP) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ✅ | ⚠️ |
| **Rules-file interop (AGENTS/.cursorrules)** | ❌ | ⚠️ | ✅ | ⚠️ | ✅ | ⚠️ | ✅ |
| **Long-term / vector memory** | ⚠️(CLAUDE.md) | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ |
| **Observability / OTel traces** | ❌ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ✅ |
| **Eval / benchmark harness** | ⚠️ | ⚠️ | ⚠️ | ❌ | ⚠️ | ⚠️ | ❌ |
| **Spend caps / budget enforcement** | ⚠️(observe) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ✅ |
| **Auto model routing / cascades** | ❌ | ❌ | ❌ | ⚠️ | ✅ | ✅ | ⚠️ |
| Statusline / output styles | ⚠️ | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ | ❌ |
| Desktop/push notifications | ❌ | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ❌ |

---

## 5. Prioritized improvement backlog

### P0 — table stakes we're missing

#### P0.1 — Multimodal (image / screenshot) input · **L**
- **Gap:** `supports_vision` exists only as a catalog flag; there is no image
  content block, so users cannot paste a screenshot or attach a diagram.
  Every major competitor accepts images.
- **Architecture:**
  - Add `ContentBlock::Image { media_type, data /* base64 */, source: Url|File }`
    to `src/types.rs`; thread through `Message`/`ChatRequest`.
  - Encode per provider: Anthropic `image` blocks, Gemini `inlineData`,
    OpenAI-compat `image_url` (data URI). Gate on the model's `supports_vision`;
    degrade with a clear message otherwise.
  - TUI: accept image paste (terminal image protocol detection), `@path/to.png`,
    and a `/image <path>` command; show a `[image: name]` chip in the transcript.
  - JSON-RPC: add an `image` field to inbound user messages.
- **Risk:** terminal image paste is inconsistent; start with `@file`/`/image`
  path attach + clipboard-image-to-temp on Windows/macOS/Linux.
- **Acceptance:** attach a PNG, ask "what's wrong in this screenshot", get a
  vision answer on a vision-capable model; non-vision models refuse cleanly.

#### P0.2 — `@`-mention autocomplete (files / symbols) · **M**
- **Gap:** no inline fuzzy file/symbol picker; users type full paths.
- **Architecture:** in `src/tui/input.rs`, detect `@` and open an inline
  fuzzy-finder backed by `find_files` + the code graph (`file_graph`/symbol
  index). On select, insert a canonical reference and pre-load the file into
  context (or mark it for the next turn). Support `@file`, `@symbol`, `@diff`,
  `@url`.
- **Acceptance:** `@ren` → picks `renderer.rs`; the file content is available to
  the model without a manual `read_file`.

#### P0.3 — Custom user slash commands · **M**
- **Gap:** only skills exist; competitors let users drop `.claude/commands/x.md`
  (or `.forge-osh/commands/`) that expand into a prompt with `$ARGUMENTS`.
- **Architecture:** loader scanning `./.forge-osh/commands/`,
  `~/.forge-osh/commands/`, and `.claude/commands/` for markdown command files
  (frontmatter: `description`, `allowed_tools`, `model`). Register them
  dynamically in the slash-command dispatcher (`src/tui/mod.rs`) and surface in
  `/help` + autocomplete. Reuse the skills materialization path.
- **Acceptance:** a `commands/pr.md` makes `/pr fix login` expand and run.

#### P0.4 — Rules-file interoperability · **S**
- **Gap:** only CLAUDE.md is read. Importing repos already carry `AGENTS.md`,
  `.cursor/rules`, `.cursorrules`, `.github/copilot-instructions.md`.
- **Architecture:** extend `system_prompt::load_memory_files` to also load these
  (clearly labelled, precedence documented). Pure additive.
- **Acceptance:** a repo with `AGENTS.md` has its rules in the system prompt.

### P1 — strong differentiators

#### P1.1 — OS-level execution sandboxing · **XL**
- **Gap:** `bash`/`powershell` run with the user's full privileges; only
  permission prompts gate them. Codex sandboxes by default.
- **Architecture:** a `Sandbox` trait in `src/tools/` with backends: macOS
  Seatbelt (`sandbox-exec`), Linux Landlock + seccomp (or `bwrap`), Windows
  Job Objects / restricted tokens, and an optional container backend (Docker).
  Policy: filesystem write-allowlist (reuse goal `write_globs`/`deny_globs`),
  network on/off, CPU/mem/time caps. Wire into `ToolExecutor` and the goal
  `Policy`. Make it the default for `/goal` and an opt-in `--sandbox` elsewhere.
- **Risk:** cross-platform parity is hard; ship Linux+macOS first, Windows via
  Job Objects best-effort, container backend as the portable fallback.
- **Acceptance:** a goal cannot write outside its allowlist or hit the network
  when policy forbids it, enforced by the OS, not just the prompt.

#### P1.2 — Vector / embedding retrieval (semantic RAG) · **L**
- **Gap:** the AST graph is exact but can't do fuzzy "where is the auth logic"
  semantic recall, and there's no doc/issue RAG.
- **Architecture:** new `src/retrieval/` — chunker (code + markdown), an
  embeddings provider abstraction (OpenAI/Gemini/Ollama/`bge`-local), and a
  local vector store (sqlite + `usearch`/HNSW, or `lancedb`). New tool
  `semantic_search(query, k)`. Incremental re-index on file change; share the
  ignore rules with `search_files`. Complements, not replaces, the graph.
- **Acceptance:** `semantic_search("rate limiting")` returns the right files
  even when the literal term isn't present.

#### P1.3 — Named subagent definitions · **M**
- **Gap:** teams/skills are close but there's no first-class, reusable subagent
  type (own system prompt, tool allowlist, model) the model can dispatch to by
  name — Claude Code's `.claude/agents/*.md`.
- **Architecture:** `.forge-osh/agents/*.md` (frontmatter: `name`,
  `description`, `system_prompt`, `tools`, `model`). Load into a registry;
  expose via `spawn_team`/a `dispatch_agent(name, task)` tool and the existing
  Worker runtime (each subagent = a Worker with a scoped config). Reuses the
  blackboard + coordinator we already built.
- **Acceptance:** define a `reviewer` agent; the model dispatches review work to
  it and gets a scoped, specialized response.

#### P1.4 — Background process management · **M**
- **Gap:** tools stream live output but block the turn; no detached long-running
  processes (dev servers, watchers) the agent can start, poll, and keep working.
- **Architecture:** a process registry in `src/tools/shell.rs` (or new
  `process.rs`): `bash(..., background=true)` returns a handle; new tools
  `process_status`, `process_logs`, `process_stop`. Surface running processes in
  the TUI statusline. Lifecycle tied to the session.
- **Acceptance:** start `npm run dev` in the background, run tests against it,
  read its logs, stop it — all in one turn chain.

#### P1.5 — Client/server core + session sharing · **XL**
- **Gap:** the agent core is embedded in the TUI process. opencode decouples a
  headless server from front-ends, enabling multiple clients, remote use, and
  shareable sessions.
- **Architecture:** extract the agent loop + session + providers into a
  `forge-osh serve` daemon exposing the existing JSON-RPC surface over a local
  socket / WebSocket. The TUI becomes a client of it (the VS Code extension
  already is, via stream-json — reuse that protocol). Add optional read-only
  **session share** (export an immutable transcript to a file or a self-hosted
  endpoint). This is the highest-leverage architectural change for the future
  (web UI, collaboration, remote agents) but the riskiest — schedule late.
- **Acceptance:** `forge-osh serve` + a thin TUI client reproduce today's UX;
  two clients can attach to one session.

#### P1.6 — Native GitHub/GitLab integration · **M**
- **Gap:** only raw git tools; no first-class PR/issue/review flows (Copilot CLI
  and Cursor lean on this heavily).
- **Architecture:** tools `gh_pr_create`, `gh_pr_review`, `gh_issue`,
  `gh_actions_status` wrapping the `gh`/`glab` CLIs (detect + degrade), plus a
  `/pr` and `/review` command that bundle the diff + a review skill. Optional
  GitHub MCP server as an alternative backend.
- **Acceptance:** `/pr` opens a PR from the current branch with an
  agent-authored title/body and lists CI status.

### P2 — valuable, not urgent

- **P2.1 Rewind / time-travel UX (M):** we have checkpoints + `/undo`; add a
  timeline modal to jump the conversation **and** the working tree back to a
  prior checkpoint (file snapshots already exist in `file_history.rs`).
- **P2.2 Spend caps & budget enforcement (M):** `/goal` observes cost; add an
  enforced global + per-goal USD/token ceiling in `config` + the goal `Budget`
  (currently no `max_usd`), with a hard stop + warning thresholds.
- **P2.3 Auto model routing / cascades (M):** a router policy that picks a cheap
  model for trivial turns and escalates to a strong model on difficulty signals
  (tool-call depth, retries, plan size). Config-driven in `provider/router.rs`.
- **P2.4 Observability / OpenTelemetry (M):** structured spans for turns, tool
  calls, provider calls, costs; export to OTLP. We already use `tracing` —
  add an OTel layer + a `/trace` toggle and run-analytics summary.
- **P2.5 Eval / benchmark harness (M):** promote `tests/test_evaluation_harness`
  + `future_plan/benchmarking.md` into a real `forge-osh eval` subcommand
  (task suites, pass@k, cost/latency, regression gating in CI).
- **P2.6 Statusline & output styles (S):** user-configurable statusline segments
  and response "personas"/output styles (concise/teacher/reviewer) selectable
  per session — extend `GeneralConfig`.
- **P2.7 Queued / steerable input (M):** let the user type and queue messages
  while a turn runs, and inject a steering note mid-turn without cancelling.
- **P2.8 Desktop / push notifications (S):** on long-task / goal completion or
  when input is needed — reuse the `Stop`/notification hook events; add OS
  notifications + optional webhook.
- **P2.9 Diff review queue (S–M):** multi-file review with per-hunk accept/reject
  before applying (extend `src/tui/diff.rs` + the diff-review gate).

### P3 — niche / opportunistic

- **P3.1 Browser / computer-use tool (L):** a native Playwright-backed `browse`
  tool (navigate, screenshot→vision, click/type) — depends on P0.1. Today only
  reachable via a Playwright MCP server.
- **P3.2 Voice input / TTS (M).**
- **P3.3 Cloud task offload (XL):** run a goal on a remote worker/CI and stream
  results back (depends on P1.5).
- **P3.4 Plugin/skill marketplace (L):** a signed registry + `forge-osh install`
  for skills/MCP servers/agents.
- **P3.5 Chat branching (M):** fork a session at a message into an alternate
  branch (depends on session model changes; pairs with rewind).
- **P3.6 Multi-root / monorepo awareness (M):** beyond `/add-dir` — per-root
  config, graph, and LSP roots.

---

## 6. Cross-cutting architecture initiatives

1. **Capability negotiation layer.** Centralize per-model capabilities
   (`supports_vision`, tool-calling, context window, caching, max output) in the
   router so features (images, routing, caching) consult one source of truth
   instead of ad-hoc flags. Prereq for P0.1, P2.3.
2. **Unified "context source" abstraction.** Today context comes from
   `read_file`, graph, search, memory, and (future) embeddings independently.
   A `ContextProvider` trait (graph / vector / lsp / memory / web) with a
   ranked-merge retriever would make `@`-mentions (P0.2), RAG (P1.2), and smarter
   compaction compose cleanly.
3. **Protocol-first core (stream-json) everywhere.** We already have a JSON-RPC
   surface for the IDE. Making the TUI consume the same surface (P1.5) collapses
   two code paths into one and unlocks web/remote/collaboration.
4. **Policy & sandbox unification.** Goal `Policy`, permission rules, and the
   future sandbox should share one model (write-globs, deny-globs, network,
   shell-allowlist) so behaviour is identical whether run interactively, in a
   goal, or in a team worker. Prereq for P1.1.
5. **Long-term memory service.** Generalize CLAUDE.md loading into a memory
   service with (a) explicit user/project notes and (b) optional auto-learned,
   vector-indexed project facts the agent can write and recall — pairs with P1.2.

---

## 7. Suggested roadmap (sequencing)

**Milestone A — "Parity polish" (P0):** rules-file interop (P0.4) → custom
slash commands (P0.3) → `@`-mentions (P0.2) → image input (P0.1).
These are mostly additive, low-risk, and close the most-noticed gaps. Build the
capability-negotiation layer (§6.1) alongside P0.1.

**Milestone B — "Trust & retrieval" (P1 core):** sandboxing (P1.1) and vector
retrieval (P1.2), built on the policy-unification (§6.4) and context-source
(§6.2) initiatives. Add background processes (P1.4) and subagent definitions
(P1.3) — both reuse the existing Worker/coordinator runtime.

**Milestone C — "Ecosystem & ops" (P1/P2):** GitHub integration (P1.6),
observability (P2.4), spend caps (P2.2), eval harness (P2.5), model routing
(P2.3).

**Milestone D — "Platform" (architectural):** client/server core + session
sharing (P1.5), then cloud offload (P3.3) and browser/computer use (P3.1) on top.

---

## 8. Non-goals / explicit risks

- **Don't fork the agent loop.** New execution modes (subagents, background,
  sandbox) must reuse `AgentLoop` + `Worker` + `Coordinator`, not duplicate them.
- **Don't let prompt-only "features" masquerade as enforcement.** Sandboxing and
  spend caps must be real (OS / accounting), consistent with the anti-reward-
  hacking stance already in `/goal`.
- **Cross-platform tax is real** (sandboxing, terminal image paste, background
  processes). Prefer a portable fallback (container, `@file` attach, session-
  scoped process registry) before per-OS optimization.
- **Client/server (P1.5) is the biggest blast radius** — do it only after the
  stream-json protocol has stabilized against the IDE extension, and behind a
  flag, so the monolithic TUI remains the default until proven.
- **Keep BYOK / provider-agnosticism a first principle** — every feature (images,
  embeddings, routing, caching) must degrade gracefully on providers/models that
  lack the capability rather than hard-failing.
