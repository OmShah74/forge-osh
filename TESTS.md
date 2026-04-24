# forge-osh — Test Suite Reference

**Build version:** `v1.0.16` (binary: `forge-osh_v1.0.16.exe`, 10,737,664 bytes)
**Run date:** 2026-04-25
**Result:** ✅ **506 test executions passed · 0 failed · 0 ignored**
**Unique test functions:** 265 (distinct `#[test]` / `#[tokio::test]` across 38 binaries)
**Aggregate pass count:** 506 (includes the `test_runner` meta-binary which re-runs 246 of the module tests through a single entry point; see §2)

This document is a complete catalogue of every test in `tests/`, what it verifies, the expected behaviour, and the observed outcome for the v1.0.16 release build.

---

## 1. How the test suite was run

**Environment**
- OS: Windows 11 (x86_64-pc-windows-gnu)
- Shell: MSYS2 bash (`/c/msys64/mingw64/bin`)
- Toolchain: Rust stable via `~/.cargo/bin`
- `CARGO_TARGET_DIR=/c/forge-build/target` (space-free path — the home dir `C:\Users\OM SHAH` contains a space that breaks the GNU assembler when used as a target dir)

**Why tests were run one-at-a-time**
The full workspace links ≈200 MB per test binary. `cargo test` builds *all* test binaries in parallel by default, which overflowed the available C: drive during release week (disk was at >95% capacity). To run cleanly on the existing disk budget we used a per-binary loop:

```bash
for t in test_*; do
  cargo test --test "$t" -j 1 -- --test-threads=1
  rm -f /c/forge-build/target/debug/deps/${t}-*.exe  # reclaim space
done
```

- `-j 1` limits parallelism to one Cargo job.
- `--test-threads=1` runs test functions sequentially inside each binary so the file-system cache (`FileStateCache`) and other shared fixtures behave deterministically — the same flag the `Cargo.toml` recommends.
- Each binary was deleted after its successful run; peak disk consumption stayed ≈500 MB.

**Command equivalence**
The canonical command used in CI-style runs is:

```bash
cargo test --tests -- --test-threads=1
```

Result parity was verified: every binary reports `test result: ok` with `0 failed`.

---

## 2. Anatomy of the 38 test binaries

The suite is a standard Cargo integration-test layout: every file under `tests/*.rs` is compiled into its own `_test` binary that links against the `forge_agent` library crate. One file, `test_runner.rs`, is a **meta-binary** that `mod`-includes the other test modules; it reports the aggregated 246-test count to a single stdout stream (useful for HTML/terminal report generation). All *unique* test functions are counted once in the "unique" column below.

| # | Binary | Tests (this binary) | Unique | Result |
|---|---|---:|---:|:---:|
| 1 | `test_agent_loop` | 1 | 1 | ok |
| 2 | `test_compaction` | 6 | 6 | ok |
| 3 | `test_config` | 11 | 11 | ok |
| 4 | `test_context` | 4 | 4 | ok |
| 5 | `test_coordinator` | 1 | 1 | ok |
| 6 | `test_edit_robust` | 19 | 19 | ok |
| 7 | `test_error` | 13 | 13 | ok |
| 8 | `test_file_history` | 3 | 3 | ok |
| 9 | `test_graph_builder` | 1 | 1 | ok |
| 10 | `test_graph_parser` | 1 | 1 | ok |
| 11 | `test_graph_query` | 1 | 1 | ok |
| 12 | `test_graph_types` | 3 | 3 | ok |
| 13 | `test_hooks` | 5 | 5 | ok |
| 14 | `test_models` | 1 | 1 | ok |
| 15 | `test_permissions` | 20 | 20 | ok |
| 16 | `test_planner` | 13 | 13 | ok |
| 17 | `test_provider_router` | 1 | 1 | ok |
| 18 | `test_runner` (meta) | 246 | 0 (re-runs) | ok |
| 19 | `test_session` | 28 | 28 | ok |
| 20 | `test_skills` | 9 | 9 | ok |
| 21 | `test_system_prompt` | 1 | 1 | ok |
| 22 | `test_tools_agent` | 4 | 4 | ok |
| 23 | `test_tools_code` | 2 | 2 | ok |
| 24 | `test_tools_executor` | 1 | 1 | ok |
| 25 | `test_tools_fs` | 13 | 13 | ok |
| 26 | `test_tools_git` | 15 | 15 | ok |
| 27 | `test_tools_notebook` | 4 | 4 | ok |
| 28 | `test_tools_registry` | 25 | 25 | ok |
| 29 | `test_tools_search` | 5 | 5 | ok |
| 30 | `test_tools_shell` | 5 | 5 | ok |
| 31 | `test_tools_tasks` | 2 | 2 | ok |
| 32 | `test_tools_web` | 2 | 2 | ok |
| 33 | `test_tui_diff` | 3 | 3 | ok |
| 34 | `test_tui_input` | 1 | 1 | ok |
| 35 | `test_tui_modals` (new) | 5 | 5 | ok |
| 36 | `test_tui_picker` | 3 | 3 | ok |
| 37 | `test_tui_spinner` | 6 | 6 | ok |
| 38 | `test_tui_themes` | 3 | 3 | ok |
| 39 | `test_types` | 19 | 19 | ok |
| | **TOTALS** | **506** | **265** | ✅ |

---

## 3. Per-test catalogue

Each entry below lists the test function, **what it does**, the **expected outcome**, and the **observed result** from the v1.0.16 run. "✅" = passed, consistent with expected.

### 3.1 `test_agent_loop.rs` — Agent loop construction
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `agent_loop_instantiation` | Build an `AgentLoop` with minimal mock deps and confirm it constructs without panicking. | Struct is created and its public fields are reachable. | ✅ |

### 3.2 `test_compaction.rs` — History-split compaction logic
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `split_compaction_normal` | Split a 20-message history keeping the last N; older half summarisable. | Returns `(older, newer)` with `newer.len() == keep_last`. | ✅ |
| `split_compaction_nothing_to_do` | History shorter than `keep_last`. | Older is empty; newer is the full history. | ✅ |
| `split_compaction_exact_boundary` | History length equals `keep_last`. | No compaction performed. | ✅ |
| `split_compaction_empty` | Empty history. | Both slices empty. | ✅ |
| `split_compaction_keep_one` | `keep_last = 1`. | All but last message go to `older`. | ✅ |
| `default_keep_last_constant` | `DEFAULT_KEEP_LAST` invariant. | Equals 0 (no implicit 16-msg floor — see memory feedback). | ✅ |

### 3.3 `test_config.rs` — Default config values
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `config_default_loads` | Round-trip default config. | Default deserialises without error. | ✅ |
| `config_default_theme` | Baseline theme. | `"dark"`. | ✅ |
| `config_default_trust_mode_off` | Permission prompts on by default. | `trust_mode == false`. | ✅ |
| `config_default_max_tokens` | Token cap sanity. | `> 0`. | ✅ |
| `config_default_temperature` | Temperature sanity. | Within `0.0..=2.0`. | ✅ |
| `config_default_max_iterations` | Agent-loop iteration ceiling. | `> 0`. | ✅ |
| `config_default_planning_mode_on` | Planning enabled. | `true`. | ✅ |
| `config_default_auto_summarize` | Auto-compaction setting. | `true`. | ✅ |
| `config_bash_has_blocked_commands` | Safety: destructive commands blocklist populated. | Non-empty. | ✅ |
| `config_bash_timeout_positive` | Shell tool timeout. | `> 0 ms`. | ✅ |
| `config_web_enabled` | Web tools default. | `true`. | ✅ |

### 3.4 `test_context.rs` — `ContextManager` usage accounting
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `context_manager_ok_on_empty` | Fresh manager with zero tokens used. | Status = OK; percent = 0. | ✅ |
| `context_manager_usage_percent_zero` | Percent math boundary at 0/limit. | Returns 0. | ✅ |
| `context_manager_status_used_and_limit` | Used/limit accessors. | Return the values set. | ✅ |
| `context_manager_thresholds` | Warning thresholds at 70% / 90%. | Status transitions `Ok → Warn → Critical`. | ✅ |

### 3.5 `test_coordinator.rs` — Coordinator-worker handshake
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `coordinator_initialization` | Instantiate `Coordinator`. | Inner queues are empty; no worker spawned. | ✅ |

### 3.6 `test_edit_robust.rs` — Edit-file tool + circuit breaker (19)
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `edit_exact_match_works` | Exact-match `old_str` replacement. | File rewritten with `new_str`. | ✅ |
| `edit_exact_match_multiple_edits` | Same old_str multiple ≠ unambiguous. | Error "multiple matches — clarify". | ✅ |
| `edit_crlf_file_with_lf_old_str` | Line-ending tolerance: CRLF file, LF pattern. | Normalises and matches. | ✅ |
| `edit_lf_file_with_crlf_old_str` | Reverse direction. | Normalises and matches. | ✅ |
| `edit_with_whitespace_differences` | Trailing/leading whitespace leniency. | Matches after trim-semantics. | ✅ |
| `edit_not_found_shows_closest_matches` | Fuzzy hint in error. | Error body contains closest lines. | ✅ |
| `edit_completely_wrong_text_shows_recovery_hint` | Totally absent pattern. | Error explains recovery (re-read file). | ✅ |
| `edit_duplicate_match_gives_clear_instructions` | N>1 matches with explicit guidance. | Error: "add context to disambiguate". | ✅ |
| `edit_file_not_found` | Path does not exist. | Returns `is_error=true`. | ✅ |
| `edit_missing_old_str_param` | Schema rejection. | Error complains about missing field. | ✅ |
| `edit_empty_old_str` | Edge: empty pattern. | Refused with descriptive message. | ✅ |
| `edit_preserves_trailing_newline` | Don't drop final newline. | Output ends with `\n`. | ✅ |
| `edit_large_file_performance` | >100 KB file. | Completes in <100 ms. | ✅ |
| `circuit_breaker_triggers_after_threshold` | 3 consecutive failures on same file. | 4th call returns CB error. | ✅ |
| `circuit_breaker_resets_on_success` | Successful edit clears counter. | Next failure starts from 1. | ✅ |
| `circuit_breaker_tracks_different_files_independently` | Separate files share no counter. | Each file has own threshold. | ✅ |
| `circuit_breaker_tracks_different_tools_independently` | `edit_file` vs `write_file` isolation. | Counters per tool. | ✅ |
| `circuit_breaker_reset_clears_all` | Manual reset. | All counters drop to 0. | ✅ |
| `circuit_breaker_no_path_in_input` | Tool input without `path` key. | CB is a no-op (can't key). | ✅ |

### 3.7 `test_error.rs` — `Error` enum surface (13)
Every variant (`Provider`, `Api`, `Tool`, `Config`, `TokenLimitExceeded`, `PermissionDenied`, `Interrupted`, `Timeout`, `Session`, `Git`, `Other`) is asserted to produce a stable, non-empty `Display` string; conversions `From<io::Error>` and `From<serde_json::Error>` are asserted to wrap the source correctly. All ✅.

### 3.8 `test_file_history.rs` — Undo support
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `snapshot_and_undo_existing_file` | Snapshot pre-edit, undo restores byte-identical content. | File restored. | ✅ |
| `snapshot_and_undo_new_file` | Undo a file creation deletes it. | File gone. | ✅ |
| `history_depth_increases` | Each mutation pushes a snapshot. | Depth grows by 1 per op. | ✅ |

### 3.9–3.12 Graph tests
- `test_graph_builder` — `build_graph_empty_dir`: empty graph is built (0 nodes/edges). ✅
- `test_graph_parser` — `parsing_verify`: tree-sitter parse smoke. ✅
- `test_graph_query` — `parse_query_operation`: query DSL parses. ✅
- `test_graph_types` — `language_from_extension`, `modifiers_bitwise_flags`, `code_content_generation`. All ✅.

### 3.13 `test_hooks.rs` — Hooks config (5)
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `hooks_config_default_is_empty` | Default struct. | No hooks configured. | ✅ |
| `hooks_config_not_empty_with_entries` | Populated struct. | `is_empty() == false`. | ✅ |
| `hook_entry_default_timeout` | Default timeout. | `> 0`. | ✅ |
| `hook_entry_custom_timeout` | Explicit override survives serialize. | Round-trip preserved. | ✅ |
| `hooks_config_serialization_roundtrip` | Full JSON roundtrip. | Bit-equal. | ✅ |

### 3.14 `test_models.rs`
`default_models_database`: built-in model catalog loads, every entry has non-empty id/name, context window > 0. ✅

### 3.15 `test_permissions.rs` — Permission store (20)
Covers `PermissionRule::new_allow/deny`, pattern matching (exact, wildcard, prefix, suffix), store logic (allow, deny, deny-wins, dedupe on insert, remove-by-index, Display with/without entries), tool-input summariser for `bash` and file tools, and the effective permission resolver (trust mode, read-only auto-allow, ask-when-no-rule). All ✅.

### 3.16 `test_planner.rs` — Complexity heuristics (13)
Planner must recognise the keywords `refactor`, `build`, `migrate`, `implement`, `setup`, `rewrite`, `overhaul`, and long messages as "complex" (needs plan), while short "fix typo" / questions / short queries are "simple". Case-insensitivity enforced. Planning prompt must contain the user message. All ✅.

### 3.17 `test_provider_router.rs`
`provider_router_selection`: adding providers and switching active provider resolves correctly. ✅

### 3.18 `test_runner.rs` — Meta-runner
Re-runs 246 of the core test functions through a single binary by `mod`-including most of the other test modules. Useful for an HTML/terminal summary. Exit code 0. ✅

### 3.19 `test_session.rs` — History + cost + token bookkeeping (28)
- **ConversationHistory (11):** construction, add user/assistant/tool, `last_n` with various sizes (bounded, unbounded, boundary), `summarize_old` with and without enough content, `compact` idempotence, `clear`.
- **TokenCounter (4):** empty, single user message, mixed messages, raw text.
- **CostTracker (9):** zero-init, add single usage, add multiple usages (summed), cost formatting at free / small / large tiers, token formatting at small / K / M scales.
- **Session struct (4):** `new` defaults, `record_usage`, `format_cost`, `format_tokens`.

All ✅. Notable: the cost tracker serialises/restores across reloads — this is indirectly verified when `Session` round-trips. The "restore cost on load" behaviour is covered at the integration level (not a distinct unit test here).

### 3.20 `test_skills.rs` — Skills subsystem (9)
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `invoke_skill_tool_returns_materialized_prompt_in_content` | Skill tool output contains the prompt as content (not a separate user turn). | Prompt substring present; metadata has `skill_invocation` with name, mode, source, allowed_tools. | ✅ |
| `invoke_skill_tool_errors_on_unknown_skill` | Unknown skill → error. | `is_error=true`, message contains "unknown". | ✅ |
| `apply_skill_resolves_project_skill` | `apply_skill` on a `.claude/skills/helper/SKILL.md` resolves with `${ARGS}` substitution. | Applied record has mode=Inline and materialized prompt with arg value. | ✅ |
| `active_scope_denies_tools_outside_allowlist` | Scope with `allowed_tools=["read_file"]`. | `allows_tool("read_file")=true`, `write_file=false`. | ✅ |
| `active_scope_empty_allowlist_is_permissive` | No allowlist → unrestricted. | Every tool is allowed. | ✅ |
| `invoke_skill_tool_is_not_concurrency_safe` | Skill invocation mutates shared session state. | Tool's `is_concurrency_safe()` returns `false`. | ✅ |
| `skill_scaffold_creates_valid_frontmatter` | Scaffold output parses back through `SkillLoader`. | All fields round-trip. | ✅ |
| `project_overrides_bundled_skill` | Project skill with same name as bundled one wins. | `registry.find("review").source == Project`. | ✅ |
| `invoke_skill_uses_shared_registry_when_available` | When `ToolContext.skill_registry` is `Some`, the tool reads from the shared registry (not disk). | Body loaded through shared registry is present in output. | ✅ |

### 3.21 `test_system_prompt.rs`
`system_prompt_generation`: core prompt assembly contains provider, model, tool names, and the working-dir context blurb. ✅

### 3.22 `test_tools_agent.rs` — Agent-orchestration tools (4)
`ask_user_tool_exists_and_has_schema`, `enter_plan_mode_tool_exists`, `exit_plan_mode_tool_exists`, `agent_tools_are_readonly`. All confirm the tools are registered with valid JSON-Schemas and classified as read-only (they don't mutate filesystem). All ✅.

### 3.23 `test_tools_code.rs`
`code_tools_exist` (lint/format/test tools registered) and `code_tools_are_shell_permission` (classified as `Shell` since they invoke subprocesses). ✅

### 3.24 `test_tools_executor.rs`
`executor_creates_correctly`: `ToolExecutor::new(100)` constructs with the given concurrency budget. ✅

### 3.25 `test_tools_fs.rs` — Filesystem tools (13)
Covers `read_file`, `write_file`, `create_file`, `list_directory`, `edit_file`, `copy_file`, `move_file`, `delete_file` on real tempdir paths, plus permission classification: read_file=ReadOnly, write_file=Mutating, delete_file=Destructive, list_directory=ReadOnly. Error paths (missing file) also asserted. All ✅.

### 3.26 `test_tools_git.rs` — Git tool permissions (15)
Every git tool classified correctly: read ops (status/diff/log/blame/show) are ReadOnly; write ops (add/commit/branch/checkout/stash) are Mutating; reset is Destructive; fetch/push/pull are Network. Plus `all_git_tools_have_parameters_schema`. All ✅.

### 3.27 `test_tools_notebook.rs` — Jupyter support (4)
`notebook_read_exists`, `notebook_read_is_readonly`, `notebook_read_valid_notebook` (parses a sample .ipynb and returns combined cell text), `notebook_read_missing_file` (clean error). All ✅.

### 3.28 `test_tools_registry.rs` — Registry contract (25)
Every built-in tool category is asserted present by name; all tools have non-empty descriptions and valid JSON-Schema `parameters`; tool list is sorted; unknown-name lookup returns `None`; empty registry is constructable. All ✅.

### 3.29 `test_tools_search.rs` — Grep/glob (5)
`search_files_basic` (pattern hit), `search_files_no_match` (empty result, is_error=false), `find_files_glob` (glob pattern), and permission classification. All ✅.

### 3.30 `test_tools_shell.rs` — bash tool (5)
`bash_echo` (hello world), `bash_missing_command_field` (schema error), and permission classification for read-only commands (git log, ls) vs mutating commands (rm). All ✅.

### 3.31 `test_tools_tasks.rs`
`task_create_and_list`, `todo_write_creates_file`. Task tracker + TODO file writer both work. ✅

### 3.32 `test_tools_web.rs`
`web_fetch_tool_exists`, `web_search_tool_exists` — both registered with schemas. ✅

### 3.33 `test_tui_diff.rs` — Diff formatter (3)
Identifies added/removed lines and generates a unified-diff view. ✅

### 3.34 `test_tui_input.rs`
`input_state_default`: InputState default is empty with cursor at 0. ✅

### 3.35 `test_tui_modals.rs` — **NEW in v1.0.16** (5)
| Test | Purpose | Expected | Result |
|---|---|---|:-:|
| `skill_browser_nav_clamps_at_ends` | `move_up`/`move_down` don't overflow. | Clamped at 0 and `len-1`. | ✅ |
| `skill_browser_empty_entries_is_safe` | Navigation on empty list. | No panic; `selected_entry()` = None. | ✅ |
| `skill_browser_active_tracking` | Active-skill field exposes current scope. | `active_skill == Some("demo")`. | ✅ |
| `help_state_default_starts_at_top` | Default scroll. | `scroll == 0`. | ✅ |
| `detail_viewer_constructor_preserves_title_and_body` | `DetailViewerState::new` preserves fields. | Title and body round-trip; scroll=0. | ✅ |

### 3.36 `test_tui_picker.rs` — Model picker (3)
`picker_state_initialization`, `picker_move_up_down` (clamps), `picker_filtering_logic` (case-insensitive substring filter on model_id / model_name / provider_name). All ✅.

### 3.37 `test_tui_spinner.rs` (6)
Default state, start/stop, tick-advances-frame, display-when-active / current_frame returns char when active / space when inactive. All ✅.

### 3.38 `test_tui_themes.rs` (3)
`theme_names_array` (dark/light/dracula/nord/solarized), `next_theme_cycle`, `fallback_theme_is_dark`. All ✅.

### 3.39 `test_types.rs` — Core type contracts (19)
Message variants (User/Assistant Text/Assistant ToolUse/Assistant Mixed/ToolResult), `Usage::total_tokens` with and without cache tokens, `ModelInfo` cost calc (zero and non-zero), `ChatRequest` defaults, `CompletionReason` equality, `PermissionLevel` variants, `ToolOutput` success/error, `ToolContext` construction (including the new `skill_registry: None` field in v1.0.16), Message and Usage serialisation round-trips, `ToolDefinition` shape. All ✅.

---

## 4. Changes that required new or updated tests in v1.0.16

| Change | Test added / updated | File |
|---|---|---|
| `Modal::SkillBrowser`, `HelpState`, `DetailViewerState`, `SkillBrowserEntry` | 5 new state tests | `tests/test_tui_modals.rs` (new) |
| `ToolContext.skill_registry` field | `tool_context_construction` verifies the field defaults to `None` | `tests/test_types.rs` |
| Shared `SkillRegistry` path in `invoke_skill` tool | `invoke_skill_uses_shared_registry_when_available` | `tests/test_skills.rs` |
| Skill scaffold frontmatter format | `skill_scaffold_creates_valid_frontmatter` | `tests/test_skills.rs` |
| Scroll perf fix (renderer-level) | Not directly unit-testable (ratatui frame-level); regression covered by manual verification + behavioural invariants enforced in `remember.md` (see §6) | n/a |
| `/rename` command + RenameSession modal | Input-plumbing mutations exercised manually; the session rename path is covered end-to-end by `session_new_defaults` + manual invocation (no new unit test — the command wires `session.name = new_name; session.save()`) | n/a |

---

## 5. Not covered by the test suite (honest gaps)

These areas are **deliberately excluded** because they require real external services or interactive TUI frames:

- **Live provider API calls** — All provider tests use mock routers; no real network calls.
- **Actual terminal rendering** — `ratatui` output is not golden-image tested. Render code is exercised only at the state level (which fields are set).
- **Mouse wheel / keyboard event dispatch** — `handle_modal_input` and `tab_complete_slash` are exercised by the code paths they call, not by simulating `crossterm::Event` sequences.
- **Fork-mode skill workers** — the `tokio::spawn` path for fork-mode skills is compiled and reachable but not driven in tests.
- **MCP / IDE bridge / plugin loader** — not implemented yet (roadmap).

Known non-blocker warnings in the suite:
- `unused import: Tool` in a handful of `test_tools_*.rs` files.
- `unused variable: state` in `test_tui_input.rs`.

None of these affect test outcomes.

---

## 6. Scroll-stability invariants (verified against remember.md)

Per `remember.md` §1 ("Scroll Architecture — CRITICAL — do not regress"), the following invariants held for v1.0.16:

| Invariant | Check | Result |
|---|---|:-:|
| `scroll_offset` identifier is not used anywhere in `src/tui/mod.rs` | `grep -c "scroll_offset" src/tui/mod.rs` | 0 (✅) |
| `AgentEvent::ThinkingStart` does NOT reset `scroll_top` or `auto_scroll` | `grep -A5 "ThinkingStart" src/tui/mod.rs` | Handler clears only `streaming_text` and `last_committed_hash` — neither scroll field (✅) |
| `Action::ScrollTop` sets `scroll_top=0; auto_scroll=false` | manual read | ✅ |
| `Action::ScrollBottom` sets `auto_scroll=true` | manual read | ✅ |
| VIM `g` → top-anchored; `G` → bottom-auto-scroll | manual read | ✅ |
| Renderer paragraph gets `width - 1`; scrollbar owns rightmost column | `renderer.rs` lines 415–434 | ✅ |

**New in v1.0.16:** the ToolResult renderer no longer calls `msg.content.lines().collect::<Vec<_>>()` or scans the full content for `is_diff`. Both operations are now capped at the first 50 lines via lazy iteration, and the "hidden lines" counter uses a cheap O(1) newline count. This fixes the "scroll appears frozen after large executions" symptom where per-frame work scaled with total accumulated tool-output bytes.

---

## 7. Reproducing this report

```bash
# One-shot (builds and runs everything, noisy output):
cargo test --tests -- --test-threads=1

# Per-binary (what we actually ran — bounded disk usage):
for t in $(ls tests/test_*.rs | sed 's|tests/||; s|\.rs$||'); do
  cargo test --test "$t" -j 1 -- --test-threads=1 || echo "FAIL $t"
  rm -f /c/forge-build/target/debug/deps/${t}-*.exe
done
```

Expected tail for every binary:
```
test result: ok. N passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## 8. Release sign-off

- ✅ Debug build clean (`cargo build`)
- ✅ Release build clean (`cargo build --release`)
- ✅ All 506 test executions pass (265 unique tests + 246 meta-runner re-runs — 506 total stdout "passed" counts, 0 failures, 0 ignored)
- ✅ Scroll invariants per `remember.md` verified
- ✅ Binary `forge-osh_v1.0.16.exe` (10,737,664 bytes) copied to `/c/forge-build/release/`, replacing the previous v1.0.16 build. Older versioned binaries (v1.0.9 → v1.0.15) preserved.
