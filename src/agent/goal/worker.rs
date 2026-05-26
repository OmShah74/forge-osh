//! Phase-2 autonomous worker for the /goal primitive.
//!
//! Strategy: rather than reimplementing the entire LLM + tool-call loop
//! (which would duplicate ~1000 lines of `agent::loop`), we *reuse* the
//! existing `AgentLoop` but give it:
//!
//! 1. its own scoped `Session` (so it has its own conversation transcript
//!    and its own cost tracker — never mingled with the user's),
//! 2. its own `Config` clone whose `general.system_prompt_extra` is
//!    augmented with the goal-contract block from `prompt.rs`,
//! 3. its own `event_tx` / `event_rx` pair so the worker can scan token
//!    output for `PROGRESS:` / `BLOCKED:` / `CLAIM_DONE:` markers,
//! 4. `permission_mode = Bypass` so the agent auto-approves tool calls
//!    (Phase 3 will tighten this with policy-based gating).
//!
//! The worker then iterates:
//!   - check budget / pause / clear
//!   - spawn `agent_loop.run(message)` as a background task
//!   - drain its events in real time, parsing markers
//!   - when the run returns, update metrics, write checkpoint
//!   - if `CLAIM_DONE` was seen → transition to Completed (verifier
//!     execution arrives in Phase 3)
//!   - if `BLOCKED` was seen → transition to Blocked
//!   - otherwise loop with a continuation message

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::RwLock as PlRwLock;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::agent::{permissions::PermissionStore, AgentEvent, AgentLoop, PermissionRequest};
use crate::config::Config;
use crate::graph::SharedGraph;
use crate::lsp::SharedLspManager;
use crate::provider::router::ProviderRouter;
use crate::session::{FileStateCache, Session};
use crate::skills::SharedSkillRegistry;
use crate::tools::ToolRegistry;
use crate::types::{PermissionMode, PermissionResponse, ThinkingConfig};

use super::persistence as persist;
use super::policy;
use super::prompt::{self, ProtocolSignal};
use super::verifier;
use super::{Checkpoint, GoalControl, GoalEvent, GoalId, GoalMetrics, GoalSpec, GoalState};

/// Shared dependencies the supervisor hands to every worker. Cloned (Arc)
/// per goal — no exclusive resources.
#[derive(Clone)]
pub struct WorkerDeps {
    pub provider_router: Arc<RwLock<ProviderRouter>>,
    pub tools: Arc<ToolRegistry>,
    pub config: Arc<Config>,
    pub graph: SharedGraph,
    pub lsp: SharedLspManager,
    pub file_cache: Arc<FileStateCache>,
    pub permission_store: Arc<PlRwLock<PermissionStore>>,
    pub skill_registry: SharedSkillRegistry,
}

/// What the supervisor passes in per-goal.
pub struct WorkerCtx {
    pub id: GoalId,
    pub spec: Arc<GoalSpec>,
    pub state: Arc<RwLock<GoalState>>,
    pub metrics: Arc<RwLock<GoalMetrics>>,
    pub events_tx: mpsc::UnboundedSender<(GoalId, GoalEvent)>,
    pub control_rx: mpsc::UnboundedReceiver<GoalControl>,
    pub resume_notify: Arc<tokio::sync::Notify>,
}

// Checkpoint cadence
const CHECKPOINT_EVERY_TOOLS: u32 = 5;
const CHECKPOINT_EVERY_SECS: u64 = 60;
const MAX_PROGRESS_LINES_PER_TURN: usize = 64;

pub async fn run_worker(mut ctx: WorkerCtx, deps: WorkerDeps) {
    let _ = ctx
        .events_tx
        .send((ctx.id.clone(), GoalEvent::Started { id: ctx.id.clone() }));

    let started_at = Instant::now();
    let mut last_checkpoint_at = Instant::now();
    let mut tools_since_checkpoint: u32 = 0;
    let mut turn: u32 = 0;
    let mut had_claim_done_summary: Option<String> = None;
    let mut blocked_reason: Option<String> = None;
    // After a CLAIM_DONE that fails verification, this holds the synthetic
    // failure-feedback message that becomes the next turn's user prompt.
    let mut pending_continuation_override: Option<String> = None;
    // Set by VerifyNow control between turns; consumed in the outer loop.
    let mut pending_verify_now = false;
    // Cumulative list of files the worker has touched across all turns.
    // Snapshotted into every Checkpoint so /goal-check shows it.
    let mut files_touched: Vec<PathBuf> = Vec::new();

    // ── Build the goal's scoped Session ───────────────────────────────────
    let (provider_id, model_id) = {
        let r = deps.provider_router.read().await;
        (
            r.active_provider_id().to_string(),
            r.active_model_id().to_string(),
        )
    };
    let session_name = format!("goal-{}", ctx.id.as_str());
    let session = Arc::new(Mutex::new(Session::new(
        session_name,
        provider_id,
        model_id,
        ctx.spec.workdir.to_string_lossy().to_string(),
    )));

    // ── Build a Config clone with the goal-contract appended ─────────────
    let goal_config: Arc<Config> = {
        let mut cfg = (*deps.config).clone();
        let extra = prompt::build_goal_system_block(&ctx.spec, None);
        if cfg.general.system_prompt_extra.is_empty() {
            cfg.general.system_prompt_extra = extra;
        } else {
            cfg.general.system_prompt_extra.push_str(&extra);
        }
        Arc::new(cfg)
    };

    // ── Per-goal event channel ───────────────────────────────────────────
    let (agent_event_tx, mut agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    // Per-goal permission channels. With permission_mode = Default we DO
    // receive requests; a responder task evaluates each via the goal's
    // Policy and answers automatically (the user is never prompted).
    let (perm_req_tx, mut perm_req_rx) =
        mpsc::unbounded_channel::<PermissionRequest>();
    // perm_resp_rx is supplied to AgentLoop for backwards compatibility with
    // the persistent-allow ("AlwaysAllow") path; for goal mode we always
    // reply via the per-request `response_tx` oneshot so this channel is
    // never used. Construct it once so the type matches.
    let (_perm_resp_tx, perm_resp_rx) = mpsc::unbounded_channel::<PermissionResponse>();

    // Spawn the policy responder task.
    {
        let policy_snapshot = ctx.spec.policy.clone();
        let events_tx_clone = ctx.events_tx.clone();
        let id_clone = ctx.id.clone();
        tokio::spawn(async move {
            while let Some(req) = perm_req_rx.recv().await {
                let decision = policy::evaluate_with_args(
                    &req.tool_name,
                    &req.input_summary,
                    &req.input,
                    &req.level,
                    &policy_snapshot,
                );
                let response = match &decision {
                    policy::Decision::Allow => PermissionResponse::Allow,
                    policy::Decision::Deny(_) => PermissionResponse::Deny,
                };
                if let policy::Decision::Deny(reason) = &decision {
                    let _ = events_tx_clone.send((
                        id_clone.clone(),
                        GoalEvent::Progress {
                            line: format!(
                                "policy DENY: {} ({}) — {}",
                                req.tool_name, req.input_summary, reason
                            ),
                        },
                    ));
                    let _ = persist::append_progress(
                        &id_clone,
                        &format!(
                            "POLICY: deny {} ({}) — {}",
                            req.tool_name, req.input_summary, reason
                        ),
                    );
                }
                let _ = req.response_tx.send(response);
            }
        });
    }

    let cancel_token = Arc::new(PlRwLock::new(CancellationToken::new()));
    let permission_mode = Arc::new(PlRwLock::new(PermissionMode::Default));
    let thinking = Arc::new(PlRwLock::new(ThinkingConfig::Disabled));

    let agent_loop = Arc::new(AgentLoop {
        provider_router: deps.provider_router.clone(),
        tools: deps.tools.clone(),
        session: session.clone(),
        config: goal_config.clone(),
        event_tx: agent_event_tx,
        permission_tx: perm_req_tx,
        permission_rx: Arc::new(Mutex::new(perm_resp_rx)),
        graph: deps.graph.clone(),
        lsp: deps.lsp.clone(),
        file_cache: deps.file_cache.clone(),
        permission_store: deps.permission_store.clone(),
        cancel: cancel_token.clone(),
        permission_mode,
        thinking,
        skill_registry: deps.skill_registry.clone(),
        // Goal workers run in the background; live tool output is not
        // surfaced to any UI for them.
        output_chunk_tx: None,
    });

    // Initial state persistence + first checkpoint
    persist_state(&ctx).await;
    write_initial_checkpoint(&ctx).await;

    // ── Outer iteration ──────────────────────────────────────────────────
    let result_state: GoalState = 'outer: loop {
        // Drain any pending control signals (non-blocking).
        if let Some(early) = drain_control(
            &mut ctx.control_rx,
            &ctx.id,
            &ctx.spec,
            &ctx.state,
            &ctx.metrics,
            &ctx.events_tx,
            &cancel_token,
        )
        .await
        {
            match early {
                ControlOutcome::Pause => {
                    // Park until resumed / cleared.
                    {
                        let mut s = ctx.state.write().await;
                        *s = GoalState::Paused;
                    }
                    let _ = ctx
                        .events_tx
                        .send((ctx.id.clone(), GoalEvent::StateChanged(GoalState::Paused)));
                    persist_state(&ctx).await;
                    let _ = persist::append_progress(&ctx.id, "PROGRESS: paused by user");
                    ctx.resume_notify.notified().await;
                    if matches!(*ctx.state.read().await, GoalState::Cleared) {
                        break 'outer GoalState::Cleared;
                    }
                    let _ = ctx
                        .events_tx
                        .send((ctx.id.clone(), GoalEvent::StateChanged(GoalState::Running)));
                    let _ = persist::append_progress(&ctx.id, "PROGRESS: resumed by user");
                }
                ControlOutcome::Clear => {
                    break 'outer GoalState::Cleared;
                }
                ControlOutcome::ForceComplete => {
                    break 'outer GoalState::Completed;
                }
                ControlOutcome::VerifyNow => {
                    pending_verify_now = true;
                }
                ControlOutcome::StatusReplied => {
                    // Status snapshot was sent back to the caller.
                }
            }
        }

        // If a /goal verify came in, run verifiers without changing terminal state.
        if pending_verify_now {
            pending_verify_now = false;
            let prior_state = ctx.state.read().await.clone();
            let _ = run_verification_phase(&ctx).await;
            // Restore prior state.
            {
                let mut s = ctx.state.write().await;
                *s = prior_state.clone();
            }
            let _ = ctx
                .events_tx
                .send((ctx.id.clone(), GoalEvent::StateChanged(prior_state)));
            persist_state(&ctx).await;
        }

        // Budget enforcement
        if let Some(reason) = check_budget(&ctx, started_at, turn).await {
            let _ = ctx.events_tx.send((
                ctx.id.clone(),
                GoalEvent::BudgetWarn {
                    kind: reason.clone(),
                    used: 0.0,
                    limit: 0.0,
                },
            ));
            blocked_reason = Some(format!("budget exhausted: {reason}"));
            break 'outer GoalState::Blocked(blocked_reason.clone().unwrap());
        }

        turn += 1;
        // Compose user message — failure-feedback overrides the default
        // continuation when verification just failed.
        let user_msg = if let Some(msg) = pending_continuation_override.take() {
            msg
        } else if turn == 1 {
            prompt::initial_user_message(&ctx.spec)
        } else {
            prompt::continuation_message()
        };

        // Fresh cancel token per turn (so /goal clear can interrupt cleanly).
        {
            let mut tok = cancel_token.write();
            *tok = CancellationToken::new();
        }

        // Spawn the agent run as a background task so we can drain its
        // event stream in real time.
        let al = agent_loop.clone();
        let task = tokio::spawn(async move { al.run(user_msg).await });

        // Drain events while the run is in flight.
        let drain_outcome = drain_agent_events(
            &mut agent_event_rx,
            &mut ctx,
            &cancel_token,
            &mut tools_since_checkpoint,
            &mut last_checkpoint_at,
            &mut pending_verify_now,
            &mut files_touched,
        )
        .await;

        // Whatever happens, await the task so its lifetime ends.
        let _ = task.await;

        // Refresh metrics from the session's cost tracker.
        refresh_metrics(&ctx, &session, started_at).await;
        let _ = persist::save_metrics(&ctx.id, &*ctx.metrics.read().await);
        persist_state(&ctx).await;

        // Write a per-turn checkpoint regardless of cadence.
        write_checkpoint_now(
            &ctx,
            "between-turns",
            &format!("turn {turn} finished"),
            "turn boundary",
            &files_touched,
        )
        .await;
        tools_since_checkpoint = 0;
        last_checkpoint_at = Instant::now();

        match drain_outcome {
            DrainOutcome::ClaimDone(summary) => {
                had_claim_done_summary = Some(summary.clone());
                let _ = persist::append_progress(
                    &ctx.id,
                    &format!("CLAIM_DONE: {} — running verifiers", summary),
                );
                let verify_outcome = run_verification_phase(&ctx).await;
                match verify_outcome {
                    VerifyOutcome::AllPass => {
                        break 'outer GoalState::Completed;
                    }
                    VerifyOutcome::NoVerifiers => {
                        // Honor the design's "trust CLAIM_DONE when no
                        // verifiers configured" rule.
                        break 'outer GoalState::Completed;
                    }
                    VerifyOutcome::SomeFailed(failure_msg) => {
                        pending_continuation_override = Some(failure_msg);
                        // Flip back to Running for the next iteration.
                        {
                            let mut s = ctx.state.write().await;
                            *s = GoalState::Running;
                        }
                        let _ = ctx.events_tx.send((
                            ctx.id.clone(),
                            GoalEvent::StateChanged(GoalState::Running),
                        ));
                        continue;
                    }
                }
            }
            DrainOutcome::Blocked(reason) => {
                blocked_reason = Some(reason.clone());
                break 'outer GoalState::Blocked(reason);
            }
            DrainOutcome::Cancelled => {
                break 'outer GoalState::Cleared;
            }
            DrainOutcome::Paused => {
                // Pause was handled in-line by drain_agent_events flipping
                // state to Paused and persisting. Park.
                ctx.resume_notify.notified().await;
                if matches!(*ctx.state.read().await, GoalState::Cleared) {
                    break 'outer GoalState::Cleared;
                }
                let _ = ctx
                    .events_tx
                    .send((ctx.id.clone(), GoalEvent::StateChanged(GoalState::Running)));
                let _ = persist::append_progress(&ctx.id, "PROGRESS: resumed by user");
                continue;
            }
            DrainOutcome::Finished => {
                // The agent loop returned without a CLAIM_DONE marker. This
                // happens when the model just stops without claiming done.
                // We loop and feed a continuation message until either
                // CLAIM_DONE, BLOCKED, or budget exhaustion.
                continue;
            }
            DrainOutcome::Errored(e) => {
                blocked_reason = Some(format!("agent loop errored: {e}"));
                break 'outer GoalState::Blocked(blocked_reason.clone().unwrap());
            }
        }
    };

    // ── Termination housekeeping ─────────────────────────────────────────
    {
        let mut s = ctx.state.write().await;
        *s = result_state.clone();
    }
    refresh_metrics(&ctx, &session, started_at).await;
    let _ = persist::save_metrics(&ctx.id, &*ctx.metrics.read().await);
    persist_state(&ctx).await;
    let _ = ctx
        .events_tx
        .send((ctx.id.clone(), GoalEvent::StateChanged(result_state.clone())));

    match &result_state {
        GoalState::Completed => {
            if let Some(s) = &had_claim_done_summary {
                let _ = persist::append_progress(
                    &ctx.id,
                    &format!("PROGRESS: CLAIM_DONE — {}", s),
                );
            } else {
                let _ = persist::append_progress(&ctx.id, "PROGRESS: completed");
            }
            let m = ctx.metrics.read().await.clone();
            let _ = ctx
                .events_tx
                .send((ctx.id.clone(), GoalEvent::Completed { metrics: m }));
        }
        GoalState::Blocked(reason) => {
            let _ =
                persist::append_progress(&ctx.id, &format!("PROGRESS: blocked — {reason}"));
            let _ = ctx.events_tx.send((
                ctx.id.clone(),
                GoalEvent::Blocked {
                    reason: reason.clone(),
                },
            ));
        }
        GoalState::Cleared => {
            let _ = persist::append_progress(&ctx.id, "PROGRESS: cleared");
        }
        _ => {}
    }

    let _ = blocked_reason; // silence unused-on-completed branch
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum ControlOutcome {
    Pause,
    Clear,
    ForceComplete,
    VerifyNow,
    StatusReplied,
}

async fn drain_control(
    rx: &mut mpsc::UnboundedReceiver<GoalControl>,
    id: &GoalId,
    spec: &Arc<GoalSpec>,
    state: &Arc<RwLock<GoalState>>,
    metrics: &Arc<RwLock<GoalMetrics>>,
    events_tx: &mpsc::UnboundedSender<(GoalId, GoalEvent)>,
    cancel: &Arc<PlRwLock<CancellationToken>>,
) -> Option<ControlOutcome> {
    let mut out = None;
    while let Ok(c) = rx.try_recv() {
        match c {
            GoalControl::Pause => {
                cancel.read().cancel();
                out = Some(ControlOutcome::Pause);
            }
            GoalControl::Resume => {
                // Resume is meaningful only when paused; supervisor handles flip.
            }
            GoalControl::Clear => {
                cancel.read().cancel();
                return Some(ControlOutcome::Clear);
            }
            GoalControl::VerifyNow => {
                let _ = events_tx.send((
                    id.clone(),
                    GoalEvent::Progress {
                        line: "verify_now queued — verifiers will run after current turn"
                            .into(),
                    },
                ));
                out = Some(ControlOutcome::VerifyNow);
            }
            GoalControl::ForceComplete => {
                cancel.read().cancel();
                return Some(ControlOutcome::ForceComplete);
            }
            GoalControl::StatusReq(tx) => {
                let snap = super::StatusSnapshot {
                    id: id.clone(),
                    state: state.read().await.clone(),
                    spec_objective: spec.objective.clone(),
                    spec_stopping: spec.stopping_condition.clone(),
                    metrics: metrics.read().await.clone(),
                    last_checkpoint: persist::load_latest_checkpoint(id).ok().flatten(),
                    tail_progress: persist::tail_progress(id, 10).unwrap_or_default(),
                };
                let _ = tx.send(snap);
                if out.is_none() {
                    out = Some(ControlOutcome::StatusReplied);
                }
            }
        }
    }
    out
}

async fn check_budget(ctx: &WorkerCtx, started_at: Instant, turn: u32) -> Option<String> {
    let b = &ctx.spec.budget;
    if let Some(max) = b.max_turns {
        if turn >= max {
            return Some(format!("turns ({max})"));
        }
    }
    if let Some(max) = b.max_wall {
        if started_at.elapsed() > max {
            return Some(format!("wall ({}s)", max.as_secs()));
        }
    }
    let m = ctx.metrics.read().await;
    if let Some(max) = b.max_input_tokens {
        if m.input_tokens >= max {
            return Some(format!("input tokens ({max})"));
        }
    }
    if let Some(max) = b.max_output_tokens {
        if m.output_tokens >= max {
            return Some(format!("output tokens ({max})"));
        }
    }
    None
}

#[derive(Debug)]
enum DrainOutcome {
    Finished,
    ClaimDone(String),
    Blocked(String),
    Paused,
    Cancelled,
    Errored(String),
}

/// Drain the agent loop's event stream until it emits `Done` or `Error`,
/// scanning text for protocol markers as we go.
async fn drain_agent_events(
    rx: &mut mpsc::UnboundedReceiver<AgentEvent>,
    ctx: &mut WorkerCtx,
    cancel: &Arc<PlRwLock<CancellationToken>>,
    tools_since_checkpoint: &mut u32,
    last_checkpoint_at: &mut Instant,
    pending_verify_now: &mut bool,
    files_touched: &mut Vec<PathBuf>,
) -> DrainOutcome {
    // Per-turn buffer for text — we re-scan from the last-seen offset to
    // catch markers that span chunk boundaries.
    let mut buf = String::new();
    let mut scan_from: usize = 0;
    let mut progress_emitted: usize = 0;
    let mut latest_tool: Option<String> = None;

    let mut errored: Option<String> = None;

    loop {
        // Check control signals between chunks (cooperative).
        if let Some(out) = drain_control(
            &mut ctx.control_rx,
            &ctx.id,
            &ctx.spec,
            &ctx.state,
            &ctx.metrics,
            &ctx.events_tx,
            cancel,
        )
        .await
        {
            match out {
                ControlOutcome::Pause => {
                    {
                        let mut s = ctx.state.write().await;
                        *s = GoalState::Paused;
                    }
                    let _ = ctx
                        .events_tx
                        .send((ctx.id.clone(), GoalEvent::StateChanged(GoalState::Paused)));
                    persist_state(ctx).await;
                    let _ = persist::append_progress(&ctx.id, "PROGRESS: paused by user");
                    return DrainOutcome::Paused;
                }
                ControlOutcome::Clear => return DrainOutcome::Cancelled,
                ControlOutcome::ForceComplete => {
                    return DrainOutcome::ClaimDone("forced complete".into())
                }
                ControlOutcome::VerifyNow => {
                    *pending_verify_now = true;
                }
                ControlOutcome::StatusReplied => {
                    // Status was sent back inline.
                }
            }
        }

        // Wait for next event with a short timeout so we can revisit
        // control / checkpoint cadence.
        let ev = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
        let Ok(maybe_ev) = ev else {
            // timeout: maybe write a checkpoint, then loop.
            maybe_checkpoint(
                ctx,
                tools_since_checkpoint,
                last_checkpoint_at,
                latest_tool.as_deref(),
                &files_touched,
            )
            .await;
            continue;
        };
        let Some(event) = maybe_ev else {
            // Channel closed.
            break;
        };

        match event {
            AgentEvent::ThinkingStart => {}
            AgentEvent::Token(t) => {
                buf.push_str(&t);
                if progress_emitted < MAX_PROGRESS_LINES_PER_TURN {
                    let new_text = &buf[scan_from..];
                    let signals = prompt::scan_all_signals(new_text);
                    if let Some((last_off, _)) = signals.last() {
                        // Advance the scan pointer only past complete lines so
                        // we don't re-emit on partial chunks.
                        let consumed = scan_from + last_off + new_line_length(&buf[scan_from + last_off..]);
                        scan_from = consumed.min(buf.len());
                    }
                    for (_, sig) in signals {
                        match sig {
                            ProtocolSignal::Progress(s) => {
                                let _ = persist::append_progress(
                                    &ctx.id,
                                    &format!("PROGRESS: {s}"),
                                );
                                {
                                    let mut m = ctx.metrics.write().await;
                                    m.progress_lines = m.progress_lines.saturating_add(1);
                                }
                                let _ = ctx.events_tx.send((
                                    ctx.id.clone(),
                                    GoalEvent::Progress { line: s },
                                ));
                                progress_emitted += 1;
                            }
                            ProtocolSignal::Blocked(reason) => {
                                cancel.read().cancel();
                                return DrainOutcome::Blocked(reason);
                            }
                            ProtocolSignal::ClaimDone(summary) => {
                                // Don't cancel — let the agent finish its
                                // current message naturally; verifiers will
                                // run after run() returns (phase 3).
                                return DrainOutcome::ClaimDone(summary);
                            }
                        }
                    }
                }
            }
            AgentEvent::ToolStart { name, input, .. } => {
                latest_tool = Some(name.clone());
                if let Some(p) = extract_path_from_tool_input(&name, &input) {
                    if !files_touched.iter().any(|x| x == &p) {
                        files_touched.push(p);
                    }
                }
            }
            AgentEvent::ToolEnd { name, is_error, .. } => {
                *tools_since_checkpoint = tools_since_checkpoint.saturating_add(1);
                let _ = persist::append_progress(
                    &ctx.id,
                    &format!(
                        "TOOL: {name} {}",
                        if is_error { "(error)" } else { "(ok)" }
                    ),
                );
                maybe_checkpoint(
                    ctx,
                    tools_since_checkpoint,
                    last_checkpoint_at,
                    Some(&name),
                    &files_touched,
                )
                .await;
            }
            AgentEvent::Error(e) => {
                errored = Some(e);
                break;
            }
            AgentEvent::Done => break,
            _ => {}
        }
    }

    if let Some(e) = errored {
        DrainOutcome::Errored(e)
    } else {
        DrainOutcome::Finished
    }
}

fn new_line_length(s: &str) -> usize {
    s.find('\n').map(|i| i + 1).unwrap_or_else(|| s.len())
}

fn extract_path_from_tool_input(name: &str, input: &serde_json::Value) -> Option<PathBuf> {
    let candidates = ["path", "file_path", "filepath", "filename"];
    for key in candidates {
        if let Some(p) = input.get(key).and_then(|v| v.as_str()) {
            return Some(PathBuf::from(p));
        }
    }
    let _ = name;
    None
}

async fn maybe_checkpoint(
    ctx: &WorkerCtx,
    tools_since_checkpoint: &mut u32,
    last_checkpoint_at: &mut Instant,
    latest_tool: Option<&str>,
    files_touched: &[PathBuf],
) {
    let due_by_count = *tools_since_checkpoint >= CHECKPOINT_EVERY_TOOLS;
    let due_by_time = last_checkpoint_at.elapsed() >= Duration::from_secs(CHECKPOINT_EVERY_SECS);
    if !(due_by_count || due_by_time) {
        return;
    }
    let phase = "running".to_string();
    let last_action = latest_tool
        .map(|t| format!("tool: {t}"))
        .unwrap_or_else(|| "thinking".into());
    write_checkpoint_now(ctx, &phase, &last_action, "auto checkpoint", files_touched).await;
    *tools_since_checkpoint = 0;
    *last_checkpoint_at = Instant::now();
}

async fn write_checkpoint_now(
    ctx: &WorkerCtx,
    phase: &str,
    last_action: &str,
    blurb: &str,
    files_touched: &[PathBuf],
) {
    let m = ctx.metrics.read().await.clone();
    let c = Checkpoint {
        at: Utc::now(),
        turn: m.turns,
        phase: phase.into(),
        last_action: last_action.into(),
        files_touched: files_touched.to_vec(),
        progress_blurb: blurb.into(),
        metrics: m,
    };
    let _ = persist::save_checkpoint(&ctx.id, &c);
    let _ = ctx
        .events_tx
        .send((ctx.id.clone(), GoalEvent::Checkpoint(c)));
}

async fn write_initial_checkpoint(ctx: &WorkerCtx) {
    let m = ctx.metrics.read().await.clone();
    let c = Checkpoint {
        at: Utc::now(),
        turn: 0,
        phase: "starting".into(),
        last_action: "spawned".into(),
        files_touched: Vec::new(),
        progress_blurb: "Worker started. Awaiting first model response.".into(),
        metrics: m,
    };
    let _ = persist::save_checkpoint(&ctx.id, &c);
    let _ = persist::append_progress(&ctx.id, "PROGRESS: worker started");
    let _ = ctx
        .events_tx
        .send((ctx.id.clone(), GoalEvent::Checkpoint(c)));
}

async fn refresh_metrics(ctx: &WorkerCtx, session: &Arc<Mutex<Session>>, started_at: Instant) {
    let (in_tok, out_tok, cost) = {
        let s = session.lock().await;
        (
            s.cost_tracker.total_input_tokens,
            s.cost_tracker.total_output_tokens,
            s.cost_tracker.total_cost_usd,
        )
    };
    let mut m = ctx.metrics.write().await;
    m.input_tokens = in_tok;
    m.output_tokens = out_tok;
    m.cost_usd = cost;
    m.wall_secs = started_at.elapsed().as_secs();
    m.turns = m.turns.saturating_add(1);
}

// ---------------------------------------------------------------------------
// Verification phase
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum VerifyOutcome {
    AllPass,
    NoVerifiers,
    SomeFailed(String),
}

async fn run_verification_phase(ctx: &WorkerCtx) -> VerifyOutcome {
    // Flip state to Verifying so /goal-check shows the right phase.
    {
        let mut s = ctx.state.write().await;
        *s = GoalState::Verifying;
    }
    let _ = ctx
        .events_tx
        .send((ctx.id.clone(), GoalEvent::StateChanged(GoalState::Verifying)));
    persist_state(ctx).await;

    if ctx.spec.verifiers.is_empty() {
        let _ = persist::append_progress(
            &ctx.id,
            "VERIFY: no verifiers configured — trusting CLAIM_DONE",
        );
        return VerifyOutcome::NoVerifiers;
    }

    let report = verifier::run_all(&ctx.spec).await;
    if let Err(e) = verifier::persist_report(&ctx.id, &report) {
        let _ = ctx.events_tx.send((
            ctx.id.clone(),
            GoalEvent::Progress {
                line: format!("verifier report write failed: {e}"),
            },
        ));
    }
    // Emit one event per verifier and append to progress.log.
    for r in &report.results {
        let _ = ctx.events_tx.send((
            ctx.id.clone(),
            GoalEvent::VerifierResult {
                name: r.name.clone(),
                pass: r.passed,
                summary: r.summary.clone(),
            },
        ));
        let mark = if r.passed { "✓" } else { "✗" };
        let _ = persist::append_progress(
            &ctx.id,
            &format!("VERIFY {} {} — {}", mark, r.name, r.summary),
        );
    }
    {
        let mut m = ctx.metrics.write().await;
        m.verifiers_passed = m
            .verifiers_passed
            .saturating_add(report.pass_count() as u32);
        m.verifiers_failed = m
            .verifiers_failed
            .saturating_add(report.fail_count() as u32);
    }
    let _ = persist::save_metrics(&ctx.id, &*ctx.metrics.read().await);
    persist_state(ctx).await;

    if report.all_pass() {
        VerifyOutcome::AllPass
    } else {
        let failures: Vec<(String, String)> = report
            .failures()
            .into_iter()
            .map(|r| {
                let detail = format!(
                    "{}\nexit: {}\nstdout:\n{}\nstderr:\n{}",
                    r.summary,
                    r.exit_code
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "-".into()),
                    if r.stdout_excerpt.is_empty() {
                        "(empty)".into()
                    } else {
                        r.stdout_excerpt.clone()
                    },
                    if r.stderr_excerpt.is_empty() {
                        "(empty)".into()
                    } else {
                        r.stderr_excerpt.clone()
                    },
                );
                (r.name.clone(), detail)
            })
            .collect();
        VerifyOutcome::SomeFailed(prompt::verifier_failure_message(&failures))
    }
}

async fn persist_state(ctx: &WorkerCtx) {
    let state = ctx.state.read().await.clone();
    let m = ctx.metrics.read().await.clone();
    let _ = persist::upsert_index(
        &ctx.id,
        state,
        &ctx.spec.objective,
        ctx.spec.created_at,
        &m,
    );
}
