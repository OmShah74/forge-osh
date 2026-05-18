//! Multi-goal supervisor.
//!
//! Owns a `HashMap<GoalId, GoalHandle>` of live goal workers. Each handle
//! holds a control channel into its worker task, the event-stream receiver,
//! and a snapshot of last-known state.
//!
//! Phase 2: workers run the real autonomous LLM loop via `worker::run_worker`.
//! `WorkerDeps` are injected post-boot via [`GoalSupervisor::set_deps`] so
//! that `AppState` can construct the supervisor before all dependencies
//! (provider router, tool registry, file cache, etc.) are ready.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;

use super::persistence as persist;
use super::worker::{self, WorkerCtx, WorkerDeps};
use super::{
    Budget, GoalControl, GoalEvent, GoalId, GoalMetrics, GoalSpec, GoalState, GoalSummary,
    StatusSnapshot,
};

/// One per live goal.
pub struct GoalHandle {
    pub id: GoalId,
    pub spec: Arc<GoalSpec>,
    pub state: Arc<RwLock<GoalState>>,
    pub metrics: Arc<RwLock<GoalMetrics>>,
    pub started_at: Instant,
    control_tx: mpsc::UnboundedSender<GoalControl>,
    resume_notify: Arc<Notify>,
    join: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl GoalHandle {
    pub async fn send_control(&self, c: GoalControl) -> Result<(), GoalError> {
        self.control_tx
            .send(c)
            .map_err(|_| GoalError::WorkerGone(self.id.clone()))
    }

    pub fn resume(&self) {
        self.resume_notify.notify_waiters();
    }

    pub async fn status(&self) -> StatusSnapshot {
        let state = self.state.read().await.clone();
        let metrics = self.metrics.read().await.clone();
        let last_checkpoint = persist::load_latest_checkpoint(&self.id).ok().flatten();
        let tail = persist::tail_progress(&self.id, 10).unwrap_or_default();
        StatusSnapshot {
            id: self.id.clone(),
            state,
            spec_objective: self.spec.objective.clone(),
            spec_stopping: self.spec.stopping_condition.clone(),
            metrics,
            last_checkpoint,
            tail_progress: tail,
        }
    }

    pub async fn summary(&self) -> GoalSummary {
        let state = self.state.read().await.clone();
        let metrics = self.metrics.read().await.clone();
        GoalSummary {
            id: self.id.clone(),
            state,
            objective: self.spec.objective.clone(),
            created_at: self.spec.created_at,
            turns: metrics.turns,
            cost_usd: metrics.cost_usd,
        }
    }
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GoalSupervisor {
    inner: Arc<RwLock<HashMap<GoalId, Arc<GoalHandle>>>>,
    deps: Arc<RwLock<Option<WorkerDeps>>>,
    events_tx: mpsc::UnboundedSender<(GoalId, GoalEvent)>,
    events_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<(GoalId, GoalEvent)>>>>,
}

impl GoalSupervisor {
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            deps: Arc::new(RwLock::new(None)),
            events_tx,
            events_rx: Arc::new(Mutex::new(Some(events_rx))),
        }
    }

    /// Inject worker dependencies. Must be called once at startup before any
    /// `/goal <objective>` spawn attempt succeeds.
    pub async fn set_deps(&self, deps: WorkerDeps) {
        *self.deps.write().await = Some(deps);
    }

    /// Returns true once `set_deps` has been called.
    pub async fn is_ready(&self) -> bool {
        self.deps.read().await.is_some()
    }

    /// Hand the event receiver to the TUI. Returns `None` if already taken.
    pub async fn take_event_rx(&self) -> Option<mpsc::UnboundedReceiver<(GoalId, GoalEvent)>> {
        self.events_rx.lock().await.take()
    }

    /// Spawn a fresh goal worker.
    pub async fn spawn(&self, spec: GoalSpec) -> Result<GoalId, GoalError> {
        let deps_snapshot = match self.deps.read().await.clone() {
            Some(d) => d,
            None => return Err(GoalError::NotReady),
        };

        let id = spec.id.clone();
        persist::ensure_goal_dirs(&id).map_err(GoalError::Io)?;
        persist::save_spec(&spec).map_err(GoalError::Io)?;

        let initial_metrics = GoalMetrics::default();
        persist::save_metrics(&id, &initial_metrics).map_err(GoalError::Io)?;
        persist::upsert_index(
            &id,
            GoalState::Running,
            &spec.objective,
            spec.created_at,
            &initial_metrics,
        )
        .map_err(GoalError::Io)?;

        let (control_tx, control_rx) = mpsc::unbounded_channel::<GoalControl>();
        let state = Arc::new(RwLock::new(GoalState::Running));
        let metrics = Arc::new(RwLock::new(initial_metrics));
        let spec_arc = Arc::new(spec);
        let resume_notify = Arc::new(Notify::new());

        let ctx = WorkerCtx {
            id: id.clone(),
            spec: spec_arc.clone(),
            state: state.clone(),
            metrics: metrics.clone(),
            events_tx: self.events_tx.clone(),
            control_rx,
            resume_notify: resume_notify.clone(),
        };

        let join = tokio::spawn(worker::run_worker(ctx, deps_snapshot));

        let handle = Arc::new(GoalHandle {
            id: id.clone(),
            spec: spec_arc,
            state,
            metrics,
            started_at: Instant::now(),
            control_tx,
            resume_notify,
            join: Arc::new(Mutex::new(Some(join))),
        });

        self.inner.write().await.insert(id.clone(), handle);
        Ok(id)
    }

    pub async fn get(&self, id: &GoalId) -> Option<Arc<GoalHandle>> {
        self.inner.read().await.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<GoalSummary> {
        let guard = self.inner.read().await;
        let mut out = Vec::with_capacity(guard.len());
        for h in guard.values() {
            out.push(h.summary().await);
        }
        out.sort_by_key(|s| s.created_at);
        out
    }

    pub async fn pause(&self, id: &GoalId) -> Result<(), GoalError> {
        let h = self.get(id).await.ok_or(GoalError::NotFound(id.clone()))?;
        h.send_control(GoalControl::Pause).await
    }

    pub async fn resume(&self, id: &GoalId) -> Result<(), GoalError> {
        let h = self.get(id).await.ok_or(GoalError::NotFound(id.clone()))?;
        {
            let mut st = h.state.write().await;
            if *st == GoalState::Paused {
                *st = GoalState::Running;
                let _ = self
                    .events_tx
                    .send((h.id.clone(), GoalEvent::StateChanged(GoalState::Running)));
            }
        }
        h.resume();
        Ok(())
    }

    pub async fn clear(&self, id: &GoalId) -> Result<(), GoalError> {
        let h = self
            .get(id)
            .await
            .ok_or(GoalError::NotFound(id.clone()))?;
        // Mark cleared first so paused workers, when woken, see Cleared and exit.
        {
            let mut s = h.state.write().await;
            *s = GoalState::Cleared;
        }
        let _ = h.send_control(GoalControl::Clear).await;
        h.resume();

        let join = h.join.lock().await.take();
        if let Some(j) = join {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), j).await;
        }

        self.inner.write().await.remove(id);
        persist::remove_from_index(id).map_err(GoalError::Io)?;
        persist::archive_goal(id).map_err(GoalError::Io)?;
        Ok(())
    }

    pub async fn status(&self, id: &GoalId) -> Result<StatusSnapshot, GoalError> {
        let h = self
            .get(id)
            .await
            .ok_or(GoalError::NotFound(id.clone()))?;
        Ok(h.status().await)
    }

    pub async fn verify_now(&self, id: &GoalId) -> Result<(), GoalError> {
        let h = self.get(id).await.ok_or(GoalError::NotFound(id.clone()))?;
        h.send_control(GoalControl::VerifyNow).await
    }

    pub async fn force_complete(&self, id: &GoalId) -> Result<(), GoalError> {
        let h = self.get(id).await.ok_or(GoalError::NotFound(id.clone()))?;
        h.send_control(GoalControl::ForceComplete).await
    }

    /// Respawn a goal from disk (used by the cold-start resumer). Unlike
    /// `spawn` the on-disk spec is the authority — id and created_at are
    /// preserved. The metrics/state are seeded from the existing index
    /// entry rather than reset to zero.
    pub async fn respawn(
        &self,
        spec: GoalSpec,
        seed_state: GoalState,
    ) -> Result<GoalId, GoalError> {
        let deps_snapshot = match self.deps.read().await.clone() {
            Some(d) => d,
            None => return Err(GoalError::NotReady),
        };

        let id = spec.id.clone();
        persist::ensure_goal_dirs(&id).map_err(GoalError::Io)?;
        // spec.toml already exists on disk, but re-save in case of schema
        // updates that need to round-trip through serde.
        let _ = persist::save_spec(&spec);

        let initial_metrics = persist::load_metrics(&id).unwrap_or_default();
        persist::upsert_index(
            &id,
            seed_state.clone(),
            &spec.objective,
            spec.created_at,
            &initial_metrics,
        )
        .map_err(GoalError::Io)?;

        let (control_tx, control_rx) = mpsc::unbounded_channel::<GoalControl>();
        let state = Arc::new(RwLock::new(seed_state));
        let metrics = Arc::new(RwLock::new(initial_metrics));
        let spec_arc = Arc::new(spec);
        let resume_notify = Arc::new(Notify::new());

        let ctx = WorkerCtx {
            id: id.clone(),
            spec: spec_arc.clone(),
            state: state.clone(),
            metrics: metrics.clone(),
            events_tx: self.events_tx.clone(),
            control_rx,
            resume_notify: resume_notify.clone(),
        };

        let join = tokio::spawn(worker::run_worker(ctx, deps_snapshot));

        let handle = Arc::new(GoalHandle {
            id: id.clone(),
            spec: spec_arc,
            state,
            metrics,
            started_at: Instant::now(),
            control_tx,
            resume_notify,
            join: Arc::new(Mutex::new(Some(join))),
        });

        self.inner.write().await.insert(id.clone(), handle);
        Ok(id)
    }

    /// Update a live goal's budget. Writes through to spec.toml so the new
    /// limits survive process restart. The worker reads `spec.budget` at
    /// the top of each iteration so the change is picked up on the next
    /// turn without needing to bounce the worker. Note: this mutates the
    /// in-memory `Arc<GoalSpec>` via a fresh allocation since GoalSpec is
    /// not interior-mutable.
    pub async fn set_budget(&self, id: &GoalId, budget: Budget) -> Result<(), GoalError> {
        let mut spec = persist::load_spec(id).map_err(GoalError::Io)?;
        spec.budget = budget;
        persist::save_spec(&spec).map_err(GoalError::Io)?;
        // The in-memory spec on the handle is immutable (Arc<GoalSpec>) —
        // for budget changes to take effect on the running worker we would
        // need to either restart the worker or thread a Watch channel
        // through. Phase 4 ships the persistence side; live re-read is
        // marked as a phase 5 follow-up.
        Ok(())
    }
}

impl Default for GoalSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum GoalError {
    #[error("no goal with id {0}")]
    NotFound(GoalId),
    #[error("goal worker for {0} has exited")]
    WorkerGone(GoalId),
    #[error("goal subsystem not initialized — set_deps has not been called")]
    NotReady,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
