//! Cold-start resumer.
//!
//! Reads `~/.forge-osh/goals/index.json` at TUI boot and respawns every
//! goal whose state is non-terminal (`Running`, `Paused`, `Verifying`,
//! `Blocked`). Each goal's `spec.toml` is loaded from disk and the worker
//! is recreated with the original spec but with state seeded from the
//! last-persisted entry, so the user sees their goals exactly where they
//! left them.
//!
//! Terminal states (`Completed`, `Cleared`, `Failed`) are left alone —
//! they should already be removed from `index.json` by `clear()`, but the
//! filter is defensive.

use super::persistence as persist;
use super::supervisor::GoalSupervisor;
use super::GoalState;

/// Resume every goal in the index that was not in a terminal state when
/// the process died. Returns the list of GoalIds that were successfully
/// respawned plus a list of (id, error) pairs for ones that could not be
/// resumed (e.g. missing spec.toml).
pub async fn resume_all(supervisor: &GoalSupervisor) -> ResumeReport {
    let mut resumed = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    let index = persist::IndexFile::load_or_default();
    for summary in &index.goals {
        if summary.state.is_terminal() {
            continue;
        }
        // Don't touch goals that are already live (defensive: in case the
        // supervisor was already initialised by something else).
        if supervisor.get(&summary.id).await.is_some() {
            continue;
        }
        match persist::load_spec(&summary.id) {
            Err(e) => {
                failed.push((summary.id.to_string(), format!("load spec: {e}")));
                continue;
            }
            Ok(spec) => {
                // Seed state: respawn() will pick the right starting state.
                // Paused goals stay paused (user has to /goal resume), all
                // other non-terminal states become Running so the worker
                // continues. Blocked goals also continue — if the original
                // block reason was budget-related, the worker will hit it
                // again immediately and re-block, which is the correct
                // behaviour.
                let seed_state = match &summary.state {
                    GoalState::Paused => GoalState::Paused,
                    _ => GoalState::Running,
                };
                match supervisor.respawn(spec, seed_state).await {
                    Ok(id) => resumed.push(id.to_string()),
                    Err(e) => failed.push((summary.id.to_string(), format!("respawn: {e}"))),
                }
            }
        }
    }

    ResumeReport { resumed, failed }
}

#[derive(Debug)]
pub struct ResumeReport {
    pub resumed: Vec<String>,
    pub failed: Vec<(String, String)>,
}

impl ResumeReport {
    pub fn is_empty(&self) -> bool {
        self.resumed.is_empty() && self.failed.is_empty()
    }
}
