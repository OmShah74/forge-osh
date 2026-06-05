//! Goal-contract system prompt block + protocol-marker parser.
//!
//! The worker injects this block into `config.general.system_prompt_extra`
//! when constructing its scoped AgentLoop, so the LLM knows it is operating
//! under a durable goal and must use the PROGRESS / BLOCKED / CLAIM_DONE
//! line protocol.

use super::{Budget, Checkpoint, GoalSpec, Policy, Verifier};

/// What the supervisor recognises in a streamed text line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolSignal {
    Progress(String),
    Blocked(String),
    ClaimDone(String),
}

/// Scan a single line of model output for a protocol marker. Markers must
/// appear at the start of a line (after any leading whitespace).
pub fn parse_protocol_line(raw: &str) -> Option<ProtocolSignal> {
    let s = raw.trim_start();
    if let Some(rest) = s.strip_prefix("PROGRESS:") {
        return Some(ProtocolSignal::Progress(rest.trim().to_string()));
    }
    if let Some(rest) = s.strip_prefix("BLOCKED:") {
        return Some(ProtocolSignal::Blocked(rest.trim().to_string()));
    }
    if let Some(rest) = s.strip_prefix("CLAIM_DONE:") {
        return Some(ProtocolSignal::ClaimDone(rest.trim().to_string()));
    }
    None
}

/// Scan accumulated text for *any* protocol markers since the last scan.
/// Returns each match plus the byte offset of the line in `text`. Caller is
/// expected to track which offsets it has already processed.
pub fn scan_all_signals(text: &str) -> Vec<(usize, ProtocolSignal)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some(sig) = parse_protocol_line(line) {
            out.push((offset, sig));
        }
        offset += line.len();
    }
    out
}

/// Build the goal-contract block to append to the system prompt.
pub fn build_goal_system_block(spec: &GoalSpec, last_ckpt: Option<&Checkpoint>) -> String {
    let mut out = String::new();
    out.push_str("\n\n## /goal mode (durable, autonomous, verifiable)\n");
    out.push_str(
        "You are running inside an autonomous /goal session. The user has \
         walked away — they are NOT monitoring this turn and will not \
         answer clarifying questions. Work toward the contract below until \
         the stopping condition is satisfied or you are genuinely blocked.\n\n",
    );

    out.push_str("### Contract\n");
    out.push_str(&format!("- **OBJECTIVE:** {}\n", spec.objective.trim()));
    if !spec.stopping_condition.trim().is_empty()
        && spec.stopping_condition.trim() != spec.objective.trim()
    {
        out.push_str(&format!(
            "- **STOPPING CONDITION:** {}\n",
            spec.stopping_condition.trim()
        ));
    }
    out.push_str(&format!("- **WORKDIR:** {}\n", spec.workdir.display()));

    // Verifiers (descriptive only — the worker runs them after CLAIM_DONE)
    if !spec.verifiers.is_empty() {
        out.push_str("- **VERIFIERS** (the supervisor — not you — runs these after you claim done):\n");
        for (i, v) in spec.verifiers.iter().enumerate() {
            out.push_str(&format!("    {}. {}\n", i + 1, format_verifier(v)));
        }
    } else {
        out.push_str(
            "- **VERIFIERS:** none configured. Because there is no automated check, you MUST \
             self-verify empirically before claiming done: actually run the project's build \
             and/or tests with your shell tools (e.g. `cargo build`/`cargo test`, `npm test`, \
             `pytest`) and read the real output. Quote the command you ran and its result in \
             your CLAIM_DONE. Do not claim done on the basis of having written code alone.\n",
        );
    }

    // Budget summary
    out.push_str(&format_budget(&spec.budget));

    // Policy summary
    out.push_str(&format_policy(&spec.policy));

    out.push_str(
        "\n### Line protocol — emit these prefixes on their own lines\n\
         - `PROGRESS: <one-line description of what you just did or are doing now>` \
         after every meaningful action (a tool call, a decision, a discovery). \
         The supervisor appends each one to progress.log; the user reads it via \
         `/goal logs <id>` and `/goal-check <id>`.\n\
         - `BLOCKED: <reason>` if you cannot proceed without human help. The \
         supervisor will park the goal until the user resumes it.\n\
         - `CLAIM_DONE: <one-paragraph summary of what you accomplished>` \
         when you believe the stopping condition is satisfied. The supervisor \
         will then run the verifiers. If any verifier fails, you will receive \
         the failure output and must continue.\n\n",
    );

    out.push_str(
        "### Working style\n\
         - Plan in checkpoints — small verifiable steps, not one giant edit.\n\
         - Prefer reading before writing. Use search/grep to confirm assumptions.\n\
         - Never ask the user clarifying questions in this mode — they cannot answer. \
         If you genuinely cannot proceed, emit BLOCKED: and stop.\n\
         - Do NOT spam PROGRESS for trivial internal thinking — one line per \
         externally-visible action is the right granularity.\n\
         - Do NOT emit a CLAIM_DONE just because you finished one step; only when \
         the entire stopping condition is satisfied.\n\n",
    );

    // ── Execution strategy (single-thread vs sub-team) ───────────────────
    out.push_str(
        "### Choosing an execution strategy\n\
         First decide HOW to execute, based on the objective's shape:\n\
         - Default to working SEQUENTIALLY in this loop (use `update_plan` to track steps). This \
         is correct for most goals and for tightly-coupled changes.\n\
         - If the objective decomposes into genuinely INDEPENDENT, parallelizable pieces (e.g. \
         investigate/modify several disjoint files or modules), call `spawn_team(strategy=\"swarm\", \
         subtasks=[…])` to run them concurrently; sub-agents coordinate via the shared blackboard.\n\
         - If the pieces are parallel but must be INTEGRATED coherently (a feature spanning shared \
         code), use `spawn_team(strategy=\"orchestrator\", subtasks=[…])` so a review agent \
         reconciles them.\n\
         - Use `spawn_team(strategy=\"sequential\", …)` only when isolated ordered sub-agents help.\n\
         `spawn_team` blocks and returns the merged results; fold them into your own verification \
         before you CLAIM_DONE. Do NOT spawn a team for small or tightly-coupled work — the \
         overhead and conflict risk are not worth it. You own the final result either way.\n\n",
    );

    // ── Anti-reward-hacking integrity contract ───────────────────────────
    out.push_str(
        "### INTEGRITY — do the real work, never game the check\n\
         The verifiers and stopping condition exist to confirm the OBJECTIVE is genuinely \
         met. They are a measurement, not the goal. The following are STRICTLY FORBIDDEN and \
         count as failing the goal, even if they make a check pass:\n\
         - Editing, deleting, disabling, skipping, `#[ignore]`-ing, commenting out, or \
         weakening tests, assertions, or any verifier target so it passes.\n\
         - Hardcoding expected outputs, stubbing functions to return the expected value, \
         special-casing the test's inputs, or faking data instead of implementing the real \
         behaviour.\n\
         - Modifying the verifier commands, CI config, or the stopping condition itself; \
         using `--no-verify`, `|| true`, forced exit codes, or env hacks to mask failures.\n\
         - Claiming done on work you did not actually perform or verify, or reporting \
         artifacts/results that do not reflect the real repository state.\n\
         If the real objective cannot be achieved within the budget, emit `BLOCKED:` with an \
         honest explanation — that is always preferable to a check that passes for the wrong \
         reason. Make the implementation correct so the checks pass *as a consequence*.\n",
    );

    if let Some(c) = last_ckpt {
        out.push_str("\n### Last checkpoint (resumed from here)\n");
        out.push_str(&format!(
            "- phase: {}\n- last action: {}\n- progress: {}\n",
            c.phase, c.last_action, c.progress_blurb
        ));
    }

    out
}

fn format_verifier(v: &Verifier) -> String {
    match v {
        Verifier::Shell { cmd, expect_exit, expect_stdout_contains } => {
            let extra = expect_stdout_contains
                .as_ref()
                .map(|s| format!(", stdout must contain '{}'", truncate(s, 30)))
                .unwrap_or_default();
            format!("shell `{}` (expect exit {}){}", truncate(cmd, 60), expect_exit, extra)
        }
        Verifier::FileExists { path } => format!("file must exist: {}", path.display()),
        Verifier::FileContains { path, needle } => format!(
            "{} must contain '{}'",
            path.display(),
            truncate(needle, 30)
        ),
        Verifier::NoUncommittedFiles { except } => {
            if except.is_empty() {
                "git tree must be clean".into()
            } else {
                format!("git tree must be clean except: {}", except.join(", "))
            }
        }
        Verifier::Custom { name, cmd } => format!("{name}: `{}`", truncate(cmd, 60)),
    }
}

fn format_budget(b: &Budget) -> String {
    let mut s = String::from("- **BUDGET:** ");
    let mut parts = Vec::new();
    if let Some(t) = b.max_turns {
        parts.push(format!("{} turns", t));
    }
    if let Some(d) = b.max_wall {
        parts.push(format!("{}s wall", d.as_secs()));
    }
    if let Some(t) = b.max_input_tokens {
        parts.push(format!("{} input tok", t));
    }
    if let Some(t) = b.max_output_tokens {
        parts.push(format!("{} output tok", t));
    }
    if parts.is_empty() {
        s.push_str("none (run until done)\n");
    } else {
        s.push_str(&parts.join(", "));
        s.push('\n');
    }
    s.push_str("- (cost is observed but never enforced — no cost ceiling)\n");
    s
}

fn format_policy(p: &Policy) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "- **POLICY:** auto_approve={:?}, network={}\n",
        p.auto_approve, p.network
    ));
    if !p.write_globs.is_empty() {
        s.push_str(&format!("    writable globs: {}\n", p.write_globs.join(", ")));
    }
    if !p.deny_globs.is_empty() {
        s.push_str(&format!("    DENY (never touch): {}\n", p.deny_globs.join(", ")));
    }
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

// ---------------------------------------------------------------------------
// First-user-message helpers
// ---------------------------------------------------------------------------

pub fn initial_user_message(spec: &GoalSpec) -> String {
    format!(
        "I have set you a goal (see /goal mode contract in the system prompt). \
         Begin working on it now. The objective is:\n\n{}\n\n\
         Remember the line protocol: emit PROGRESS:, BLOCKED:, or CLAIM_DONE: \
         lines as appropriate. I have walked away — work autonomously.",
        spec.objective.trim()
    )
}

pub fn continuation_message() -> String {
    "Continue working toward the goal. Emit PROGRESS: / BLOCKED: / CLAIM_DONE: \
     lines as appropriate. Do not ask me questions — I am not here."
        .to_string()
}

pub fn verifier_failure_message(failures: &[(String, String)]) -> String {
    let mut s =
        String::from("Verification failed after your CLAIM_DONE. Fix and re-claim:\n\n");
    for (name, detail) in failures {
        s.push_str(&format!("  ✗ {name}\n    {}\n", detail.trim()));
    }
    s.push_str(
        "\nContinue working until every verifier passes, then emit CLAIM_DONE: again.",
    );
    s
}
