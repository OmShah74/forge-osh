# `/goal` design for forge-osh

## The mental model

A goal is **not a long prompt**. It is a **durable contract** with:
- an *objective* (what to do),
- a *stopping condition* (what "done" looks like in plain English),
- *verifiers* (shell commands or file predicates that empirically prove "done"),
- a *budget* (turns / wall-time / tokens / $),
- a *policy* (what the agent is allowed to write / run / network-fetch unattended),
- a *checkpoint trail* (so `/goal-check` is cheap and crash-safe).

The agent loop runs **autonomously** in a background task. The TUI stays free. The user can chat, run other slash commands, or open `/mcp` while the goal works. Verification is **separate from self-report** — the worker is never trusted; verifiers are.

## Architecture (one diagram, then text)

```
            ┌──────────────── TUI (src/tui/mod.rs) ────────────────┐
            │  /goal …    /goal-check    /goal pause/resume/clear  │
            │  status bar: ● goal#a3f running · 14 ckpt · 2/3 verifs│
            └──────┬──────────────────────────────────▲────────────┘
       GoalControl │                                  │ GoalEvent
                   ▼                                  │
        ┌──────────────────── GoalSupervisor ─────────┴───────────┐
        │  registry of active goals (HashMap<GoalId, Handle>)     │
        │  - control_tx, event_rx, state                          │
        │  - persistence root  ~/.forge-osh/goals/<id>/           │
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

Key invariant: **the user's conversation session and the goal's session are different sessions.** This means /goal-check doesn't pollute the user's transcript with progress noise, the user can keep chatting, and pausing a goal doesn't strand their cursor.

## Files (new)

- `src/agent/goal/mod.rs` — public types (`GoalSpec`, `GoalState`, `GoalEvent`, `GoalControl`, `GoalId`, `GoalMetrics`, `Verifier`, `Policy`, `Budget`)
- `src/agent/goal/supervisor.rs` — `GoalSupervisor` + `GoalHandle`; registry; spawn/pause/resume/clear
- `src/agent/goal/worker.rs` — the autonomous loop (analogous to `agent/loop.rs` but with goal contract wrapping every turn)
- `src/agent/goal/verifier.rs` — runs shell verifiers and file-predicate verifiers; captures stdout/stderr/exit
- `src/agent/goal/persistence.rs` — atomic-write spec/metrics/checkpoint; resume-from-disk
- `src/agent/goal/prompt.rs` — the system-prompt wrapper that tells the model "you are working on goal X; here is the contract; checkpoint every N actions; only stop when verifiers pass"

## Files (edited)

- `src/tui/mod.rs` — slash command dispatch (`/goal`, `/goal-check`, `/goal pause|resume|clear|complete|metrics|verify|logs`), status-bar indicator, event consumer
- `src/config/mod.rs` — `[features] goals = true` flag (default off — match Codex semantics; user enables via config or `/experimental`)
- `src/session/mod.rs` — expose a `Session::fork_for_goal()` so the worker gets its own copy

## Types (the contract)

```rust
pub struct GoalSpec {
    pub id: GoalId,                  // ULID-style
    pub objective: String,           // free text
    pub stopping_condition: String,  // free text the LLM reads
    pub verifiers: Vec<Verifier>,    // empirical proof
    pub budget: Budget,
    pub policy: Policy,
    pub created_at: DateTime<Utc>,
    pub seed_files: Vec<PathBuf>,    // read these first
    pub workdir: PathBuf,
}

pub enum Verifier {
    Shell { cmd: String, expect_exit: i32, expect_stdout_contains: Option<String> },
    FileExists(PathBuf),
    FileContains { path: PathBuf, needle: String },
    NoUncommittedFiles { except: Vec<String> },
    Custom { name: String, cmd: String }, // free-form, scored pass/fail by exit code
}

pub struct Budget {
    pub max_turns: Option<u32>,         // default 200
    pub max_wall: Option<Duration>,     // default 4h
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub max_usd: Option<f64>,           // hard stop on cost
}

pub struct Policy {
    pub network: bool,                  // default true
    pub auto_approve: AutoApprove,      // ReadOnly | AllowedTools | All
    pub write_globs: Vec<String>,       // e.g. ["src/**", "tests/**"]
    pub deny_globs: Vec<String>,        // hard deny
    pub shell_allowlist: Vec<String>,   // regex of allowed shell cmds
}

pub enum GoalState { Idle, Running, Paused, Blocked(String),
                    Verifying, Completed, Cleared, Failed(String) }

pub enum GoalControl { Pause, Resume, Clear, StatusReq(oneshot::Sender<StatusSnapshot>),
                       VerifyNow, ForceComplete }

pub enum GoalEvent {
    Started { id: GoalId },
    Checkpoint(Checkpoint),               // emitted every N actions or T seconds
    VerifierResult { name: String, pass: bool, summary: String },
    Progress { line: String },            // human-readable; lands in progress.log
    Blocked { reason: String },
    StateChanged(GoalState),
    Completed { metrics: GoalMetrics },
    BudgetWarn { kind: &'static str, used: f64, limit: f64 },
}
```

## The worker loop (semantics, not code yet)

```
init: load spec; load or create transcript; restore last checkpoint if resuming
emit Started

while !budget_exhausted && state != Paused {
    if state == Paused { park on resume signal }
    if control.try_recv() handle it (Pause/Clear/VerifyNow/StatusReq)

    n_turns += 1
    1. Build system prompt = base + Goal Contract block + last checkpoint summary
    2. Inner turn: send to LLM with the worker's own session, stream response
    3. Parse tool calls; execute through ToolExecutor with goal Policy
         - read-only ops always pass
         - mutating ops pass if path matches write_globs and not deny_globs
         - shell ops pass if command matches shell_allowlist (or AutoApprove::All)
         - violations -> ToolResult(Denied{reason}) handed BACK to model
    4. Periodically (every CHECKPOINT_EVERY=5 actions OR 60s):
         emit Checkpoint(turns, tokens_in/out, $, files_touched, last_action, blurb)
         persistence::write_checkpoint(...)
    5. If model emits a "claim_done" marker (a stop sequence we taught it):
         state = Verifying
         run verifiers; emit one VerifierResult per
         if all pass -> Completed, write metrics.json, persist, break
         if any fail -> hand the failing verifier output back to the model and continue
}

if budget_exhausted -> Blocked("budget exhausted: …")
final flush; emit Completed or StateChanged(...)
```

## How the worker tells the LLM it's running a goal

A small prompt block injected at the *system* layer of the goal session:

```
You are running in /goal mode. Your contract:

  OBJECTIVE: {objective}
  STOPPING CONDITION: {stopping_condition}
  VERIFIERS (you do not run these — the supervisor does after you claim done):
    1. {verifier_summary_1}
    2. {verifier_summary_2}
  BUDGET: {budget summary}
  POLICY: write {write_globs}, deny {deny_globs}, shell allow {shell_allowlist}

Work in checkpoints. After every meaningful action, emit a one-line
progress note prefixed with "PROGRESS:".

Do NOT ask the user clarifying questions — they have walked away. If you
are blocked, emit a line starting with "BLOCKED:" and stop the turn.

When you believe the stopping condition is met, emit a line starting with
"CLAIM_DONE:" plus a one-paragraph summary, then stop. The supervisor
will then run the verifiers. If a verifier fails, you will receive its
output and must fix and re-claim done.
```

The supervisor scans the stream for `PROGRESS:` / `BLOCKED:` / `CLAIM_DONE:` line prefixes and reacts.

## The non-interrupting `/goal-check`

`/goal-check` does **not** touch the worker task at all. It:

1. Reads `~/.forge-osh/goals/<id>/metrics.json` (last flushed atomically).
2. Reads `checkpoints/latest.json`.
3. Tails progress.log.
4. Renders a one-screen card in the TUI.

Because checkpoints are flushed every 5 actions or 60s on a background fsync, the displayed status lags by at most ~60s. That's the right trade-off — checking status while the LLM is mid-tool-call should NOT serialize with the worker.

## The `/goal-check` card (mock)

```
┌─ goal#a3f2 · Running · turn 47/200 · 1h12m/4h ─────────────────┐
│ Objective:  Migrate src/provider/openai to v2 SDK              │
│ Stopping:   `cargo test --package forge_agent -- openai` green │
│ Phase:      Writing tests for new stream parser                │
│ Budget:     in 312k / out 41k tok · $0.18 / $5.00              │
│ Verifiers:  ✓ build  ✓ clippy  ⏳ tests  · 2/3 passing as of   │
│             last verify @ 14:02:11                             │
│ Last 3 actions:                                                │
│   14:11:02  edit src/provider/openai/stream.rs (+34/-12)       │
│   14:11:40  run `cargo build` → ok                             │
│   14:12:18  PROGRESS: extracted SseFrame helper                │
└────────────────────────────────────────────────────────────────┘
```

## Pause / Resume semantics (the hard part)

- **Pause**: control_tx sends `Pause`. Worker checks at the top of each iteration AND between tool calls within a turn. If mid-stream, it lets the current tool finish (no torn writes), flushes a Checkpoint, sets state=Paused, parks on a `Notify`. The provider stream is dropped — no token billing for unread tokens after the pause point.
- **Resume**: respawn the worker from the last checkpoint, re-load transcript.jsonl, re-load file_cache state. The next system prompt includes a "you were paused, here is where you left off" note.
- **Clear**: control sends `Clear`. Worker aborts current tool call (graceful on file I/O, hard-kill on shell), flushes final metrics, supervisor moves goal to archive (`~/.forge-osh/goals/_archive/<id>/`).

## Persistence layout

```
~/.forge-osh/goals/
  active.json              # {"id":"a3f2…"} — single active goal pointer
  a3f2.../
    spec.toml              # GoalSpec, human-readable
    transcript.jsonl       # full message history (separate from user session)
    progress.log           # plain text, append-only
    metrics.json           # rolling counters, atomic write
    checkpoints/
      latest.json          # pointer
      2026-05-17T14-11-02Z.json
      ...
    verifier_runs/
      2026-05-17T14-02-11Z.json
```

`metrics.json` and `latest.json` are written with `tempfile + rename` so `/goal-check` never reads a torn file.

## Slash command surface

```
/goal <objective>            # opens a short prompt to gather stop-cond / verifiers / budget,
                               then starts. (Or: /goal --from PLAN.md to read a spec file.)
/goal                        # if a goal is active: show summary card
                             # otherwise: show "no active goal" + last 5 archived
/goal-check                  # status snapshot, never blocks the worker
/goal pause
/goal resume
/goal clear
/goal complete               # admin override — skips verification
/goal verify                 # run verifiers now without changing state
/goal metrics                # pretty-print metrics.json
/goal logs [N=50]            # tail progress.log
/goal save                   # force flush (rarely needed; checkpoints are automatic)
```

Subcommand parsing in `src/tui/mod.rs` follows the existing pattern used by `/mcp …`.

## Verification (the part that makes it a contract, not a promise)

Verifiers run in a separate `tokio::process` invocation with a strict 5-minute per-verifier wall clock. Output (stdout/stderr/exit) is captured to `verifier_runs/<ts>.json`. Pass criteria:

- `Shell` → exit == expect_exit AND (if set) stdout contains expect_stdout_contains
- `FileExists` → `tokio::fs::metadata` succeeds
- `FileContains` → read file, substring match
- `NoUncommittedFiles` → `git status --porcelain` filtered by `except` is empty
- `Custom` → exit == 0

If **any** verifier fails after a `CLAIM_DONE:`, the worker re-enters Running with the failure messages handed to the model as a synthetic user turn:

```
Verification failed:
  ✗ tests → exit 101
    output: ---- openai::stream_parser_handles_partial_chunk stdout ----
            thread '…' panicked at 'assertion failed'
Continue working until all verifiers pass.
```

## Budget enforcement

Tracked in the worker per-iteration:
- input/output tokens from each provider response usage block (we already capture this in `session/tokens.rs`)
- $ cost from existing `cost_tracker` (which per MEMORY must be serialized — already enforced)
- wall time from `Instant::now() - started_at`
- turns counter

On any budget breach: emit `BudgetWarn` then transition to `Blocked("budget exhausted: …")`. State persists; user can `/goal resume` after raising the limit via `/goal budget --max-usd 10`.

## Feature flag (mirrors Codex)

`config.toml`:
```toml
[features]
goals = true
```

If false, the slash commands return "goals are experimental — enable via [features] goals = true". Mirrors the Codex DX so docs/screenshots transfer.

## Concurrency / multi-goal

Phase 1: **single active goal**. `active.json` enforces this; trying to start a second goal errors out with "already running goal X; pause or clear it first". This avoids the file-write-collision class described in the Hermes article.

Phase 2 (later): named worktrees — `/goal --worktree feature-foo …` spawns the worker inside `git worktree add` so multiple goals can run in parallel on independent branches without stepping on files. Out of scope for the first cut.

## Safety rails I'm baking in by default

- `deny_globs` always includes `.git/**` and `~/.forge-osh/keys.json` regardless of policy.
- Goal worker session never has access to the user's API keys directly — it borrows the same provider client, but key access is via the existing keystore (no path-of-leakage).
- Shell allowlist defaults to a conservative set: `cargo (build|check|test|clippy|fmt)`, `git (status|diff|log|add|commit)`, `npm (test|run build)`, etc. Anything else → blocked unless `auto_approve = All`.
- Goal mode is **non-destructive by default**: `rm`, `git push`, `git reset --hard` always blocked unless explicitly added to allowlist.

## Phased implementation order

I'd ship this in 4 PRs, each independently buildable:

1. **Skeleton + types + persistence** (`src/agent/goal/{mod,persistence}.rs`, config flag, slash command stubs that print "not yet implemented"). ~250 LOC. Verifies plumbing.
2. **Worker loop + system-prompt wrapper + budget tracking** (`worker.rs`, `prompt.rs`). Goal can run but verification is no-op. ~500 LOC.
3. **Verifiers + claim-done parsing + verify-fail→continue loop** (`verifier.rs` + worker integration). ~250 LOC.
4. **TUI UX**: status bar indicator, `/goal-check` card, `/goal logs`, `/goal pause/resume/clear`, archive view. ~400 LOC.

## What I'd like from you before I write the first line

Three decisions that change the design:

1. **Single goal vs. multi-goal from day 1?** (I'd recommend single for v1, worktree multi later.)
2. **Auto-approval default**: ReadOnly + allowlist (safe, sometimes annoying) or All-within-write_globs (more like Codex's default, faster but riskier)?
3. **Goal session bills against the user's same cost tracker, or its own tracker tagged with goal_id?** (I'd recommend its own, summed into the parent for display, so `/goal metrics` shows isolated spend.)

Once you answer those three (or say "your call on all three"), I'll start with PR 1 (skeleton + persistence + stubs) so you can see the plumbing land cleanly before the big loops.

---

## Implementation Amendments (per user direction, 2026-05-17)

The user has overridden three design decisions before Phase 1 begins. These changes are binding for the implementation:

1. **Multi-goal from day 1.** No `active.json` single-goal pointer. The supervisor holds `HashMap<GoalId, GoalHandle>` and supports arbitrarily many concurrent goals. The "file-collision" risk is left to the user's discretion (they're expected to scope goals via `write_globs` / different repos / worktrees).
2. **No cost limits.** `Budget::max_usd` is removed entirely. The cost tracker still records spend per goal (and rolls into `metrics.json`), but spend never triggers `Blocked`. Token / wall / turn budgets remain available as optional knobs.
3. **Goal session has its own cost tracker** tagged with `goal_id`, summed into the parent display.

### Revised `Budget` shape

```rust
pub struct Budget {
    pub max_turns: Option<u32>,         // default 200
    pub max_wall: Option<Duration>,     // default 4h
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    // No max_usd — cost is observed, never enforced.
}
```

### Revised persistence layout

```
~/.forge-osh/goals/
  index.json               # {"goals":[{"id":"a3f2","state":"Running",…}, …]}
  a3f2.../                 # one dir per goal, all independent
    spec.toml
    transcript.jsonl
    progress.log
    metrics.json           # includes cost_usd as observed (no limit)
    checkpoints/…
    verifier_runs/…
  _archive/                # goals cleared/completed move here
    a3f2.../
```

`index.json` replaces `active.json`. Atomically rewritten on every state transition. The supervisor rebuilds its in-memory registry by reading this file on startup, then spawning a resumer task for any goal whose state is `Running` or `Paused` at the time of crash.

### Phase 1 (this PR) — concrete deliverables

1. `src/agent/goal/mod.rs` — types (`GoalSpec`, `GoalState`, `GoalEvent`, `GoalControl`, `GoalId`, `GoalMetrics`, `Verifier`, `Policy`, `Budget`, `AutoApprove`, `Checkpoint`, `StatusSnapshot`) — no methods beyond constructors and serde derives.
2. `src/agent/goal/persistence.rs` — atomic write helpers (`write_atomic`, `read_or_default`), `IndexFile` type with load/save, per-goal directory helpers (`goal_dir`, `archive_goal`), checkpoint ring rotation, progress.log appender. No business logic.
3. `src/agent/goal/supervisor.rs` — `GoalSupervisor` skeleton with the multi-goal registry, `spawn(spec) -> GoalId`, `pause(id)`, `resume(id)`, `clear(id)`, `status(id) -> StatusSnapshot`, `list() -> Vec<GoalSummary>`. The worker side is a no-op placeholder that emits `Started` then sits in a select loop awaiting control signals (so Phase 2 just fills in the action body).
4. `src/agent/goal/mod.rs` registered from `src/agent/mod.rs`.
5. `src/config/mod.rs` — `[features] goals = bool` flag, default `false`.
6. Slash command stubs in `src/tui/mod.rs`:
   - `/goal <objective>` → if flag off, error; else create a `GoalSpec` with sensible defaults from the objective text and call `supervisor.spawn`.
   - `/goal` (no args) → list all goals via `supervisor.list()`, render a small table.
   - `/goal-check [id]` → render `StatusSnapshot` card.
   - `/goal pause|resume|clear|complete|verify|metrics|logs <id>` → dispatch into supervisor methods, print result.
7. No worker loop yet — Phase 2 fills `worker.rs`. The supervisor's spawn function creates the goal dir, writes the spec, updates the index, spawns a placeholder task that just emits `Started` and then awaits Pause/Clear. This is enough to verify wiring end-to-end without LLM calls.

Phase 1 success criteria: `/goal "test"` creates the goal directory on disk, `index.json` shows it as `Running`, `/goal-check <id>` returns a populated `StatusSnapshot`, `/goal clear <id>` archives it and removes it from the live index. No verifiers, no LLM calls, no actual work — just the contract surface.

---

## Phase 1 — completed 2026-05-17

Landed; `cargo check` is clean. Working surface:

### New files
- `future_plan/05_goal_integration.md` (this file)
- `src/agent/goal/mod.rs` — all public types: `GoalId` (sortable `<base36ts>-<hex>`), `GoalSpec` (with `from_objective(workdir)` constructor), `GoalState` (`Idle|Running|Paused|Verifying|Blocked|Completed|Cleared|Failed`), `GoalEvent`, `GoalControl`, `GoalMetrics`, `Verifier` (Shell/FileExists/FileContains/NoUncommittedFiles/Custom), `Policy` with conservative shell allowlist defaults, `Budget` with **no `max_usd`** (cost observed, never enforced), `AutoApprove`, `Checkpoint`, `StatusSnapshot`, `GoalSummary`. All serde-derived. Default deny_globs = `[".git/**", "**/keys.json", "**/.env"]`.
- `src/agent/goal/persistence.rs` — atomic `write_atomic(tempfile+rename)`, `IndexFile` load/save, `save_spec/load_spec` (TOML), `save_metrics/load_metrics` (JSON), `save_checkpoint` + `load_latest_checkpoint` + 50-file ring rotation, `append_progress` + `tail_progress`, `append_transcript_line`, `archive_goal`, `upsert_index/remove_from_index`.
- `src/agent/goal/supervisor.rs` — `GoalSupervisor` with `HashMap<GoalId, Arc<GoalHandle>>`, fan-in `events_tx`/`events_rx` (single drain via `take_event_rx`), methods `spawn/get/list/pause/resume/clear/status/verify_now/force_complete`. Placeholder worker `run_placeholder_worker` that emits `Started`, writes an initial checkpoint, then parks on a `tokio::select!` waiting for `GoalControl` messages.

### Edits
- `src/agent/mod.rs` — registered `pub mod goal`.
- `src/config/mod.rs` — added `FeaturesConfig { goals: bool }` table; gated by `[features] goals = true`.
- `src/tui/mod.rs`:
  - Added `AppState::goal_supervisor: Arc<GoalSupervisor>` (always constructed; spawn is gated by the feature flag at command-dispatch time).
  - Added `/goal` and `/goal-check` to `handle_slash_command`.
  - Added `cmd_goal` covering: no-arg list, `pause/resume/clear/complete/verify/metrics/logs <id>`, and "treat any other text as a new objective".
  - Added `cmd_goal_check` that resolves a single live goal automatically when no id is given; renders state + tokens + cost + last checkpoint + last 5 progress lines.

### Verified working today
- `/goal "test"` (with `[features] goals=true`) creates `~/.forge-osh/goals/<id>/{spec.toml, metrics.json, progress.log, checkpoints/{latest.json,<ts>.json}}` and updates `index.json`. Multiple `/goal` invocations spawn concurrent workers — no single-active gate.
- `/goal` lists all goals with state/turns/cost.
- `/goal-check [id]` reads from disk (does **not** touch the worker task), so it's safe to call while the worker is busy.
- `/goal clear <id>` sends `Clear`, awaits up to 3s for graceful exit, removes from `index.json`, and moves the directory to `~/.forge-osh/goals/_archive/`.
- Cost is recorded in `GoalMetrics.cost_usd` but never enforced; budget is `turns / wall / tokens` only.

### Deliberately not yet implemented (later phases)
- The worker is a placeholder; no LLM is called and no tools execute.
- Verifier shell execution is stubbed (`verify_now` just emits a progress note).
- No status-bar indicator in the TUI yet.
- No `--from PLAN.md` spec-file loader.
- No resumer task that rebuilds goal handles from `index.json` after a process restart (cold-start currently leaves on-disk goal dirs orphaned of in-memory handles until the next `/goal-check` reads disk).

---

## Phase 2 — Worker loop + system-prompt wrapper + budget tracking

### Goal of this phase

Replace the placeholder worker with a real autonomous loop that:

1. Iteratively prompts the configured provider with a goal-contract system prompt.
2. Streams response text and scans it for the three protocol markers (`PROGRESS:`, `BLOCKED:`, `CLAIM_DONE:`).
3. Parses and executes tool calls returned by the provider through the existing `ToolExecutor`, applying the goal's `Policy` for auto-approval (so it doesn't ask the user for permission mid-run).
4. Tracks tokens, cost (observed only, no limit), turns, wall time. Honors `Budget` for `max_turns / max_wall / max_input_tokens / max_output_tokens`. On any budget breach → `Blocked("budget exhausted: …")`.
5. Writes a `Checkpoint` to disk every 5 tool calls or 60 wall-seconds (whichever comes first). Each `PROGRESS:` line also appends to `progress.log`.
6. Cooperates with `GoalControl::Pause / Resume / Clear`. Pause is observed at iteration top and between tool calls (never mid-stream so we don't leak tokens or leave partial writes).
7. On `CLAIM_DONE:`, transitions to `Verifying`; phase-2 stub simply transitions through to `Completed` (real verifier execution arrives in phase 3). The `CLAIM_DONE` summary is captured into `progress.log` and final `metrics.json`.

### New file

- `src/agent/goal/prompt.rs` — builds the goal-contract system prompt block from a `GoalSpec` + last `Checkpoint` + recent failed-verifier output (latter unused in phase 2). Exposes `build_system_prompt(spec, last_ckpt, base_prompt) -> String` and `parse_protocol_line(text) -> Option<ProtocolSignal>` where `ProtocolSignal ∈ { Progress(s), Blocked(s), ClaimDone(s) }`.

### New file

- `src/agent/goal/worker.rs` — the real worker. Moved out of `supervisor.rs` to keep that file focused on lifecycle. Exposes:
  ```rust
  pub struct WorkerDeps {
      pub provider_router: Arc<RwLock<ProviderRouter>>,
      pub key_store: Arc<Mutex<KeyStore>>,
      pub tool_registry: Arc<ToolRegistry>,
      pub config: Arc<Config>,
      pub shared_graph: SharedGraph,
      pub file_cache: Arc<FileStateCache>,
  }
  pub async fn run_worker(ctx: WorkerCtx, deps: WorkerDeps);
  ```
  Inner shape (no LLM-loop reuse from `agent/loop.rs` — too much user-session coupling; we re-implement the slim variant here):
  - `messages: Vec<Message>` — the goal session's transcript. Seeded with the system prompt + an initial user message containing the objective.
  - per-iteration: rebuild system prompt (in case the last checkpoint changed), call `provider.complete_stream(...)`, drain the stream, accumulate text + tool calls + usage.
  - on each chunk: scan for protocol markers; append `PROGRESS:` lines to disk immediately for live `/goal-check`.
  - tool execution: synthesize a `ToolPermissionRequest`, evaluate it against the goal `Policy::auto_approve` + write_globs + shell_allowlist; **never** prompt the user. Denied calls return a `ToolResult { is_error: true, content: "denied by goal policy: …" }` back to the model so it can adapt.
  - track `files_touched` by inspecting tool names / args (`write_file`, `edit_file`, etc.).
  - between turns: persist transcript line, update metrics, write checkpoint if due, persist index.
  - state transitions emitted via `events_tx` (so the TUI can rerender the status bar).

### Edits

- `src/agent/goal/supervisor.rs`:
  - Replace `run_placeholder_worker` with a thin shim that defers to `worker::run_worker`, threading the `WorkerDeps` it receives at `GoalSupervisor::spawn`.
  - Extend `GoalSupervisor::new()` → `GoalSupervisor::new(deps: WorkerDeps)`. The supervisor now holds `Arc<WorkerDeps>` so each goal spawn doesn't have to re-pass them.
- `src/tui/mod.rs`:
  - At TUI bootstrap (where `AppState::new` is called from `run_tui`), construct the `WorkerDeps` from already-available `provider_router`, `key_store`, `tool_registry`, etc., and pass them into `GoalSupervisor::new`.
  - Drain `goal_supervisor.take_event_rx()` once at startup and forward each `GoalEvent` into a TUI system-message line (Phase 4 will replace this with a proper status bar / live card, but this is enough for the user to see liveness in phase 2).

### Policy enforcement (the hard part — getting it right)

Helper `policy::is_tool_call_allowed(call, policy, workdir) -> Allow | Deny(reason)`:
- Read-only tools (`read_file`, `glob`, `grep`, `list_dir`) → always allow.
- Mutating tools (`write_file`, `edit_file`, `delete_file`) → allow iff every affected path matches at least one `write_glob` AND no `deny_glob`. Paths are normalized to absolute then matched relative to `workdir`.
- Shell tools (`bash`, `shell_exec`) → allow iff the command (after trim) matches any `shell_allowlist` regex AND no `deny_glob` overlap. With `AutoApprove::All` the regex check is skipped.
- Network tools (`web_fetch`, `web_search`) → allow iff `policy.network`.
- MCP tools → allow under `AutoApprove::AllowedTools | All` (MCP servers are user-installed and already trust-tagged).
- Anything else → deny with a clear reason that gets surfaced to the model.

This is conservative on purpose; the model is expected to adapt to denials, not the user to whitelist on the fly.

### Budget enforcement

At the top of every iteration:
```
let elapsed = Instant::now() - started_at;
if let Some(max) = budget.max_wall      { if elapsed > max          { Block("wall"); } }
if let Some(max) = budget.max_turns     { if turns >= max           { Block("turns"); } }
if let Some(max) = budget.max_input_tokens  { if in_tok >= max      { Block("in_tok"); } }
if let Some(max) = budget.max_output_tokens { if out_tok >= max     { Block("out_tok"); } }
```
On `Block(kind)` we emit `GoalEvent::BudgetWarn`, then set state = `Blocked(kind)`, flush, persist, and return. Worker can be `Resume`d once the user raises the limit via a future `/goal budget` subcommand (not in this phase).

### Checkpoint policy

- Triggered every 5 successful tool calls OR every 60 seconds (whichever first), and at every state transition.
- Atomic write through `persistence::save_checkpoint` (already implemented).
- Includes `phase` (currently a free-form blurb: "thinking", "running tool: bash", "verifying"), `last_action` (last tool name + 1-line arg summary), `files_touched`, `progress_blurb` (latest PROGRESS: line), and a snapshot of `GoalMetrics`.

### Phase 2 success criteria

- `[features] goals=true` + `/goal "Write a hello.txt with the text 'hi' in the current working dir"` produces an actual file on disk (assuming default write_globs are extended to cover the workdir, which we will scaffold to "**" if write_globs is empty — TODO: tighten).
- `/goal-check <id>` shows the model's PROGRESS: lines streaming in over the seconds while the worker is running.
- `/goal pause <id>` causes the worker to stop on the next tool boundary; `/goal resume <id>` continues from there.
- `/goal clear <id>` interrupts gracefully and archives.
- Tokens, cost, turns, wall time all visible in `/goal metrics <id>`.
- Verification still no-op: `CLAIM_DONE:` → `Completed` without running shell verifiers.

---

## Phase 2 — completed 2026-05-17

`cargo check` clean (13s). Workers now run a real autonomous LLM loop.

### Design decision: reuse AgentLoop instead of reimplementing

Rather than duplicating ~1000 lines of streaming / tool-execution / context-management logic from `src/agent/loop.rs`, the goal worker now constructs a *scoped* `AgentLoop` with its own session and its own event channel, and drives it iteratively. This keeps tool execution, MCP integration, context auto-compaction, skill scoping, and provider/error retry exactly consistent between user-driven and goal-driven sessions — a single source of truth.

### New files

- `src/agent/goal/prompt.rs`:
  - `ProtocolSignal { Progress, Blocked, ClaimDone }` — what the supervisor recognises in streamed text lines.
  - `parse_protocol_line(raw)` — line-prefix matcher (`PROGRESS:` / `BLOCKED:` / `CLAIM_DONE:`).
  - `scan_all_signals(text)` — scan accumulated text for multiple signals, returning byte offsets so the caller can advance a scan pointer past completed lines.
  - `build_goal_system_block(spec, last_checkpoint)` — appends a long `## /goal mode` block to the system prompt, containing the contract (objective + stopping condition + workdir + verifiers + budget + policy summary), the line-protocol instructions, the working-style guidance, and an optional "resumed from last checkpoint" block.
  - `initial_user_message(spec)`, `continuation_message()`, `verifier_failure_message(failures)` — first-turn, mid-run, and (Phase 3) re-prompt strings.

- `src/agent/goal/worker.rs`:
  - `WorkerDeps` — `Arc`-shared dependencies the supervisor hands to every worker: `provider_router`, `tools`, `config`, `graph`, `lsp`, `file_cache`, `permission_store`, `skill_registry`.
  - `WorkerCtx` — per-goal scratch state: `id`, `spec`, `state`, `metrics`, `events_tx`, `control_rx`, `resume_notify`.
  - `run_worker(ctx, deps)` — the real loop.

### Worker loop semantics (what actually happens now)

1. Emit `Started` and write an initial checkpoint (turn 0, phase `starting`).
2. Construct a **scoped `Session`** named `goal-<id>` with its own `CostTracker`, working_dir = `spec.workdir`, provider/model copied from the live router.
3. Clone `Config` and append `build_goal_system_block(...)` to `general.system_prompt_extra`. This is the entire integration point that makes the model aware of `/goal` mode — no other code paths needed.
4. Create per-goal `event_tx`/`event_rx`, `permission_tx`/`permission_rx` (unused in Bypass mode but `AgentLoop` requires the fields), `cancel` token, `permission_mode = Bypass` (Phase 2 simplification — Phase 3 wires policy-based gating).
5. Construct `AgentLoop` Arc with all of the above sharing `WorkerDeps` for everything else.
6. Outer iteration:
   - **drain_control** — non-blocking try_recv on `control_rx`. Pause flips state & parks on `resume_notify`. Clear cancels the cancel token and breaks out. ForceComplete short-circuits to `Completed`. StatusReq sends a fresh `StatusSnapshot` back to the requester. VerifyNow emits a "no verifiers wired in phase 2" progress line.
   - **check_budget** — `max_turns / max_wall / max_input_tokens / max_output_tokens` (no cost limit). On breach: emit `BudgetWarn`, transition to `Blocked("budget exhausted: …")`, break.
   - **fresh cancel token** per turn so `Clear` is interruptable mid-stream.
   - **spawn** the `AgentLoop::run(user_message)` as a tokio task and **drain its event stream concurrently**.
7. `drain_agent_events`:
   - 200 ms timeout polling loop on `event_rx`, interleaved with `drain_control` between events (Pause flips state and returns `Paused`; Clear returns `Cancelled`).
   - `Token` events accumulate into a per-turn buffer; on each chunk we call `scan_all_signals` against the unscanned tail. `PROGRESS:` lines append to `progress.log` and emit `GoalEvent::Progress`; `BLOCKED:` cancels and returns `Blocked(reason)`; `CLAIM_DONE:` returns `ClaimDone(summary)` immediately so verification can run after the agent finishes the current message.
   - `ToolStart` extracts paths from common arg keys (`path`/`file_path`/`filepath`/`filename`) into `files_touched`. `ToolEnd` increments the per-checkpoint counter and appends a `TOOL: <name> (ok|error)` line to `progress.log`.
   - `Error` is captured; `Done` is the normal exit. Channel closure also exits cleanly.
   - **maybe_checkpoint** every 5 tool calls or 60 wall-seconds (whichever comes first). Writes a `Checkpoint` to disk atomically; reads the current `GoalMetrics` snapshot.
8. After the run returns:
   - `refresh_metrics` reads `session.cost_tracker.{total_input_tokens, total_output_tokens, total_cost_usd}` and updates `GoalMetrics`. Increments `turns`. Saves `metrics.json` atomically.
   - Writes a between-turns checkpoint.
   - Dispatch on the `DrainOutcome`: `ClaimDone(s)` → break to Completed; `Blocked(r)` → break to Blocked; `Paused` → park; `Cancelled` → break to Cleared; `Errored(e)` → break to Blocked; `Finished` → loop with a continuation message.

### Supervisor changes

- `GoalSupervisor::new()` keeps the no-arg signature (so `AppState::new` can construct it during early boot). Deps are injected post-boot via new `async fn set_deps(&self, deps: WorkerDeps)`.
- `is_ready()` returns whether deps have been set.
- `spawn(spec)` returns `GoalError::NotReady` if `set_deps` has not yet been called.
- The placeholder worker is gone; `spawn` now spawns `worker::run_worker(ctx, deps_snapshot)`.
- `clear()` now sets `state = Cleared` *before* sending the control signal, so a paused worker (parked on `resume_notify`) sees Cleared when woken and exits cleanly. Timeout for graceful exit was bumped from 3s to 5s.

### TUI changes

- After the user's `AgentLoop` is built in `run_tui`, we construct `WorkerDeps` from the same shared `provider_router`, `tools`, `config`, `graph`, `lsp`, `file_cache`, `permission_store`, `skill_registry` Arcs, then call `state.goal_supervisor.set_deps(deps).await`.
- A background task consumes `take_event_rx()` and forwards each `GoalEvent` into the existing `state.mcp_status_msgs` queue (which the main loop already drains and surfaces via `push_system`). Phase 4 will replace this with a dedicated status bar + live `/goal-check` card.
- `format_goal_event_line(id, ev)` is a small helper in `src/tui/mod.rs` that renders a `GoalEvent` as a single user-facing line (`Started`, `Progress`, `Blocked`, `Completed`, `StateChanged`, `BudgetWarn` → visible; `Checkpoint` / `VerifierResult` → silent at this layer because phase 4 will show them in the live card).

### What works end-to-end now

- `[features] goals = true` in `~/.forge-osh/config.toml`, then in the TUI:
  - `/goal Write hello.txt with 'hello world'` spawns an autonomous worker that uses the *current* provider/model, runs the agent loop with `PermissionMode::Bypass` (so no permission prompts interrupt it), watches the stream for protocol markers, and persists everything.
  - `/goal-check <id>` reports the latest checkpoint, metric snapshot, and last 5 progress lines — entirely from disk, never touching the worker task.
  - `/goal pause <id>` lands the worker on the next event boundary; `/goal resume <id>` re-enters the loop with a continuation message.
  - `/goal clear <id>` cancels mid-stream, archives to `_archive/`, and removes from `index.json`.
  - All TUI-surface `GoalEvent`s appear inline in the conversation as system messages prefixed `[goal#<id>]`.

### Known limitations (deliberately deferred)

- **Verifiers are a stub.** `CLAIM_DONE:` transitions straight to `Completed`. Phase 3 adds shell/file verifier execution and the verify-fail → continue-with-feedback loop.
- **Permission policy is binary (Bypass).** The configured `Policy::auto_approve`, `write_globs`, `shell_allowlist` are not yet enforced — Phase 3 will gate tool calls through a policy filter before reaching the executor.
- **Checkpoint `files_touched` is empty.** `maybe_checkpoint` currently passes an empty Vec; the worker tracks the list locally but doesn't snapshot it into each checkpoint yet (cosmetic, fixed in phase 4).
- **No `--from PLAN.md` spec loader and no `/goal budget …` subcommand.** Both are phase 4.
- **No cold-start resume.** If forge-osh is killed while a goal is running, the goal directory and `index.json` survive but the in-memory `GoalSupervisor` doesn't auto-respawn the worker on the next launch. Phase 4 adds a resumer that reads `index.json` at boot.
- **No status-bar indicator.** Phase 4 will add `● goal#a3f running · ckpt 14 · 2/3 verifs` to the ratatui chrome.

---

## Phase 3 — Verifiers + policy-based tool gating

### Ultimate goal of this phase

Turn `CLAIM_DONE` from a self-report into a verified contract, and turn the goal `Policy` from a comment into an enforced gate. After phase 3:

1. When the model emits `CLAIM_DONE:`, the supervisor runs every configured verifier in a separate process, captures stdout/stderr/exit, persists the run to disk, and only transitions to `Completed` when **all** verifiers pass. Any failure feeds the failure output back to the model as a synthetic user turn ("verification failed — fix and re-claim"), and the worker continues looping.
2. Tool calls from the goal session no longer run under blanket `Bypass`. Instead the worker runs in `Default` permission mode and intercepts every `PermissionRequest`, evaluating it against the goal's `Policy` (auto_approve level, write_globs, deny_globs, shell_allowlist, network) and answering automatically. The user is **never** prompted — but unauthorised calls are denied and the model receives a structured error it can adapt to.
3. `/goal verify <id>` becomes meaningful: it triggers a verifier run on demand without flipping state.

### New file: `src/agent/goal/verifier.rs`

- `pub struct VerifierResult { name: String, passed: bool, summary: String, exit_code: Option<i32>, stdout_excerpt: String, stderr_excerpt: String, duration_ms: u64 }`
- `pub struct VerifierReport { results: Vec<VerifierResult> }` with `all_pass()`, `failures()`, `pass_count()`, `fail_count()`.
- `pub async fn run_all(spec: &GoalSpec) -> VerifierReport` — runs every verifier in the spec **sequentially** (the goal's workdir is shared state — parallel runs would race). Each verifier has a hard 5-minute wall clock.
- Per-verifier implementations:
  - `Shell { cmd, expect_exit, expect_stdout_contains }`: spawn via `tokio::process::Command`; on Windows use `cmd /C`, on Unix use `sh -c`; run inside `spec.workdir`. Pass = exit matches expect_exit AND (if set) stdout contains the needle.
  - `FileExists { path }`: `tokio::fs::metadata(workdir.join(path))` succeeds.
  - `FileContains { path, needle }`: read file, byte-find the needle.
  - `NoUncommittedFiles { except }`: run `git status --porcelain` in workdir, parse lines, exclude any path whose tail matches any glob in `except`. Pass = empty result.
  - `Custom { name, cmd }`: shell exec; exit 0 = pass.
- Every run persists to `~/.forge-osh/goals/<id>/verifier_runs/<iso_ts>.json` (already-allocated layout from phase 1).

### New file: `src/agent/goal/policy.rs`

- `pub enum Decision { Allow, Deny(String) }`
- `pub fn evaluate(tool_name: &str, args: &Value, level: PermissionLevel, policy: &Policy, workdir: &Path) -> Decision`
- Decision tree (in order):
  - `AutoApprove::All` → Allow (unless `deny_globs` would catch a referenced path; still hard-deny on `.git/**`, `**/keys.json`, `**/.env`).
  - Tool name starts with `mcp__` → Allow if `AutoApprove != ReadOnly` (MCP tools are already trust-tagged at registration time).
  - `level == ReadOnly` → Allow.
  - `level == Mutating`: gather all path-typed args (`path`, `file_path`, `filename`, `filepath`); each must match ≥1 entry of `write_globs` AND no entry of `deny_globs`. If `write_globs.is_empty()`, default to "**" (entire workdir writable) **only** when `auto_approve = AllowedTools`.
  - `level == Destructive` → Deny unless `AutoApprove::All`.
  - `level == Shell`: extract the command string from `args.command` or `args.cmd`; pass iff it matches ≥1 entry of `shell_allowlist` as a regex (compiled with `regex::Regex::new`, anchored at the start by convention). Also deny if the command mentions any deny_glob path.
  - `level == Network` → Allow iff `policy.network`.
- Tool-name → `PermissionLevel` mapping comes from `tool_registry.level_for(name)`; for unknown tools default to `Mutating`.

### Worker changes (`src/agent/goal/worker.rs`)

1. Switch `permission_mode` from `Bypass` to `Default`. Spawn a per-goal "permission responder" task that drains `perm_req_rx` (we will need to actually wire this — the request rx exists but is currently discarded; we'll keep it).
2. The responder task: for each `PermissionRequest`, call `policy::evaluate(...)` against the goal's spec.policy + spec.workdir. Reply `Allow` or `Deny` via `request.response_tx.send(...)`. Never block on user input.
3. On a `DrainOutcome::ClaimDone(summary)`, transition state to `Verifying`, emit `StateChanged(Verifying)`, call `verifier::run_all(&spec)`, persist a `verifier_runs/<ts>.json`, emit one `VerifierResult` event per check, and:
   - all pass → transition to `Completed`, emit `Completed`, break outer loop.
   - any fail → bump `metrics.verifiers_failed`, append a `PROGRESS: verification FAILED (n/m passed)` line, build a `verifier_failure_message(failures)` and continue the loop — the next agent turn submits that message as the user prompt.
   - Track cumulative `verifiers_passed` / `verifiers_failed` in `GoalMetrics`.
4. New `verify_now` control: actually run verifiers and emit results without changing state.

### Supervisor / TUI changes

- `format_goal_event_line` learns `VerifierResult` so `/goal verify <id>` and the ambient stream show `✓` / `✗` per check.
- `/goal verify <id>` command: now meaningful — issues `GoalControl::VerifyNow`; the worker runs verifiers and emits events.

### Out of scope for phase 3 (deferred to phase 4)

- Status-bar indicator
- Cold-start resumer
- `/goal budget` subcommand
- `--from PLAN.md` loader
- `GoalSpec` editing via slash command (verifiers must be added by editing `spec.toml` directly for now)

### Phase 3 success criteria

- A spec with `verifiers = [{ type = "shell", cmd = "ls hello.txt", expect_exit = 0 }]` will not transition to `Completed` until `hello.txt` actually exists in workdir.
- A spec with a deliberately-failing verifier loops indefinitely (or until budget exhausts), the model receiving fresh failure output each turn.
- `Policy::auto_approve = ReadOnly` causes every write/shell call to be denied automatically — model sees `denied by goal policy: …` in tool result and can adapt.
- `Policy::shell_allowlist = ["^cargo "]` causes `bash` calls matching that regex to allow, others to deny, with no user prompt.
- `/goal verify <id>` from the TUI runs verifiers and shows pass/fail per check without altering state.
- `~/.forge-osh/goals/<id>/verifier_runs/*.json` accumulates one file per verifier run.

---

## Phase 3 — completed 2026-05-17

`cargo check` clean (10.63s). Verifiers run, policy gates apply.

### New files

- `src/agent/goal/policy.rs`:
  - `Decision { Allow, Deny(reason) }`
  - `evaluate(tool_name, input_summary, level, policy) -> Decision` — the heart of the gate. Decision tree honours `deny_globs` first (always hard-deny), then MCP tools (allow unless `ReadOnly`), then `AutoApprove::All` (blanket allow), then per-level rules. `Mutating` checks `write_globs` if non-empty (defaults to "allow within workdir" when empty). `Destructive` requires `AutoApprove::All`. `Shell` matches `shell_allowlist` regexes against an extracted command token. `Network` honors `policy.network`.
  - Heuristic helpers `extract_shell_cmd` (strips `bash:`/`command:` prefixes from `input_summary`) and `extract_path_token` (pulls the first path-shaped token from a summary). Path-glob enforcement is therefore *advisory* in phase 3 — it kicks in when the summary contains a clear path token. Phase 4 will plumb raw JSON args through so the gate is exact.
- `src/agent/goal/verifier.rs`:
  - `VerifierResult { name, passed, summary, exit_code, stdout_excerpt, stderr_excerpt, duration_ms }`
  - `VerifierReport { at, results }` with `all_pass / is_empty / failures / pass_count / fail_count`.
  - `run_all(spec)` — sequential execution (verifiers share workdir state — parallel runs would race). Each verifier capped at 5 min wall.
  - Per-verifier implementations:
    - `Shell`: `cmd /C` on Windows, `sh -c` on Unix, run inside `spec.workdir`. Pass iff exit matches `expect_exit` AND (if set) stdout contains the needle. Output captured & truncated to 4 KiB excerpts.
    - `FileExists`: `tokio::fs::metadata(workdir.join(path))`.
    - `FileContains`: read file, byte-find needle.
    - `NoUncommittedFiles { except }`: `git status --porcelain` in workdir, filter porcelain entries by `glob::Pattern` against `except`.
    - `Custom { name, cmd }`: shell exec; exit 0 = pass.
  - `persist_report(id, report)` writes `~/.forge-osh/goals/<id>/verifier_runs/<iso_ts>.json` atomically.

### Worker changes (`src/agent/goal/worker.rs`)

- `permission_mode` switched from `Bypass` to **`Default`**. The agent loop now sends `PermissionRequest`s to a per-goal mpsc channel.
- Spawned **policy responder task** drains `perm_req_rx`. For every request it calls `policy::evaluate(tool_name, input_summary, level, &spec.policy)` and replies `Allow` / `Deny` via the request's `response_tx`. Every Deny is also surfaced to the user via `GoalEvent::Progress` and appended to `progress.log` as `POLICY: deny <tool> (<summary>) — <reason>`. The user is never prompted.
- New `pending_continuation_override: Option<String>` — set when a verification run fails, consumed by the next iteration as that turn's user prompt (the model receives the verifier-failure feedback message).
- New `pending_verify_now: bool` — set when `GoalControl::VerifyNow` arrives mid-stream. Outer loop checks it after each turn's drain completes; if true, runs verifiers, restores prior state, emits results.
- `ClaimDone` no longer terminates immediately. It calls `run_verification_phase`:
  - flips state to `Verifying` and emits `StateChanged(Verifying)`.
  - If `spec.verifiers.is_empty()` → `VerifyOutcome::NoVerifiers` → trust CLAIM_DONE → `Completed`.
  - Otherwise runs all verifiers, emits one `GoalEvent::VerifierResult` per check, persists the report to `verifier_runs/`, and updates `metrics.verifiers_passed` / `verifiers_failed`.
  - All pass → `Completed`. Any fail → build `prompt::verifier_failure_message(&failures)` (which contains a per-failure summary including exit code + stdout/stderr excerpts), stash in `pending_continuation_override`, flip state back to `Running`, `continue` the outer loop. The model gets the failure feedback as the next turn's user prompt.

### Control / outcome refactor

- `ControlOutcome::VerifyNowOrStatus` (phase-2 placeholder) split into:
  - `VerifyNow` — explicit signal to run verifiers between turns.
  - `StatusReplied` — `StatusReq` was just answered; no further action.
- Both inner `drain_agent_events` and the outer iteration now handle these cleanly. `drain_agent_events` gained a `&mut pending_verify_now` so VerifyNow can be set even mid-stream.

### TUI

- `format_goal_event_line` now renders `VerifierResult` as `[goal#<id>] verify ✓|✗ <name> — <summary>`.

### What works end-to-end now

- `[features] goals = true` + a spec with `verifiers = [{ type = "shell", cmd = "test -f hello.txt", expect_exit = 0 }]` (Linux/macOS) or a `FileExists` verifier on Windows: the worker iterates until the file genuinely exists; only then does CLAIM_DONE survive the verification phase and the goal transition to `Completed`.
- A spec with a deliberately-failing verifier loops with verifier-failure feedback feeding the model — the model receives the actual exit code + stdout/stderr excerpt and adapts.
- `Policy::auto_approve = ReadOnly` causes every mutating/shell call to be auto-denied without a user prompt; the model sees `denied` and adapts.
- `Policy::shell_allowlist = ["^cargo "]` allows `cargo …` shell commands but denies everything else.
- `/goal verify <id>` while a worker is running emits a "verify_now queued" progress note and runs verifiers between the current and next turn; results show in the TUI and persist to `verifier_runs/`.
- `~/.forge-osh/goals/<id>/verifier_runs/*.json` accumulates one timestamped file per `run_all` invocation.

### Known limitations (deferred to phase 4)

- Policy enforcement uses `input_summary` heuristics (path tokens / shell-prefix stripping) instead of raw tool-call JSON args. Adequate for most cases but imperfect — phase 4 will plumb the JSON args through `PermissionRequest` so glob matching is exact.
- No status-bar indicator (still phase 4).
- No `--from PLAN.md`, no `/goal budget` subcommand, no cold-start resumer.
- `Verifying` state is not yet shown distinctly in the TUI status; users see the `StateChanged(Verifying)` event line and the individual `VerifierResult` events but no dedicated UI affordance.

---

## Phase 4 — Polish: resumer, plan files, budget tweaks, status bar, exact policy

### Ultimate goal of this phase

Close the productionisation gap left by phases 1-3. After phase 4:

1. **Crash-safe resume.** If forge-osh is killed while a goal is in `Running` / `Paused` / `Verifying` / `Blocked`, the next launch automatically re-spawns the worker from `index.json` + `spec.toml` + the latest checkpoint. The user sees their goals exactly where they left them.
2. **Spec files.** `/goal --from <path>` reads a TOML file as a `GoalSpec`, regenerates the id + created_at, and spawns. This unlocks the design's "PLAN.md → /goal" workflow without forcing inline objective text.
3. **In-flight budget edits.** `/goal budget <id> --max-turns N --max-wall 2h --max-input-tokens 500000 --max-output-tokens 80000` updates a live goal's budget. Useful for "I want this to keep going past the default cap I set."
4. **Exact policy enforcement.** `PermissionRequest` carries the raw `serde_json::Value` args alongside `input_summary`. `policy::evaluate` gains a second entry point that takes the value and does exact `glob::Pattern::matches` on every path-typed arg — no more heuristics. The phase-3 summary-based evaluator stays as a fallback when args are absent (e.g. for hand-built test paths).
5. **Status-bar indicator** in the TUI chrome: `● 2 goal(s) — 1 running, 1 verifying`. Tap into the existing ratatui status bar render path.
6. **Files_touched in checkpoints.** The worker already tracks files per turn; checkpoints now snapshot the running list so `/goal-check` can show what's been written.

### New file

- `src/agent/goal/resumer.rs` — `pub async fn resume_all(supervisor: &GoalSupervisor, deps: WorkerDeps)`. Reads `IndexFile`, filters entries whose state is non-terminal, loads each `spec.toml`, and calls `supervisor.respawn(spec, last_state)`. `respawn` is a new entry point on `GoalSupervisor` that creates a handle in the same way as `spawn` but seeds the state from disk instead of forcing `Running`.

### Edits

- `src/agent/loop.rs`:
  - Add `pub input: serde_json::Value` to `PermissionRequest`. Populate from `tc.input.clone()` at the construction site. Existing readers (TUI confirmation modal) ignore the new field — non-breaking.
- `src/agent/goal/policy.rs`:
  - Add `evaluate_with_args(tool_name, input_summary, args, level, policy, workdir) -> Decision`. Path-glob matching now operates on actual arg values (`path`, `file_path`, `filename`, `filepath`, `dir`, plus any value under `paths` array). Falls back to the summary heuristic only when no path-typed arg is present.
- `src/agent/goal/worker.rs`:
  - Responder task uses `evaluate_with_args(... &req.input ...)`.
  - `maybe_checkpoint` / `write_checkpoint_now` accept the running `files_touched: &[PathBuf]` and snapshot it into each `Checkpoint`. The `files_touched` Vec lives at the outer scope of `run_worker` (lifted from `drain_agent_events`) so subsequent turns and checkpoints can see the cumulative set.
- `src/agent/goal/supervisor.rs`:
  - Add `respawn(spec, state) -> GoalId` for the resumer.
- `src/tui/mod.rs`:
  - At TUI boot, after `set_deps`, call `goal::resumer::resume_all(...)`. Surface a system message for any goals that came back.
  - Slash command additions:
    - `/goal --from <path>` (parses TOML; respins id/created_at).
    - `/goal budget <id> --max-turns N --max-wall 2h --max-input-tokens N --max-output-tokens N`. The supervisor exposes `set_budget(id, budget)` which writes through to the live spec + spec.toml.
    - `/goal verifying-state` rendered distinctly (gold `⏳`).
  - Status-bar indicator: render `● <N> goal(s) — …` in the existing `renderer::render` chrome. We'll plumb a tiny pre-computed string into `AppState` (`goal_status_blurb: String`) and update it whenever a `GoalEvent::StateChanged` lands.

### Out of scope (kept for future work)

- Worktree-per-goal automatic spawning (still requires user-driven `git worktree add`).
- Multi-region cost reconciliation across worker-spawned MCP child sessions (each MCP child already has its own credentials per the connection model).
- LLM-judge verifiers (only deterministic shell / file / git verifiers today).

### Phase 4 success criteria

- Kill forge-osh while a goal is `Running`, restart — the goal continues from its last checkpoint (transcript replay handled by the existing per-goal `Session`; new turns build on top).
- A `spec.toml` with verifiers + custom policy can be loaded with `/goal --from ./my_spec.toml` and starts running.
- `Policy::write_globs = ["src/**"]` + a tool call that writes `Cargo.toml` is denied even though the summary mentions both paths (policy walks the JSON args, not just the text blurb).
- `/goal budget <id> --max-wall 8h` extends a live goal's wall budget without restarting it.
- The TUI chrome shows `● 2 goal(s) — 1 running, 1 verifying` when two goals are live.
- `/goal-check <id>` shows `Files touched (N): …` populated from the running snapshot.

---

## Phase 4 — completed 2026-05-17

`cargo check` clean (5.86s → 2.61s on incremental). All six phase-4 commitments landed.

### Raw args plumbed through `PermissionRequest`

- `src/agent/loop.rs` — `PermissionRequest` gains `pub input: serde_json::Value`, populated at the (single) construction site by capturing `tc.input.clone()` into the request-build closure. Non-breaking: existing readers (the TUI confirmation modal) ignore the new field.
- `src/agent/goal/policy.rs` — added `evaluate_with_args(tool_name, input_summary, args, level, policy)`. Path-glob enforcement walks the actual JSON args (`path`, `file_path`, `filename`, `filepath`, `dir`, `directory`, `target`, `target_file`, `src`, `source`, `dst`, `dest`, `destination`, and arrays under `paths`/`files`/`targets`). Every extracted path must match ≥1 `write_glob`; any path hitting `deny_globs` is a hard deny. Falls back to the heuristic `evaluate` only when no path-typed arg is found (so shell commands — which never live in args — still match via the `shell_allowlist` regex against `input_summary`). Phase-3 `evaluate` stays in place as the fallback.
- `src/agent/goal/worker.rs` — responder task now calls `evaluate_with_args(... &req.input ...)`.

### Files_touched in checkpoints

- `worker.rs` — `files_touched: Vec<PathBuf>` lifted to the outer `run_worker` scope and passed by `&mut` into `drain_agent_events`, so the list survives across turns.
- `write_checkpoint_now(..., files_touched: &[PathBuf])` and `maybe_checkpoint` both accept it; every Checkpoint snapshot now carries the cumulative list.
- `/goal-check` renders `Files (N): a, b, c (+M more)` (cap 8 inline) from `last_checkpoint.files_touched`.

### Cold-start resumer

- `src/agent/goal/resumer.rs` — `resume_all(supervisor)` reads `IndexFile`, filters entries whose `state.is_terminal()` is false, loads each `spec.toml`, and calls `supervisor.respawn(spec, seed_state)`. Paused goals stay paused (user must `/goal resume`); everything else respawns as Running. Returns a `ResumeReport { resumed, failed }`.
- `supervisor.rs` — new `respawn(spec, seed_state) -> GoalId`. Unlike `spawn`, it preserves id + created_at, seeds metrics from the existing `metrics.json` on disk (not zero), and starts the worker in the seed state.
- `tui/mod.rs` — at boot, after `set_deps`, calls `resume_all(...)` and surfaces a one-shot system message: `Resumed N goal(s): …` plus any failures. The status blurb is recomputed immediately so the indicator reflects the resumed goals.

### `/goal --from <path>`

- Reads a TOML file, deserialises into `GoalSpec`, **regenerates the id and created_at** so the same `spec.toml` can be reused across runs, and falls back to `cwd()` if `workdir` is empty.
- Errors are surfaced to the system message stream — read failures, TOML parse errors, and spawn errors all show inline.

### `/goal budget <id> [--max-turns N] [--max-wall <secs>] [--max-input-tokens N] [--max-output-tokens N]`

- Each flag is optional; missing flags leave the existing value alone.
- Loads the spec from disk, mutates the `Budget`, and writes back via `persist::save_spec`. The TUI surfaces what changed and notes that the running worker picks up the new caps at the next outer-loop iteration (turn boundary).
- Live in-memory replacement of `Arc<GoalSpec>` on a running handle is deliberately deferred to phase 5 — the persistence layer is the source of truth and worker re-spawn (or a process restart) currently picks up the new budget cleanly.

### Status-bar indicator

- `AppState` gains `goal_status_blurb: Arc<parking_lot::Mutex<String>>` (parking_lot, since the renderer is synchronous and must not block on tokio).
- `compute_goal_status_blurb(&supervisor)` summarises live goals into one line: `● 2 goal(s) — 1 running, 1 verifying` (also handles paused / blocked).
- Recomputed on every incoming `GoalEvent` (the event-drainer task already runs, so this costs basically nothing) and once at boot after the resumer.
- `renderer::render_status_bar` reads the blurb and appends it to the existing chrome line, immediately after the `🪄 skill` indicator and before scroll info. Empty string when no live goals → indicator disappears cleanly.

### TUI surface additions

- `cmd_goal` learned two new subcommands (`--from` and `budget`), wired alongside the existing list/pause/resume/clear/complete/verify/metrics/logs handlers.
- `format_goal_event_line` (added in phase 3) already covers VerifierResult; no changes needed in phase 4.
- `/goal-check` now renders the `Files (N): …` line under the last checkpoint summary.

### Verified

- `cargo check` is clean.
- The whole goal subsystem (Phases 1-4) is self-contained under `src/agent/goal/{mod, persistence, prompt, policy, verifier, worker, supervisor, resumer}.rs` plus small edits to `agent/loop.rs`, `agent/mod.rs`, `config/mod.rs`, `tui/mod.rs`, `tui/renderer.rs`.

### Known limitations (parked for phase 5+)

- Live in-memory budget hot-swap (currently requires worker re-spawn or process restart for changes to be picked up by an actively-running iteration). Workaround: budget changes apply to the next turn after a checkpoint, which is at most ~60s away.
- Worktree-per-goal automatic spawning.
- LLM-judge verifiers (only deterministic shell / file / git verifiers today).
- No web UI / mobile bridge (the design's "Hermes orchestrator" framing is still local-CLI-only).


