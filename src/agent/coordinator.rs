//! Coordinator — the orchestration layer for multithread mode.
//!
//! When `/multithread` is toggled ON, the Coordinator intercepts user prompts
//! and decides whether to:
//!   1. Handle them directly (simple questions)
//!   2. Split them into parallel Worker tasks (research, implementation, etc.)
//!
//! When `/multithread` is OFF (default), this module is completely idle and the
//! standard `AgentLoop::run()` handles everything as before.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::Config;
use crate::graph::SharedGraph;
use crate::provider::router::ProviderRouter;
use crate::session::Session;
use crate::tools::ToolRegistry;

use super::worker::{Worker, WorkerId, WorkerNotification, WorkerStatus};
use super::AgentEvent;

// ---------------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------------

/// Manages multiple parallel workers for the multithread execution mode.
pub struct Coordinator {
    provider_router: Arc<RwLock<ProviderRouter>>,
    tools: Arc<ToolRegistry>,
    config: Arc<Config>,
    graph: SharedGraph,
    session: Arc<Mutex<Session>>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    /// Channel for receiving worker completion notifications
    notify_tx: mpsc::UnboundedSender<WorkerNotification>,
    notify_rx: Arc<Mutex<mpsc::UnboundedReceiver<WorkerNotification>>>,
    /// Active workers indexed by ID
    active_workers: HashMap<WorkerId, WorkerHandle>,
}

/// Metadata about an active worker (the actual execution is in a tokio task).
#[derive(Debug)]
struct WorkerHandle {
    pub description: String,
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
        }
    }

    /// Spawn a new worker with the given task prompt. Returns the worker's ID.
    pub fn spawn_worker(&mut self, description: String, prompt: String) -> WorkerId {
        let working_dir = {
            // We need to get working_dir without async — use try_lock
            // This is safe because we only call spawn_worker from the TUI thread
            // which is the only thread that locks session during command handling.
            if let Ok(sess) = self.session.try_lock() {
                sess.working_dir.clone()
            } else {
                ".".to_string()
            }
        };

        let worker = Worker::new(
            description.clone(),
            self.provider_router.clone(),
            self.tools.clone(),
            self.config.clone(),
            self.graph.clone(),
            working_dir,
        );

        let worker_id = worker.id.clone();
        let notify_tx = self.notify_tx.clone();
        let event_tx = self.event_tx.clone();

        // Emit spawn event
        let _ = self.event_tx.send(AgentEvent::WorkerSpawned {
            worker_id: worker_id.clone(),
            description: description.clone(),
        });

        // Launch the worker in a background task
        let task = tokio::spawn(async move {
            worker.run(prompt, notify_tx, event_tx).await;
        });

        self.active_workers.insert(
            worker_id.clone(),
            WorkerHandle { description, task },
        );

        worker_id
    }

    /// Stop a running worker by ID.
    pub fn stop_worker(&mut self, worker_id: &str) -> bool {
        if let Some(handle) = self.active_workers.remove(worker_id) {
            handle.task.abort();
            let _ = self.event_tx.send(AgentEvent::WorkerCompleted {
                worker_id: worker_id.to_string(),
                description: handle.description,
                result: "(stopped by user)".to_string(),
                duration_ms: 0,
            });
            true
        } else {
            false
        }
    }

    /// Stop all running workers.
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
                // Remove from active workers
                self.active_workers.remove(&notif.worker_id);

                match notif.status {
                    WorkerStatus::Completed { result, token_usage, duration_ms } => {
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

                        // Also emit usage info
                        let _ = self.event_tx.send(AgentEvent::Token(format!(
                            "\n[Worker tokens: {} in / {} out]\n",
                            token_usage.input_tokens, token_usage.output_tokens,
                        )));
                    }
                    WorkerStatus::Failed { error, duration_ms } => {
                        events.push(AgentEvent::WorkerFailed {
                            worker_id: notif.worker_id,
                            description: notif.description,
                            error,
                            duration_ms,
                        });
                    }
                    WorkerStatus::Stopped => {
                        events.push(AgentEvent::WorkerCompleted {
                            worker_id: notif.worker_id,
                            description: notif.description,
                            result: "(stopped)".to_string(),
                            duration_ms: 0,
                        });
                    }
                    WorkerStatus::Running => {} // shouldn't arrive here
                }
            }
        }
        events
    }

    /// Get the number of currently active workers.
    pub fn active_count(&self) -> usize {
        self.active_workers.len()
    }

    /// List active workers for display.
    pub fn list_workers(&self) -> Vec<(String, String)> {
        self.active_workers
            .iter()
            .map(|(id, h)| (id.clone(), h.description.clone()))
            .collect()
    }
}
