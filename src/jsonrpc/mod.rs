//! NDJSON JSON-RPC bridge over stdio.
//!
//! Enables the agent to be driven by an IDE (e.g. the planned VS Code
//! extension) instead of the TUI. Activated by
//! `--output-format=stream-json --stdin-json`. Logging must be redirected
//! to stderr in this mode — anything written to stdout that is not a valid
//! `OutboundEvent` line is a bug.

pub mod inbound;
pub mod outbound;

pub use inbound::{ContextBlock, InboundCommand};
pub use outbound::OutboundEvent;

/// Wire-format major version. Bump on any breaking change. The IDE reads
/// `forge-osh --jsonrpc-version` on startup and refuses to attach on
/// mismatch.
pub const JSONRPC_VERSION: u32 = 2;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::agent::permissions::PermissionStore;
use crate::agent::{AgentEvent, AgentLoop, PermissionRequest};
use crate::app::App;
use crate::session::FileStateCache;
use crate::types::{PermissionLevel, PermissionMode, PermissionResponse, ThinkingConfig};

use crate::agent::goal::supervisor::GoalSupervisor;
use crate::agent::goal::worker::WorkerDeps;
use crate::agent::goal::{GoalId, GoalSpec};
use crate::session::checkpoint::Checkpoint;

type PendingPerms = Arc<PlMutex<HashMap<String, oneshot::Sender<PermissionResponse>>>>;

/// Serialize one event and write it as a single NDJSON line to stdout.
async fn write_event(stdout: &Arc<Mutex<tokio::io::Stdout>>, ev: &OutboundEvent) {
    let mut line = match serde_json::to_string(ev) {
        Ok(s) => s,
        Err(e) => {
            // Should never happen for our enum — but if it does, emit
            // an Error event so the IDE at least sees something.
            tracing::error!(error = %e, "failed to serialize OutboundEvent");
            return;
        }
    };
    line.push('\n');
    let mut guard = stdout.lock().await;
    if let Err(e) = guard.write_all(line.as_bytes()).await {
        tracing::error!(error = %e, "failed to write OutboundEvent");
        return;
    }
    let _ = guard.flush().await;
}

fn level_str(level: &PermissionLevel) -> &'static str {
    match level {
        PermissionLevel::ReadOnly => "read_only",
        PermissionLevel::Mutating => "mutating",
        PermissionLevel::Destructive => "destructive",
        PermissionLevel::Shell => "shell",
        PermissionLevel::Network => "network",
    }
}

fn parse_permission_response(s: &str) -> Option<PermissionResponse> {
    match s {
        "allow" => Some(PermissionResponse::Allow),
        "deny" => Some(PermissionResponse::Deny),
        "always_allow" => Some(PermissionResponse::AlwaysAllow),
        "trust" => Some(PermissionResponse::TrustMode),
        _ => None,
    }
}

/// Translate one AgentEvent into zero-or-more OutboundEvents.
///
/// Note: `Done` is suppressed here — the caller emits `Done { reason }`
/// after the AgentLoop turn future resolves so the reason can be
/// classified (end_turn / cancelled / error).
fn translate(ev: AgentEvent) -> Vec<OutboundEvent> {
    match ev {
        AgentEvent::Token(t) => vec![OutboundEvent::AssistantTextDelta { text: t }],
        AgentEvent::ThinkingStart => vec![OutboundEvent::ThinkingStart],
        AgentEvent::ThinkingDelta { text } => vec![OutboundEvent::ThinkingDelta { text }],
        AgentEvent::ThinkingEnd => vec![OutboundEvent::ThinkingEnd],
        AgentEvent::ToolStart { id, name, input } => {
            vec![OutboundEvent::ToolCallStart { id, name, input }]
        }
        AgentEvent::ToolEnd {
            id,
            output,
            is_error,
            ..
        } => vec![OutboundEvent::ToolCallEnd {
            id,
            output_excerpt: output,
            is_error,
        }],
        AgentEvent::ToolOutputDelta { id, stream, text } => {
            vec![OutboundEvent::ToolOutputDelta { id, stream, text }]
        }
        AgentEvent::DiffPreview {
            tool_call_id,
            path,
            unified_diff,
        } => vec![OutboundEvent::DiffPreview {
            tool_call_id,
            path,
            unified_diff,
        }],
        AgentEvent::TurnUsage {
            input,
            output,
            cache_read,
            cache_write,
            cost_usd,
        } => vec![OutboundEvent::Usage {
            input,
            output,
            cache_read,
            cache_write,
            cost_usd,
        }],
        AgentEvent::ContextWarning { used, limit } => vec![OutboundEvent::SystemMessage {
            text: format!("Context warning: {used}/{limit} tokens"),
            kind: "warn".into(),
        }],
        AgentEvent::CompactionStart {
            message_count,
            provider_name,
            model_id,
            automatic,
        } => vec![OutboundEvent::Compaction {
            stage: "start".into(),
            summary: Some(format!(
                "compacting {message_count} msg via {provider_name}/{model_id} (auto={automatic})"
            )),
        }],
        AgentEvent::HistoryCompacted {
            kept,
            removed,
            summary_preview,
            succeeded,
            ..
        } => vec![OutboundEvent::Compaction {
            stage: if succeeded {
                "complete".into()
            } else {
                "failed".into()
            },
            summary: Some(format!(
                "kept={kept} removed={removed} summary={summary_preview}"
            )),
        }],
        AgentEvent::Done => vec![], // suppressed; emitted by caller
        AgentEvent::Error(msg) => vec![OutboundEvent::Error { message: msg }],
        AgentEvent::WorkerSpawned { worker_id, description } => {
            vec![OutboundEvent::SystemMessage {
                text: format!("worker spawned: {worker_id} — {description}"),
                kind: "info".into(),
            }]
        }
        AgentEvent::WorkerCompleted {
            worker_id,
            duration_ms,
            ..
        } => vec![OutboundEvent::SystemMessage {
            text: format!("worker {worker_id} completed in {duration_ms}ms"),
            kind: "info".into(),
        }],
        AgentEvent::WorkerFailed {
            worker_id,
            error,
            ..
        } => vec![OutboundEvent::SystemMessage {
            text: format!("worker {worker_id} failed: {error}"),
            kind: "error".into(),
        }],
        AgentEvent::WorkerToolStart { .. } | AgentEvent::WorkerToolEnd { .. } => vec![],
        AgentEvent::SkillScopeChanged { name } => vec![OutboundEvent::SystemMessage {
            text: match name {
                Some(n) => format!("skill scope: {n}"),
                None => "skill scope cleared".into(),
            },
            kind: "info".into(),
        }],
        AgentEvent::PlanUpdated { plan } => vec![OutboundEvent::PlanUpdated {
            plan: serde_json::to_value(&plan).unwrap_or(serde_json::Value::Null),
        }],
    }
}

/// Entry point invoked from `main.rs` when
/// `--output-format=stream-json --stdin-json` is set.
pub async fn run(app: App) -> anyhow::Result<()> {
    let stdout = Arc::new(Mutex::new(tokio::io::stdout()));

    // Channels shared across the whole session — the AgentLoop is built once
    // and reused for every turn.
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let (perm_tx, mut perm_rx) = mpsc::unbounded_channel::<PermissionRequest>();
    let (_perm_resp_tx, perm_resp_rx) = mpsc::unbounded_channel::<PermissionResponse>();
    // Streaming tool-output channel.  Long-running shell/powershell tools push
    // line-sized `ToolOutputChunk`s here; the forwarder spawned below
    // re-emits them through `event_tx` as `AgentEvent::ToolOutputDelta`,
    // which the existing translation path converts into
    // `OutboundEvent::ToolOutputDelta` on stdout.
    let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<crate::types::ToolOutputChunk>();
    {
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(chunk) = chunk_rx.recv().await {
                let _ = event_tx.send(AgentEvent::ToolOutputDelta {
                    id: chunk.tool_call_id,
                    stream: chunk.stream,
                    text: chunk.text,
                });
            }
        });
    }

    // Build the AgentLoop once with cheap-to-clone Arcs.
    let agent = Arc::new(AgentLoop {
        provider_router: app.provider_router.clone(),
        tools: app.tools.clone(),
        session: app.session.clone(),
        config: app.config.clone(),
        event_tx,
        permission_tx: perm_tx,
        permission_rx: Arc::new(Mutex::new(perm_resp_rx)),
        graph: app.shared_graph.clone(),
        lsp: app.lsp.clone(),
        file_cache: Arc::new(FileStateCache::new()),
        permission_store: Arc::new(parking_lot::RwLock::new(PermissionStore::load())),
        cancel: Arc::new(parking_lot::RwLock::new(CancellationToken::new())),
        permission_mode: Arc::new(parking_lot::RwLock::new(
            if app.config.general.trust_mode {
                PermissionMode::Bypass
            } else {
                PermissionMode::Default
            },
        )),
        thinking: Arc::new(parking_lot::RwLock::new(ThinkingConfig::Disabled)),
        skill_registry: app.skills.clone(),
        output_chunk_tx: Some(chunk_tx),
    });

    let pending: PendingPerms = Arc::new(PlMutex::new(HashMap::new()));

    // Goal subsystem — owned by this jsonrpc session. The supervisor is
    // injected with the same WorkerDeps the TUI builds so /goal in either
    // surface gets identical behaviour.
    let goal_sup = Arc::new(GoalSupervisor::new());
    goal_sup
        .set_deps(WorkerDeps {
            provider_router: app.provider_router.clone(),
            tools: app.tools.clone(),
            config: app.config.clone(),
            graph: app.shared_graph.clone(),
            lsp: app.lsp.clone(),
            file_cache: agent.file_cache.clone(),
            permission_store: agent.permission_store.clone(),
            skill_registry: app.skills.clone(),
        })
        .await;
    let goal_task = {
        let stdout_g = stdout.clone();
        let sup = goal_sup.clone();
        let rx = sup.take_event_rx().await;
        tokio::spawn(async move {
            if let Some(mut rx) = rx {
                while let Some((id, ev)) = rx.recv().await {
                    let payload = match serde_json::to_value(&ev) {
                        Ok(v) => v,
                        Err(_) => serde_json::Value::Null,
                    };
                    write_event(
                        &stdout_g,
                        &OutboundEvent::GoalEvent {
                            goal_id: id.to_string(),
                            payload,
                        },
                    )
                    .await;
                }
            }
        })
    };

    // Send Ready.
    let (provider_name, model_name) = {
        let router = app.provider_router.read().await;
        (
            router.active_provider_id().to_string(),
            router.active_model_id().to_string(),
        )
    };
    write_event(
        &stdout,
        &OutboundEvent::Ready {
            jsonrpc_version: JSONRPC_VERSION,
            forge_version: env!("CARGO_PKG_VERSION").to_string(),
            provider: provider_name,
            model: model_name,
        },
    )
    .await;

    // Task A — agent events → stdout.
    let stdout_a = stdout.clone();
    let event_task = tokio::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            for out in translate(ev) {
                write_event(&stdout_a, &out).await;
            }
        }
    });

    // Task B — permission requests → stdout + pending map.
    let stdout_b = stdout.clone();
    let pending_b = pending.clone();
    let perm_task = tokio::spawn(async move {
        while let Some(req) = perm_rx.recv().await {
            let id = uuid::Uuid::new_v4().to_string();
            let ev = OutboundEvent::PermissionRequest {
                id: id.clone(),
                tool: req.tool_name.clone(),
                summary: req.input_summary.clone(),
                level: level_str(&req.level).to_string(),
                input: req.input.clone(),
                diff_preview: None,
            };
            pending_b.lock().insert(id, req.response_tx);
            write_event(&stdout_b, &ev).await;
        }
    });

    // Task C — stdin reader → dispatch.
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    // Serialize turns: only one user_message runs at a time. While a turn
    // is in flight, further user_messages are queued in this oneshot-free
    // way: we drop the spawn handle and `await` it before the next.
    let mut current_turn: Option<tokio::task::JoinHandle<crate::error::Result<()>>> = None;

    loop {
        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break, // EOF — IDE detached
            Err(e) => {
                write_event(
                    &stdout,
                    &OutboundEvent::SystemMessage {
                        text: format!("stdin read error: {e}"),
                        kind: "error".into(),
                    },
                )
                .await;
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cmd: InboundCommand = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                write_event(
                    &stdout,
                    &OutboundEvent::SystemMessage {
                        text: format!("bad command JSON: {e}"),
                        kind: "warn".into(),
                    },
                )
                .await;
                continue;
            }
        };

        match cmd {
            InboundCommand::UserMessage { text, context_blocks } => {
                // Wait for any in-flight turn before starting the next one.
                if let Some(h) = current_turn.take() {
                    let _ = h.await;
                }
                let mut full = text;
                for blk in context_blocks {
                    full.push_str(&blk.render());
                }
                // Install a fresh cancel token for this turn.
                agent.reset_cancel();
                let agent_c = agent.clone();
                let stdout_c = stdout.clone();
                current_turn = Some(tokio::spawn(async move {
                    let result = agent_c.run(full).await;
                    let reason = match &result {
                        Ok(_) => "end_turn",
                        Err(_) => "error",
                    };
                    // Per-call `Usage` events have already been emitted via
                    // `AgentEvent::TurnUsage`; the IDE sums them to track
                    // cumulative cost. We just close the turn here.
                    write_event(
                        &stdout_c,
                        &OutboundEvent::Done {
                            reason: reason.to_string(),
                        },
                    )
                    .await;
                    result
                }));
            }
            InboundCommand::PermissionResponse { id, response } => {
                let parsed = match parse_permission_response(&response) {
                    Some(p) => p,
                    None => {
                        write_event(
                            &stdout,
                            &OutboundEvent::SystemMessage {
                                text: format!("unknown permission response: {response}"),
                                kind: "warn".into(),
                            },
                        )
                        .await;
                        continue;
                    }
                };
                if let Some(tx) = pending.lock().remove(&id) {
                    let _ = tx.send(parsed);
                } else {
                    write_event(
                        &stdout,
                        &OutboundEvent::SystemMessage {
                            text: format!("no pending permission for id={id}"),
                            kind: "warn".into(),
                        },
                    )
                    .await;
                }
            }
            InboundCommand::Cancel => {
                agent.cancel_current_turn();
            }
            InboundCommand::Ping => {
                write_event(
                    &stdout,
                    &OutboundEvent::SystemMessage {
                        text: "pong".into(),
                        kind: "info".into(),
                    },
                )
                .await;
            }
            InboundCommand::SwitchModel { provider, model } => {
                let mut router = app.provider_router.write().await;
                match router.set_active(&provider, &model) {
                    Ok(_) => {
                        let mut s = app.session.lock().await;
                        s.provider_id = provider.clone();
                        s.model_id = model.clone();
                        write_event(
                            &stdout,
                            &OutboundEvent::SystemMessage {
                                text: format!("switched to {provider}/{model}"),
                                kind: "info".into(),
                            },
                        )
                        .await;
                    }
                    Err(e) => {
                        write_event(
                            &stdout,
                            &OutboundEvent::Error {
                                message: format!("switch_model failed: {e}"),
                            },
                        )
                        .await;
                    }
                }
            }
            InboundCommand::Compact { keep_last } => {
                handle_compact(&app, &stdout, keep_last.map(|n| n as usize)).await;
            }
            InboundCommand::LoadSession { name } => {
                handle_load_session(&app, &stdout, &name).await;
            }
            InboundCommand::NewSession { name } => {
                handle_new_session(&app, &stdout, name).await;
            }
            InboundCommand::SpawnGoal {
                objective,
                spec_path,
            } => {
                handle_spawn_goal(&goal_sup, &stdout, objective, spec_path).await;
            }
            InboundCommand::GoalControl { goal_id, action } => {
                handle_goal_control(&goal_sup, &stdout, goal_id, action).await;
            }
            InboundCommand::InvokeSkill { name, args } => {
                // Wait for any in-flight turn to drain so the skill scope
                // is applied atomically to the next agent.run().
                if let Some(h) = current_turn.take() {
                    let _ = h.await;
                }
                let prompt = match handle_invoke_skill(&app, &stdout, &name, args.as_deref()).await
                {
                    Some(p) => p,
                    None => continue,
                };
                agent.reset_cancel();
                let agent_c = agent.clone();
                let stdout_c = stdout.clone();
                current_turn = Some(tokio::spawn(async move {
                    let result = agent_c.run(prompt).await;
                    let reason = match &result {
                        Ok(_) => "end_turn",
                        Err(_) => "error",
                    };
                    write_event(
                        &stdout_c,
                        &OutboundEvent::Done {
                            reason: reason.to_string(),
                        },
                    )
                    .await;
                    result
                }));
            }
            InboundCommand::Configure { key, value } => {
                handle_configure(&agent, &stdout, &key, &value).await;
            }
            InboundCommand::Undo => {
                let msg = crate::agent::file_history::undo_last().await;
                write_event(
                    &stdout,
                    &OutboundEvent::SystemMessage {
                        text: msg,
                        kind: "info".into(),
                    },
                )
                .await;
            }
            InboundCommand::RenameSession { name } => {
                let result = {
                    let mut s = app.session.lock().await;
                    s.name = name.clone();
                    s.save()
                };
                match result {
                    Ok(_) => {
                        write_event(
                            &stdout,
                            &OutboundEvent::SystemMessage {
                                text: format!("session renamed to '{name}'"),
                                kind: "info".into(),
                            },
                        )
                        .await
                    }
                    Err(e) => {
                        write_event(
                            &stdout,
                            &OutboundEvent::Error {
                                message: format!("rename saved in-memory but disk save failed: {e}"),
                            },
                        )
                        .await
                    }
                }
            }
            InboundCommand::SaveSession => {
                let result = { app.session.lock().await.save() };
                match result {
                    Ok(_) => {
                        write_event(
                            &stdout,
                            &OutboundEvent::SystemMessage {
                                text: "session saved".into(),
                                kind: "info".into(),
                            },
                        )
                        .await
                    }
                    Err(e) => {
                        write_event(
                            &stdout,
                            &OutboundEvent::Error {
                                message: format!("save failed: {e}"),
                            },
                        )
                        .await
                    }
                }
            }
            InboundCommand::GoalStatus { goal_id } => {
                handle_goal_status(&goal_sup, &stdout, goal_id).await;
            }
            InboundCommand::SkillCommand { action, name } => {
                handle_skill_command(&app, &stdout, &action, name.as_deref()).await;
            }
            InboundCommand::PermissionRules {
                action,
                tool,
                pattern,
                index,
            } => {
                handle_permission_rules(&agent, &stdout, &action, tool, pattern, index).await;
            }
            InboundCommand::McpCommand { action, server } => {
                handle_mcp_command(&app, &stdout, &action, server.as_deref()).await;
            }
            InboundCommand::BuildGraph { rebuild } => {
                handle_build_graph(&app, &stdout, rebuild).await;
            }
            InboundCommand::HooksReload => {
                // Agent loop reloads on every turn already; this is a
                // user-visible confirmation that the file was re-read.
                let _ = crate::agent::hooks::HooksConfig::load();
                write_event(
                    &stdout,
                    &OutboundEvent::SystemMessage {
                        text: "hooks reloaded".into(),
                        kind: "info".into(),
                    },
                )
                .await;
            }
        }
    }

    // EOF: cancel any running turn, drain background tasks.
    agent.cancel_current_turn();
    if let Some(h) = current_turn.take() {
        let _ = h.await;
    }
    drop(agent); // drops the AgentLoop's senders → receivers terminate
    let _ = event_task.await;
    let _ = perm_task.await;
    drop(goal_sup);
    let _ = goal_task.await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Inbound-command handlers
// ---------------------------------------------------------------------------

async fn handle_compact(app: &App, stdout: &Arc<Mutex<tokio::io::Stdout>>, keep_last: Option<usize>) {
    use crate::agent::compaction;
    let keep = keep_last.unwrap_or(compaction::DEFAULT_KEEP_LAST);
    let (messages, invoked_skills) = {
        let s = app.session.lock().await;
        (s.history.messages().to_vec(), s.invoked_skills.clone())
    };
    let total = messages.len();
    if total == 0 || (keep > 0 && total <= keep) {
        write_event(
            stdout,
            &OutboundEvent::SystemMessage {
                text: format!("nothing to compact (have {total}, keeping {keep})"),
                kind: "info".into(),
            },
        )
        .await;
        return;
    }
    let (to_summarize, _) = compaction::split_for_compaction(&messages, keep);
    let to_summarize = to_summarize.to_vec();
    let count = to_summarize.len();
    let (ctx_window, model_id) = {
        let r = app.provider_router.read().await;
        (r.active_context_window(), r.active_model_id().to_string())
    };
    write_event(
        stdout,
        &OutboundEvent::Compaction {
            stage: "start".into(),
            summary: Some(format!("compacting {count} message(s)")),
        },
    )
    .await;
    let result = {
        let r = app.provider_router.read().await;
        match r.active() {
            Ok(provider) => {
                compaction::summarize_messages(
                    &to_summarize,
                    &invoked_skills,
                    provider,
                    &model_id,
                    ctx_window,
                )
                .await
            }
            Err(e) => Err(e),
        }
    };
    match result {
        Ok(summary) => {
            let removed = total.saturating_sub(keep);
            let save_result = {
                let mut s = app.session.lock().await;
                s.history.summarize_old(summary.clone(), keep);
                s.save()
            };
            write_event(
                stdout,
                &OutboundEvent::Compaction {
                    stage: "complete".into(),
                    summary: Some(format!("kept={keep} removed={removed}")),
                },
            )
            .await;
            if let Err(e) = save_result {
                write_event(
                    stdout,
                    &OutboundEvent::SystemMessage {
                        text: format!("compaction saved in-memory but disk save failed: {e}"),
                        kind: "warn".into(),
                    },
                )
                .await;
            }
            let _ = summary;
        }
        Err(e) => {
            write_event(
                stdout,
                &OutboundEvent::Compaction {
                    stage: "failed".into(),
                    summary: Some(format!("{e}")),
                },
            )
            .await;
        }
    }
}

async fn handle_load_session(app: &App, stdout: &Arc<Mutex<tokio::io::Stdout>>, name_or_id: &str) {
    let sessions = match Checkpoint::list() {
        Ok(v) => v,
        Err(e) => {
            write_event(
                stdout,
                &OutboundEvent::Error {
                    message: format!("list sessions failed: {e}"),
                },
            )
            .await;
            return;
        }
    };
    let target = sessions
        .iter()
        .find(|s| s.id == name_or_id)
        .or_else(|| sessions.iter().find(|s| s.id.starts_with(name_or_id)))
        .or_else(|| sessions.iter().find(|s| s.name == name_or_id))
        .map(|s| s.id.clone());
    let id = match target {
        Some(i) => i,
        None => {
            write_event(
                stdout,
                &OutboundEvent::Error {
                    message: format!("no session matches '{name_or_id}'"),
                },
            )
            .await;
            return;
        }
    };
    let mut loaded = match Checkpoint::load(&id) {
        Ok(s) => s,
        Err(e) => {
            write_event(
                stdout,
                &OutboundEvent::Error {
                    message: format!("load failed: {e}"),
                },
            )
            .await;
            return;
        }
    };
    // Keep the current working dir so tool calls don't suddenly retarget.
    let current_wd = { app.session.lock().await.working_dir.clone() };
    loaded.working_dir = current_wd;
    let route_ok = {
        let mut r = app.provider_router.write().await;
        r.set_active(&loaded.provider_id, &loaded.model_id).is_ok()
    };
    if !route_ok {
        let r = app.provider_router.read().await;
        loaded.provider_id = r.active_provider_id().to_string();
        loaded.model_id = r.active_model_id().to_string();
    }
    let msg_count = loaded.history.messages().len() as u32;
    let session_id = loaded.id.clone();
    {
        let mut s = app.session.lock().await;
        *s = loaded;
    }
    write_event(
        stdout,
        &OutboundEvent::SessionLoaded {
            id: session_id,
            message_count: msg_count,
        },
    )
    .await;
}

async fn handle_new_session(
    app: &App,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    name: Option<String>,
) {
    let (provider_id, model_id, working_dir) = {
        let r = app.provider_router.read().await;
        let s = app.session.lock().await;
        (
            r.active_provider_id().to_string(),
            r.active_model_id().to_string(),
            s.working_dir.clone(),
        )
    };
    let session_name = name.unwrap_or_else(|| chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
    let new_session =
        crate::session::Session::new(session_name, provider_id, model_id, working_dir);
    let id = new_session.id.clone();
    {
        let mut s = app.session.lock().await;
        *s = new_session;
    }
    write_event(
        stdout,
        &OutboundEvent::SessionLoaded {
            id,
            message_count: 0,
        },
    )
    .await;
}

async fn handle_spawn_goal(
    sup: &Arc<GoalSupervisor>,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    objective: String,
    spec_path: Option<String>,
) {
    let spec: GoalSpec = if let Some(path) = spec_path {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                write_event(
                    stdout,
                    &OutboundEvent::Error {
                        message: format!("spec read failed: {e}"),
                    },
                )
                .await;
                return;
            }
        };
        match toml::from_str::<GoalSpec>(&text) {
            Ok(mut s) => {
                s.id = GoalId::new();
                s.created_at = chrono::Utc::now();
                if s.workdir.as_os_str().is_empty() {
                    s.workdir = std::env::current_dir().unwrap_or_default();
                }
                s
            }
            Err(e) => {
                write_event(
                    stdout,
                    &OutboundEvent::Error {
                        message: format!("spec parse failed: {e}"),
                    },
                )
                .await;
                return;
            }
        }
    } else {
        let workdir = std::env::current_dir().unwrap_or_default();
        GoalSpec::from_objective(objective, workdir)
    };
    match sup.spawn(spec).await {
        Ok(id) => {
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("goal spawned: {id}"),
                    kind: "info".into(),
                },
            )
            .await;
        }
        Err(e) => {
            write_event(
                stdout,
                &OutboundEvent::Error {
                    message: format!("goal spawn failed: {e}"),
                },
            )
            .await;
        }
    }
}

async fn handle_goal_control(
    sup: &Arc<GoalSupervisor>,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    goal_id: String,
    action: String,
) {
    let id = GoalId(goal_id);
    let result = match action.as_str() {
        "pause" => sup.pause(&id).await,
        "resume" => sup.resume(&id).await,
        "clear" => sup.clear(&id).await,
        "verify_now" => sup.verify_now(&id).await,
        "force_complete" => sup.force_complete(&id).await,
        other => {
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("unknown goal action: {other}"),
                    kind: "warn".into(),
                },
            )
            .await;
            return;
        }
    };
    if let Err(e) = result {
        write_event(
            stdout,
            &OutboundEvent::Error {
                message: format!("goal control failed: {e}"),
            },
        )
        .await;
    }
}

/// Returns the materialized skill prompt that should be fed to `agent.run()`
/// next, or `None` if invocation failed (in which case an Error event was
/// already emitted).
async fn handle_invoke_skill(
    app: &App,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    name: &str,
    args: Option<&str>,
) -> Option<String> {
    let (working_dir, session_id) = {
        let s = app.session.lock().await;
        (s.working_dir.clone(), s.id.clone())
    };
    crate::skills::refresh_registry(&app.skills, std::path::Path::new(&working_dir));
    let registry = crate::skills::SkillLoader::load(std::path::Path::new(&working_dir));
    let applied = match crate::skills::apply_skill(&registry, name, args, &session_id) {
        Ok(a) => a,
        Err(e) => {
            write_event(
                stdout,
                &OutboundEvent::Error {
                    message: format!("skill '{name}' invocation failed: {e}"),
                },
            )
            .await;
            return None;
        }
    };
    let materialized = applied.materialized_prompt.clone();
    {
        let mut s = app.session.lock().await;
        if applied.mode == crate::skills::SkillExecutionMode::Inline {
            s.active_skill_scope = Some(crate::skills::ActiveSkillScope {
                skill_name: applied.skill_name.clone(),
                allowed_tools: applied.allowed_tools.clone(),
                model_override: applied.model_override.clone(),
                hooks: applied.hooks.clone(),
                execution_mode: applied.mode,
            });
        } else {
            s.active_skill_scope = None;
        }
        s.push_invoked_skill(crate::skills::SkillInvocationRecord {
            skill_name: applied.skill_name.clone(),
            source: applied.source,
            canonical_path: applied.canonical_path.clone(),
            materialized_prompt: applied.materialized_prompt.clone(),
            invoked_at: chrono::Utc::now(),
            worker_id: None,
        });
    }
    write_event(
        stdout,
        &OutboundEvent::SystemMessage {
            text: format!("invoking skill: {}", applied.skill_name),
            kind: "info".into(),
        },
    )
    .await;
    Some(materialized)
}

async fn handle_configure(
    agent: &Arc<AgentLoop>,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    key: &str,
    value: &serde_json::Value,
) {
    match key {
        "permission_mode" => {
            let mode = match value.as_str() {
                Some("default") => PermissionMode::Default,
                Some("plan") => PermissionMode::Plan,
                Some("accept_edits") => PermissionMode::AcceptEdits,
                Some("bypass") => PermissionMode::Bypass,
                _ => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: "permission_mode must be one of default/plan/accept_edits/bypass".into(),
                            kind: "warn".into(),
                        },
                    )
                    .await;
                    return;
                }
            };
            *agent.permission_mode.write() = mode;
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("permission_mode = {value}"),
                    kind: "info".into(),
                },
            )
            .await;
        }
        "thinking" => {
            let cfg = match value {
                serde_json::Value::Bool(false) => ThinkingConfig::Disabled,
                serde_json::Value::Bool(true) => ThinkingConfig::Enabled,
                serde_json::Value::Number(n) => {
                    let tokens = n.as_u64().unwrap_or(0) as u32;
                    if tokens == 0 {
                        ThinkingConfig::Disabled
                    } else {
                        ThinkingConfig::Budget { tokens }
                    }
                }
                _ => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: "thinking must be bool or token-budget number".into(),
                            kind: "warn".into(),
                        },
                    )
                    .await;
                    return;
                }
            };
            *agent.thinking.write() = cfg;
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("thinking = {value}"),
                    kind: "info".into(),
                },
            )
            .await;
        }
        "effort_level" => {
            let n = value.as_u64().unwrap_or(3).clamp(1, 5) as u8;
            let mut s = agent.session.lock().await;
            s.effort_level = n;
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("effort_level = {n}"),
                    kind: "info".into(),
                },
            )
            .await;
        }
        other => {
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("unknown configure key: {other}"),
                    kind: "warn".into(),
                },
            )
            .await;
        }
    }
}

async fn handle_goal_status(
    sup: &Arc<GoalSupervisor>,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    goal_id: String,
) {
    let id = GoalId(goal_id);
    match sup.status(&id).await {
        Ok(snap) => {
            let payload = serde_json::to_value(&snap).unwrap_or(serde_json::Value::Null);
            write_event(
                stdout,
                &OutboundEvent::GoalEvent {
                    goal_id: id.to_string(),
                    payload,
                },
            )
            .await;
        }
        Err(e) => {
            write_event(
                stdout,
                &OutboundEvent::Error {
                    message: format!("goal status failed: {e}"),
                },
            )
            .await;
        }
    }
}

async fn handle_skill_command(
    app: &App,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    action: &str,
    name: Option<&str>,
) {
    let working_dir = { app.session.lock().await.working_dir.clone() };
    let path = std::path::Path::new(&working_dir);
    match action {
        "list" => {
            crate::skills::refresh_registry(&app.skills, path);
            let registry = crate::skills::SkillLoader::load(path);
            let listing: Vec<serde_json::Value> = registry
                .skills
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "description": s.description,
                        "source": s.source.label(),
                        "execution_mode": s.execution_mode.as_str(),
                        "allowed_tools": s.allowed_tools,
                    })
                })
                .collect();
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: serde_json::to_string(&listing).unwrap_or_default(),
                    kind: "info".into(),
                },
            )
            .await;
        }
        "show" => {
            let n = match name {
                Some(n) => n,
                None => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: "skill show requires 'name'".into(),
                            kind: "warn".into(),
                        },
                    )
                    .await;
                    return;
                }
            };
            let registry = crate::skills::SkillLoader::load(path);
            match registry.find(n) {
                Some(s) => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: s.content.clone(),
                            kind: "info".into(),
                        },
                    )
                    .await
                }
                None => {
                    write_event(
                        stdout,
                        &OutboundEvent::Error {
                            message: format!("no skill named '{n}'"),
                        },
                    )
                    .await
                }
            }
        }
        "reload" => {
            crate::skills::refresh_registry(&app.skills, path);
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: "skill registry reloaded".into(),
                    kind: "info".into(),
                },
            )
            .await;
        }
        "delete" => {
            let n = match name {
                Some(n) => n,
                None => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: "skill delete requires 'name'".into(),
                            kind: "warn".into(),
                        },
                    )
                    .await;
                    return;
                }
            };
            let registry = crate::skills::SkillLoader::load(path);
            match registry.find(n) {
                Some(s) => {
                    let Some(canonical) = s.canonical_path.clone() else {
                        write_event(
                            stdout,
                            &OutboundEvent::Error {
                                message: format!(
                                    "skill '{n}' is bundled and cannot be deleted from disk"
                                ),
                            },
                        )
                        .await;
                        return;
                    };
                    match std::fs::remove_file(&canonical) {
                        Ok(_) => {
                            crate::skills::refresh_registry(&app.skills, path);
                            write_event(
                                stdout,
                                &OutboundEvent::SystemMessage {
                                    text: format!("deleted skill '{n}'"),
                                    kind: "info".into(),
                                },
                            )
                            .await;
                        }
                        Err(e) => {
                            write_event(
                                stdout,
                                &OutboundEvent::Error {
                                    message: format!("delete failed: {e}"),
                                },
                            )
                            .await
                        }
                    }
                }
                None => {
                    write_event(
                        stdout,
                        &OutboundEvent::Error {
                            message: format!("no skill named '{n}'"),
                        },
                    )
                    .await
                }
            }
        }
        other => {
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("unknown skill action: {other}"),
                    kind: "warn".into(),
                },
            )
            .await
        }
    }
}

async fn handle_permission_rules(
    agent: &Arc<AgentLoop>,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    action: &str,
    tool: Option<String>,
    pattern: Option<String>,
    index: Option<usize>,
) {
    match action {
        "list" => {
            let rules = {
                let store = agent.permission_store.read();
                store
                    .rules
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        serde_json::json!({
                            "index": i,
                            "tool": r.tool,
                            "pattern": r.pattern,
                            "allow": r.allow,
                        })
                    })
                    .collect::<Vec<_>>()
            };
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: serde_json::to_string(&rules).unwrap_or_default(),
                    kind: "info".into(),
                },
            )
            .await;
        }
        "add_allow" | "add_deny" => {
            let Some(t) = tool else {
                write_event(
                    stdout,
                    &OutboundEvent::SystemMessage {
                        text: "add_allow/add_deny require 'tool'".into(),
                        kind: "warn".into(),
                    },
                )
                .await;
                return;
            };
            let p = pattern.unwrap_or_default();
            {
                let mut store = agent.permission_store.write();
                if action == "add_allow" {
                    store.add_allow(&t, &p);
                } else {
                    store.add_deny(&t, &p);
                }
            }
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("{action} rule added for {t} '{p}'"),
                    kind: "info".into(),
                },
            )
            .await;
        }
        "remove" => {
            let Some(idx) = index else {
                write_event(
                    stdout,
                    &OutboundEvent::SystemMessage {
                        text: "remove requires 'index'".into(),
                        kind: "warn".into(),
                    },
                )
                .await;
                return;
            };
            {
                let mut store = agent.permission_store.write();
                store.remove(idx);
            }
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("removed rule at index {idx}"),
                    kind: "info".into(),
                },
            )
            .await;
        }
        other => {
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("unknown permission_rules action: {other}"),
                    kind: "warn".into(),
                },
            )
            .await
        }
    }
}

async fn handle_mcp_command(
    app: &App,
    stdout: &Arc<Mutex<tokio::io::Stdout>>,
    action: &str,
    server: Option<&str>,
) {
    match action {
        "list" => {
            let snap = app.mcp.snapshot().await;
            // ServerSnapshot doesn't impl Serialize; hand-render a stable
            // JSON shape so the IDE always sees the same field names.
            let listing: Vec<serde_json::Value> = snap
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "display_name": s.display_name,
                        "description": s.description,
                        "category": s.category,
                        "enabled": s.enabled,
                        "status": s.status.label(),
                        "tool_count": s.tool_count,
                        "server_version": s.server_version,
                        "last_error": s.last_error,
                    })
                })
                .collect();
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: serde_json::to_string(&listing).unwrap_or_default(),
                    kind: "info".into(),
                },
            )
            .await;
        }
        "connect" => {
            let Some(id) = server else {
                return mcp_missing_server(stdout, "connect").await;
            };
            match app.mcp.connect(id).await {
                Ok(n) => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: format!("connected to {id} ({n} tool(s) registered)"),
                            kind: "info".into(),
                        },
                    )
                    .await
                }
                Err(e) => {
                    write_event(
                        stdout,
                        &OutboundEvent::Error {
                            message: format!("mcp connect '{id}' failed: {e}"),
                        },
                    )
                    .await
                }
            }
        }
        "disconnect" => {
            let Some(id) = server else {
                return mcp_missing_server(stdout, "disconnect").await;
            };
            match app.mcp.disconnect(id).await {
                Ok(_) => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: format!("disconnected {id}"),
                            kind: "info".into(),
                        },
                    )
                    .await
                }
                Err(e) => {
                    write_event(
                        stdout,
                        &OutboundEvent::Error {
                            message: format!("mcp disconnect '{id}' failed: {e}"),
                        },
                    )
                    .await
                }
            }
        }
        "enable" | "disable" => {
            let Some(id) = server else {
                return mcp_missing_server(stdout, action).await;
            };
            let want = action == "enable";
            match app.mcp.set_enabled(id, want).await {
                Ok(_) => {
                    write_event(
                        stdout,
                        &OutboundEvent::SystemMessage {
                            text: format!("{id} enabled={want}"),
                            kind: "info".into(),
                        },
                    )
                    .await
                }
                Err(e) => {
                    write_event(
                        stdout,
                        &OutboundEvent::Error {
                            message: format!("mcp {action} '{id}' failed: {e}"),
                        },
                    )
                    .await
                }
            }
        }
        other => {
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: format!("unknown mcp action: {other}"),
                    kind: "warn".into(),
                },
            )
            .await
        }
    }
}

async fn mcp_missing_server(stdout: &Arc<Mutex<tokio::io::Stdout>>, action: &str) {
    write_event(
        stdout,
        &OutboundEvent::SystemMessage {
            text: format!("mcp {action} requires 'server'"),
            kind: "warn".into(),
        },
    )
    .await;
}

async fn handle_build_graph(app: &App, stdout: &Arc<Mutex<tokio::io::Stdout>>, rebuild: bool) {
    use crate::graph::builder::GraphBuilder;
    use crate::graph::CodeGraph;
    let working_dir = { app.session.lock().await.working_dir.clone() };
    let root = std::path::PathBuf::from(&working_dir);
    let exe_dir = CodeGraph::artifact_dir();
    let artifact = CodeGraph::artifact_path(&root, &exe_dir);
    if !rebuild {
        let has = app.shared_graph.read().ok().is_some_and(|g| g.is_some());
        if has {
            write_event(
                stdout,
                &OutboundEvent::SystemMessage {
                    text: "forge-graph already loaded; pass rebuild=true to force".into(),
                    kind: "info".into(),
                },
            )
            .await;
            return;
        }
    }
    write_event(
        stdout,
        &OutboundEvent::SystemMessage {
            text: format!("building forge-graph for {} …", root.display()),
            kind: "info".into(),
        },
    )
    .await;
    let shared = app.shared_graph.clone();
    let stdout_b = stdout.clone();
    tokio::task::spawn_blocking(move || {
        let (tx, _rx) = std::sync::mpsc::channel();
        let result = GraphBuilder::build(&root, &tx);
        (result, artifact, shared, stdout_b)
    })
    .await
    .ok()
    .map(|(result, artifact, shared, stdout_b)| {
        tokio::spawn(async move {
            match result {
                Ok(graph) => {
                    let save_err = graph.save(&artifact).err().map(|e| e.to_string());
                    let nodes = graph.meta.total_nodes;
                    let edges = graph.meta.total_edges;
                    if let Ok(mut g) = shared.write() {
                        *g = Some(graph);
                    }
                    let text = match save_err {
                        None => format!(
                            "forge-graph built. nodes={nodes} edges={edges} artifact={}",
                            artifact.display()
                        ),
                        Some(e) => format!(
                            "forge-graph built in-memory (nodes={nodes} edges={edges}) but save failed: {e}"
                        ),
                    };
                    write_event(
                        &stdout_b,
                        &OutboundEvent::SystemMessage {
                            text,
                            kind: "info".into(),
                        },
                    )
                    .await;
                }
                Err(e) => {
                    write_event(
                        &stdout_b,
                        &OutboundEvent::Error {
                            message: format!("forge-graph build failed: {e}"),
                        },
                    )
                    .await;
                }
            }
        });
    });
}
