//! Blocking team runner used by the model-callable `spawn_team` tool.
//!
//! Unlike [`crate::agent::coordinator::Coordinator`] (which is driven by the
//! TUI event loop), `TeamSpawner` runs a whole team **to completion inside a
//! single async call** and returns an aggregated report. This is what lets an
//! agent — most importantly a `/goal` worker — dynamically decide to fan a task
//! out across sub-agents and get the merged result back as a tool result.
//!
//! Modes (chosen by the calling model):
//! - **Swarm**: all sub-agents run concurrently and coordinate peer-to-peer via
//!   the shared blackboard (`team_post`/`team_read`); no central review.
//! - **Orchestrator**: sub-agents run concurrently, then a dedicated review
//!   agent integrates/reconciles their outputs.
//! - **Sequential**: sub-agents run one after another (each can read the
//!   blackboard left by the previous), for tightly-ordered work.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};

use crate::config::Config;
use crate::graph::SharedGraph;
use crate::provider::router::ProviderRouter;
use crate::tools::ToolRegistry;

use super::team::{TeamBoard, TeamMode, TeamTaskKind, TeamTaskStatus};
use super::team_bus::{self, SharedBlackboard};
use super::worker::{Worker, WorkerNotification, WorkerStatus};
use super::AgentEvent;

/// Execution strategy requested by the caller of `spawn_team`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnStrategy {
    Swarm,
    Orchestrator,
    Sequential,
}

impl SpawnStrategy {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "swarm" | "parallel" | "peer" => Some(SpawnStrategy::Swarm),
            "orchestrator" | "orch" | "central" | "review" => Some(SpawnStrategy::Orchestrator),
            "sequential" | "serial" | "single" | "ordered" => Some(SpawnStrategy::Sequential),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SpawnStrategy::Swarm => "swarm",
            SpawnStrategy::Orchestrator => "orchestrator",
            SpawnStrategy::Sequential => "sequential",
        }
    }

    /// The durable board mode that mirrors this strategy.
    fn team_mode(&self) -> TeamMode {
        match self {
            SpawnStrategy::Swarm | SpawnStrategy::Sequential => TeamMode::Swarm,
            SpawnStrategy::Orchestrator => TeamMode::Orchestrator,
        }
    }
}

/// Runtime dependencies needed to spawn real LLM sub-agents. Cloned (Arc) per
/// run; injected post-boot (see `run_tui`) because the tool registry is built
/// before the provider router exists.
#[derive(Clone)]
pub struct TeamSpawner {
    pub provider_router: Arc<RwLock<ProviderRouter>>,
    pub tools: Arc<ToolRegistry>,
    pub config: Arc<Config>,
    pub graph: SharedGraph,
}

const MAX_SUBTASKS: usize = 8;

impl TeamSpawner {
    pub fn new(
        provider_router: Arc<RwLock<ProviderRouter>>,
        tools: Arc<ToolRegistry>,
        config: Arc<Config>,
        graph: SharedGraph,
    ) -> Self {
        Self {
            provider_router,
            tools,
            config,
            graph,
        }
    }

    /// Run a team to completion and return an aggregated, model-readable report.
    pub async fn run_team(
        &self,
        goal: String,
        strategy: SpawnStrategy,
        workdir: String,
    ) -> String {
        let mut board =
            TeamBoard::from_goal_with_mode(goal, PathBuf::from(&workdir), strategy.team_mode());
        let blackboard = team_bus::new_blackboard();

        let (notify_tx, mut notify_rx) = mpsc::unbounded_channel::<WorkerNotification>();
        // Sub-agent UI events are not surfaced anywhere here; drain & drop them
        // so the unbounded channel cannot grow without bound.
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        tokio::spawn(async move { while event_rx.recv().await.is_some() {} });

        let worker_ids: Vec<String> = board
            .tasks
            .iter()
            .filter(|t| t.kind == TeamTaskKind::Worker)
            .take(MAX_SUBTASKS)
            .map(|t| t.id.clone())
            .collect();
        let total = worker_ids.len();
        if total == 0 {
            return "spawn_team: the goal produced no subtasks — nothing was spawned.".to_string();
        }

        if strategy == SpawnStrategy::Sequential {
            // One at a time; each later agent can read what earlier ones posted.
            for id in &worker_ids {
                let (prompt, context) = self.prepare(&mut board, id);
                let mut w = self
                    .build_worker(id, context, &workdir)
                    .with_blackboard(blackboard.clone());
                let ntx = notify_tx.clone();
                let etx = event_tx.clone();
                w.run(prompt, ntx, etx).await;
                if let Some(n) = notify_rx.recv().await {
                    apply_notification(&mut board, n);
                }
            }
        } else {
            // Swarm / Orchestrator: launch the whole roster concurrently.
            let mut handles = Vec::with_capacity(total);
            for id in &worker_ids {
                let (prompt, context) = self.prepare(&mut board, id);
                let mut w = self
                    .build_worker(id, context, &workdir)
                    .with_blackboard(blackboard.clone());
                let ntx = notify_tx.clone();
                let etx = event_tx.clone();
                handles.push(tokio::spawn(async move {
                    w.run(prompt, ntx, etx).await;
                }));
            }
            // Await the tasks first so a panicking worker can't make us block
            // forever on a notification that never arrives; notifications are
            // buffered in the unbounded channel and drained afterwards.
            for h in handles {
                let _ = h.await;
            }
            while let Ok(n) = notify_rx.try_recv() {
                apply_notification(&mut board, n);
            }
        }

        // Orchestrator: integrate via a dedicated review agent.
        if strategy == SpawnStrategy::Orchestrator {
            if let Some(review_id) = board.add_review_task() {
                let (prompt, context) = self.prepare(&mut board, &review_id);
                let mut w = self
                    .build_worker(&review_id, context, &workdir)
                    .with_blackboard(blackboard.clone());
                let ntx = notify_tx.clone();
                let etx = event_tx.clone();
                let h = tokio::spawn(async move {
                    w.run(prompt, ntx, etx).await;
                });
                let _ = h.await;
                while let Ok(n) = notify_rx.try_recv() {
                    apply_notification(&mut board, n);
                }
            }
        }

        board.refresh_phase_after_drain();
        aggregate_report(&board, strategy)
    }

    /// Resolve a task's prompt + bus context and mark it spawned on the board.
    fn prepare(&self, board: &mut TeamBoard, task_id: &str) -> (String, String) {
        let prompt = board
            .get_task(task_id)
            .map(|t| t.prompt.clone())
            .unwrap_or_default();
        let context = board.worker_prompt(task_id).unwrap_or_default();
        board.mark_spawned(task_id, format!("w-{task_id}"));
        (prompt, context)
    }

    fn build_worker(&self, task_id: &str, context: String, workdir: &str) -> Worker {
        Worker::new_with_team(
            format!("spawn_team:{task_id}"),
            self.provider_router.clone(),
            self.tools.clone(),
            self.config.clone(),
            self.graph.clone(),
            workdir.to_string(),
            task_id.to_string(),
            context,
        )
    }
}

fn apply_notification(board: &mut TeamBoard, n: WorkerNotification) {
    let task_id = match n.task_id.as_deref() {
        Some(id) => id.to_string(),
        None => return,
    };
    match n.status {
        WorkerStatus::Completed {
            result,
            duration_ms,
            ..
        } => {
            let _ = board.mark_completed(&task_id, result, duration_ms);
            if !n.artifacts.is_empty() {
                if let Some(task) = board.tasks.iter_mut().find(|t| t.id == task_id) {
                    task.artifacts = n.artifacts;
                }
            }
        }
        WorkerStatus::Failed { error, duration_ms } => {
            board.mark_failed(&task_id, error, duration_ms);
        }
        WorkerStatus::Stopped => {
            board.mark_failed(&task_id, "stopped".to_string(), 0);
        }
        WorkerStatus::Running => {}
    }
}

fn aggregate_report(board: &TeamBoard, strategy: SpawnStrategy) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Sub-team finished — strategy: {}, final phase: {}.\n\n",
        strategy.label(),
        board.phase
    ));
    for task in &board.tasks {
        out.push_str(&format!(
            "## {} [{}] {}\n",
            task.id, task.status, task.title
        ));
        if let Some(result) = &task.result {
            out.push_str(&truncate(result, 1500));
            out.push('\n');
        }
        if let Some(error) = &task.error {
            out.push_str(&format!("ERROR: {error}\n"));
        }
        if matches!(task.status, TeamTaskStatus::Completed | TeamTaskStatus::Merged)
            && !task.artifacts.is_empty()
        {
            out.push_str("Artifacts:\n");
            for a in &task.artifacts {
                out.push_str(&format!("- {} — {}\n", a.path, a.summary));
            }
        }
        out.push('\n');
    }
    let conflicts = board.conflicts();
    if !conflicts.is_empty() {
        out.push_str("Conflicts detected (resolve before relying on these results):\n");
        for c in conflicts {
            out.push_str(&format!("- {c}\n"));
        }
    }
    out
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!("{head}\n…(truncated)")
}
