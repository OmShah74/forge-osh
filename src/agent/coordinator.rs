//! Coordinator - orchestration layer for multithread mode and Agent Teams.
//!
//! `/multithread` keeps the original lightweight worker behavior. `/team`
//! builds on the same worker runtime but adds a durable board, shared bus
//! instructions, lifecycle state, artifact tracking, peer review, and conflict
//! detection.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::Config;
use crate::graph::SharedGraph;
use crate::provider::router::ProviderRouter;
use crate::session::Session;
use crate::tools::ToolRegistry;

use super::team::{TeamBoard, TeamPhase, TeamTaskKind};
use super::worker::{Worker, WorkerId, WorkerNotification, WorkerStatus};
use super::AgentEvent;

/// Manages background workers and optional durable Agent Team boards.
pub struct Coordinator {
    provider_router: Arc<RwLock<ProviderRouter>>,
    tools: Arc<ToolRegistry>,
    config: Arc<Config>,
    graph: SharedGraph,
    session: Arc<Mutex<Session>>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    notify_tx: mpsc::UnboundedSender<WorkerNotification>,
    notify_rx: Arc<Mutex<mpsc::UnboundedReceiver<WorkerNotification>>>,
    active_workers: HashMap<WorkerId, WorkerHandle>,
    team_board: Option<TeamBoard>,
}

#[derive(Debug)]
struct WorkerHandle {
    pub description: String,
    pub team_task_id: Option<String>,
    pub task: tokio::task::JoinHandle<()>,
}

impl Coordinator {
    pub fn new(
        provider_router: Arc<RwLock<ProviderRouter>>,
        tools: Arc<ToolRegistry>,
        config: Arc<Config>,
        graph: SharedGraph,
        session: Arc<Mutex<Session>>,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Self {
        let (notify_tx, notify_rx) = mpsc::unbounded_channel();
        let mut team_board = session.try_lock().ok().and_then(|sess| {
            TeamBoard::load_latest_for_dir(std::path::Path::new(&sess.working_dir))
        });
        if let Some(board) = &mut team_board {
            if matches!(
                board.phase,
                TeamPhase::Planning | TeamPhase::Running | TeamPhase::Reviewing
            ) {
                board.mark_stopped();
                board.record_event("loaded from disk without live workers; marked stopped");
                let _ = board.save();
            }
        }

        Self {
            provider_router,
            tools,
            config,
            graph,
            session,
            event_tx,
            notify_tx,
            notify_rx: Arc::new(Mutex::new(notify_rx)),
            active_workers: HashMap::new(),
            team_board,
        }
    }

    /// Spawn a classic ad-hoc worker. This preserves the existing
    /// `/multithread` + `@worker` behavior.
    pub fn spawn_worker(&mut self, description: String, prompt: String) -> WorkerId {
        self.spawn_worker_internal(description, prompt, None, None)
    }

    /// Start a durable team board and spawn the initial worker wave.
    pub fn start_team(&mut self, goal: String) -> String {
        if self.team_board.as_ref().is_some_and(|board| {
            matches!(
                board.phase,
                TeamPhase::Planning | TeamPhase::Running | TeamPhase::Reviewing
            )
        }) {
            return "A team is already active. Use `/team status` or `/team stop` first."
                .to_string();
        }

        let working_dir = self
            .session
            .try_lock()
            .map(|sess| std::path::PathBuf::from(&sess.working_dir))
            .unwrap_or_else(|_| std::path::PathBuf::from("."));

        let mut board = TeamBoard::from_goal(goal, working_dir);
        board.phase = TeamPhase::Running;
        self.team_board = Some(board);
        self.spawn_queued_team_tasks();
        self.save_team_board();
        self.team_status()
            .unwrap_or_else(|| "Agent team started.".to_string())
    }

    pub fn team_status(&self) -> Option<String> {
        self.team_board.as_ref().map(TeamBoard::format_markdown)
    }

    pub fn stop_team(&mut self) -> bool {
        let had_team = self.team_board.is_some();
        let ids: Vec<WorkerId> = self
            .active_workers
            .iter()
            .filter_map(|(id, handle)| handle.team_task_id.as_ref().map(|_| id.clone()))
            .collect();
        for id in ids {
            self.stop_worker(&id);
        }
        if let Some(board) = &mut self.team_board {
            board.mark_stopped();
        }
        self.save_team_board();
        had_team
    }

    /// Stop a running worker by ID.
    pub fn stop_worker(&mut self, worker_id: &str) -> bool {
        if let Some(handle) = self.active_workers.remove(worker_id) {
            handle.task.abort();
            if let (Some(board), Some(task_id)) = (&mut self.team_board, handle.team_task_id) {
                board.mark_failed(&task_id, "stopped by user".to_string(), 0);
            }
            let _ = self.event_tx.send(AgentEvent::WorkerCompleted {
                worker_id: worker_id.to_string(),
                description: handle.description,
                result: "(stopped by user)".to_string(),
                duration_ms: 0,
            });
            self.save_team_board();
            true
        } else {
            false
        }
    }

    /// Stop all running workers, including ad-hoc and team workers.
    pub fn stop_all(&mut self) {
        let ids: Vec<WorkerId> = self.active_workers.keys().cloned().collect();
        for id in ids {
            self.stop_worker(&id);
        }
    }

    /// Drain completed worker notifications (non-blocking). Returns events
    /// that should be displayed in the TUI.
    pub fn drain_notifications(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if let Ok(mut rx) = self.notify_rx.try_lock() {
            while let Ok(notif) = rx.try_recv() {
                self.active_workers.remove(&notif.worker_id);

                match notif.status {
                    WorkerStatus::Completed {
                        result,
                        token_usage,
                        duration_ms,
                    } => {
                        if let (Some(board), Some(task_id)) =
                            (&mut self.team_board, notif.task_id.as_deref())
                        {
                            let artifacts = if notif.artifacts.is_empty() {
                                board.mark_completed(task_id, result.clone(), duration_ms)
                            } else {
                                let _ = board.mark_completed(task_id, result.clone(), duration_ms);
                                if let Some(task) =
                                    board.tasks.iter_mut().find(|task| task.id == task_id)
                                {
                                    task.artifacts = notif.artifacts.clone();
                                }
                                notif.artifacts.clone()
                            };
                            if !artifacts.is_empty() {
                                board.record_event(format!(
                                    "{task_id} reported {} artifact(s)",
                                    artifacts.len()
                                ));
                            }
                        }

                        events.push(AgentEvent::WorkerCompleted {
                            worker_id: notif.worker_id,
                            description: notif.description,
                            result: if result.is_empty() {
                                "(no text output)".to_string()
                            } else {
                                result
                            },
                            duration_ms,
                        });

                        let _ = self.event_tx.send(AgentEvent::Token(format!(
                            "\n[Worker tokens: {} in / {} out]\n",
                            token_usage.input_tokens, token_usage.output_tokens,
                        )));
                    }
                    WorkerStatus::Failed { error, duration_ms } => {
                        if let (Some(board), Some(task_id)) =
                            (&mut self.team_board, notif.task_id.as_deref())
                        {
                            board.mark_failed(task_id, error.clone(), duration_ms);
                        }
                        events.push(AgentEvent::WorkerFailed {
                            worker_id: notif.worker_id,
                            description: notif.description,
                            error,
                            duration_ms,
                        });
                    }
                    WorkerStatus::Stopped => {
                        if let (Some(board), Some(task_id)) =
                            (&mut self.team_board, notif.task_id.as_deref())
                        {
                            board.mark_failed(task_id, "stopped".to_string(), 0);
                        }
                        events.push(AgentEvent::WorkerCompleted {
                            worker_id: notif.worker_id,
                            description: notif.description,
                            result: "(stopped)".to_string(),
                            duration_ms: 0,
                        });
                    }
                    WorkerStatus::Running => {}
                }
            }
        }
        self.advance_team_board();
        self.save_team_board();
        events
    }

    pub fn active_count(&self) -> usize {
        self.active_workers.len()
    }

    pub fn list_workers(&self) -> Vec<(String, String)> {
        self.active_workers
            .iter()
            .map(|(id, h)| (id.clone(), h.description.clone()))
            .collect()
    }

    fn spawn_worker_internal(
        &mut self,
        description: String,
        prompt: String,
        team_task_id: Option<String>,
        team_context: Option<String>,
    ) -> WorkerId {
        let working_dir = self
            .session
            .try_lock()
            .map(|sess| sess.working_dir.clone())
            .unwrap_or_else(|_| ".".to_string());

        let worker = match (team_task_id.clone(), team_context) {
            (Some(task_id), Some(context)) => Worker::new_with_team(
                description.clone(),
                self.provider_router.clone(),
                self.tools.clone(),
                self.config.clone(),
                self.graph.clone(),
                working_dir,
                task_id,
                context,
            ),
            _ => Worker::new(
                description.clone(),
                self.provider_router.clone(),
                self.tools.clone(),
                self.config.clone(),
                self.graph.clone(),
                working_dir,
            ),
        };

        let worker_id = worker.id.clone();
        let notify_tx = self.notify_tx.clone();
        let event_tx = self.event_tx.clone();

        let _ = self.event_tx.send(AgentEvent::WorkerSpawned {
            worker_id: worker_id.clone(),
            description: description.clone(),
        });

        let task = tokio::spawn(async move {
            worker.run(prompt, notify_tx, event_tx).await;
        });

        self.active_workers.insert(
            worker_id.clone(),
            WorkerHandle {
                description,
                team_task_id,
                task,
            },
        );

        worker_id
    }

    fn spawn_queued_team_tasks(&mut self) {
        let queued = self
            .team_board
            .as_ref()
            .map(TeamBoard::queued_worker_ids)
            .unwrap_or_default();
        for task_id in queued {
            self.spawn_team_task(&task_id);
        }
    }

    fn spawn_queued_review_tasks(&mut self) {
        let queued = self
            .team_board
            .as_ref()
            .map(TeamBoard::queued_review_ids)
            .unwrap_or_default();
        for task_id in queued {
            self.spawn_team_task(&task_id);
        }
    }

    fn spawn_team_task(&mut self, task_id: &str) {
        let Some((description, prompt, context, kind)) =
            self.team_board.as_ref().and_then(|board| {
                let task = board.get_task(task_id)?;
                let context = board.worker_prompt(task_id)?;
                Some((
                    format!("team:{}:{}", task.id, task.title),
                    task.prompt.clone(),
                    context,
                    task.kind,
                ))
            })
        else {
            return;
        };

        let worker_id = self.spawn_worker_internal(
            description,
            prompt,
            Some(task_id.to_string()),
            Some(context),
        );
        if let Some(board) = &mut self.team_board {
            board.mark_spawned(task_id, worker_id);
            if kind == TeamTaskKind::Review {
                board.phase = TeamPhase::Reviewing;
            }
        }
    }

    fn advance_team_board(&mut self) {
        let mut spawn_review = false;
        if let Some(board) = &mut self.team_board {
            board.refresh_phase_after_drain();
            if matches!(
                board.phase,
                TeamPhase::Conflict | TeamPhase::Failed | TeamPhase::Stopped
            ) {
                return;
            }
            if board.all_workers_finished()
                && board.bus_config.require_review
                && !board.has_review_task()
            {
                spawn_review = board.add_review_task().is_some();
            }
        }

        if spawn_review {
            self.spawn_queued_review_tasks();
        } else {
            self.spawn_queued_team_tasks();
        }

        if let Some(board) = &mut self.team_board {
            board.refresh_phase_after_drain();
        }
    }

    fn save_team_board(&self) {
        if let Some(board) = &self.team_board {
            let _ = board.save();
        }
    }
}
