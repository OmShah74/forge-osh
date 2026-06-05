//! Live team message bus (blackboard).
//!
//! A `TeamBlackboard` is an in-memory, shared scratchpad that every worker on
//! the same team can read and write **during execution** — peer-to-peer
//! coordination that does NOT route through the central orchestrator. Workers
//! reach it through `ToolContext::team_blackboard` via the `team_post` /
//! `team_read` tools.
//!
//! This complements the durable `TeamBoard` (which records final lifecycle
//! state and is read between waves): the board is "what finished", the
//! blackboard is "what's happening right now".

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// One posted message on the shared blackboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardEntry {
    /// Worker / task id that posted it.
    pub from: String,
    /// Optional category for filtering (`""` when none).
    pub topic: String,
    pub message: String,
    pub at: DateTime<Utc>,
}

impl BlackboardEntry {
    /// One-line rendering for injecting into a `team_read` result.
    pub fn render(&self) -> String {
        let topic = if self.topic.is_empty() {
            String::new()
        } else {
            format!("[{}] ", self.topic)
        };
        format!(
            "{} · {}: {}{}",
            self.at.format("%H:%M:%S"),
            self.from,
            topic,
            self.message
        )
    }
}

/// The shared scratchpad. Capped so a runaway worker can't grow it unbounded.
#[derive(Debug, Default)]
pub struct TeamBlackboard {
    entries: Vec<BlackboardEntry>,
}

const MAX_ENTRIES: usize = 500;

impl TeamBlackboard {
    pub fn post(
        &mut self,
        from: impl Into<String>,
        topic: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.entries.push(BlackboardEntry {
            from: from.into(),
            topic: topic.into(),
            message: message.into(),
            at: Utc::now(),
        });
        if self.entries.len() > MAX_ENTRIES {
            let overflow = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(0..overflow);
        }
    }

    /// Most-recent-last entries, optionally filtered by topic, capped to `limit`.
    /// When `exclude_from` is set, that author's own posts are omitted so a
    /// worker reading the board sees only its teammates.
    pub fn read(
        &self,
        topic: Option<&str>,
        limit: usize,
        exclude_from: Option<&str>,
    ) -> Vec<BlackboardEntry> {
        let mut v: Vec<BlackboardEntry> = self
            .entries
            .iter()
            .filter(|e| topic.map_or(true, |t| e.topic.eq_ignore_ascii_case(t)))
            .filter(|e| exclude_from.map_or(true, |f| e.from != f))
            .cloned()
            .collect();
        let start = v.len().saturating_sub(limit.max(1));
        v.split_off(start)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Cheap-to-clone shared handle, threaded through `ToolContext`.
pub type SharedBlackboard = Arc<Mutex<TeamBlackboard>>;

/// Construct a fresh, empty shared blackboard for one team run.
pub fn new_blackboard() -> SharedBlackboard {
    Arc::new(Mutex::new(TeamBlackboard::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_and_read_filters_topic_and_author() {
        let bb = new_blackboard();
        bb.lock().post("task-1", "files", "I own src/a.rs");
        bb.lock().post("task-2", "files", "I own src/b.rs");
        bb.lock().post("task-1", "", "general note");

        // task-2 reads the "files" topic, excluding its own posts.
        let got = bb.lock().read(Some("files"), 10, Some("task-2"));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].from, "task-1");
        assert!(got[0].message.contains("src/a.rs"));

        // No filter, exclude task-1 → only task-2's post remains.
        let got = bb.lock().read(None, 10, Some("task-1"));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].from, "task-2");
    }
}
