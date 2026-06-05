//! Persistent, real-time task plan.
//!
//! This is the central planner that powers the live, ticking task list shown
//! in the TUI (the equivalent of Claude Code / Codex's task manager). Unlike
//! the old "blurt the whole plan then wait for approval" flow, a `TaskPlan` is:
//!
//! - **Two-level**: a central list of tasks, each identified by an id and
//!   containing individual steps (each step has its own id, content, status).
//! - **Persistent**: serialized to `~/.forge-osh/tasks/<session_id>.json`
//!   (see [`crate::config::tasks_dir`]). Every mutation is written to disk
//!   immediately, so the plan survives app close/open, session switches, and
//!   mid-turn interruptions.
//! - **Live**: the agent loop re-emits the plan as an `AgentEvent::PlanUpdated`
//!   after every `update_plan` tool call so the TUI can tick steps off in real
//!   time, and injects the current plan into the system prompt so the model
//!   always knows what is done, in progress, and pending.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Status of an individual plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Blocked,
}

impl StepStatus {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "in_progress" | "in-progress" | "active" | "doing" | "working" => {
                StepStatus::InProgress
            }
            "completed" | "complete" | "done" | "finished" => StepStatus::Completed,
            "blocked" | "failed" | "error" => StepStatus::Blocked,
            _ => StepStatus::Pending,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Pending => "pending",
            StepStatus::InProgress => "in_progress",
            StepStatus::Completed => "completed",
            StepStatus::Blocked => "blocked",
        }
    }

    /// Checkbox glyph used by the TUI plan panel.
    pub fn checkbox(&self) -> &'static str {
        match self {
            StepStatus::Pending => "☐",
            StepStatus::InProgress => "▣",
            StepStatus::Completed => "☑",
            StepStatus::Blocked => "☒",
        }
    }

    pub fn is_completed(&self) -> bool {
        matches!(self, StepStatus::Completed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub status: StepStatus,
    /// Optional short note (e.g. why a step is blocked, or what changed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    pub id: String,
    pub subject: String,
    #[serde(default)]
    pub steps: Vec<PlanStep>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PlanTask {
    pub fn step_progress(&self) -> (usize, usize) {
        let done = self.steps.iter().filter(|s| s.status.is_completed()).count();
        (done, self.steps.len())
    }

    pub fn is_complete(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.status.is_completed())
    }
}

/// The full, persisted plan for one session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskPlan {
    pub session_id: String,
    /// Short heading shown above the list (e.g. "Plan", "Updated Plan").
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tasks: Vec<PlanTask>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl TaskPlan {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            title: "Plan".to_string(),
            tasks: Vec::new(),
            updated_at: None,
        }
    }

    /// On-disk path for a given session's plan.
    pub fn path(session_id: &str) -> PathBuf {
        crate::config::tasks_dir().join(format!("{session_id}.json"))
    }

    /// Load the plan for a session, or return an empty plan if none exists or
    /// the file is corrupt (a corrupt plan must never crash the agent/TUI).
    pub fn load(session_id: &str) -> Self {
        let path = Self::path(session_id);
        match std::fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str::<TaskPlan>(&data).unwrap_or_else(|_| {
                let mut p = TaskPlan::new(session_id);
                p.session_id = session_id.to_string();
                p
            }),
            Err(_) => {
                let mut p = TaskPlan::new(session_id);
                p.session_id = session_id.to_string();
                p
            }
        }
    }

    /// Persist the plan to disk. Best-effort — returns the IO error so callers
    /// can surface it, but a failure here never blocks tool execution.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path(&self.session_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, data)
    }

    /// Delete the on-disk plan for a session (used when a session is deleted).
    pub fn delete(session_id: &str) {
        let _ = std::fs::remove_file(Self::path(session_id));
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// (completed_steps, total_steps) across every task.
    pub fn progress(&self) -> (usize, usize) {
        let mut done = 0;
        let mut total = 0;
        for t in &self.tasks {
            let (d, n) = t.step_progress();
            done += d;
            total += n;
        }
        (done, total)
    }

    /// True when there is at least one step and not all of them are completed —
    /// i.e. the planner should be shown as "live" in the UI.
    pub fn is_active(&self) -> bool {
        let (done, total) = self.progress();
        total > 0 && done < total
    }

    /// True when every step in every task is completed.
    pub fn all_completed(&self) -> bool {
        let (done, total) = self.progress();
        total > 0 && done == total
    }

    /// Apply a declarative update from a tool call. The whole plan is replaced
    /// by `tasks`, but `created_at` timestamps are preserved for any task whose
    /// id matches an existing one (so the plan keeps a stable creation history
    /// as it is repeatedly rewritten while ticking steps off).
    pub fn apply_update(&mut self, title: Option<String>, tasks: Vec<PlanTask>) {
        if let Some(title) = title {
            if !title.trim().is_empty() {
                self.title = title;
            }
        }
        // Preserve created_at for tasks that already existed.
        let mut new_tasks = tasks;
        for nt in new_tasks.iter_mut() {
            if let Some(old) = self.tasks.iter().find(|t| t.id == nt.id) {
                nt.created_at = old.created_at;
            }
        }
        self.tasks = new_tasks;
        self.updated_at = Some(Utc::now());
    }

    /// A compact textual rendering of the plan for injection into the system
    /// prompt so the model always knows the current state.
    pub fn to_prompt_block(&self) -> String {
        let mut out = String::new();
        let (done, total) = self.progress();
        out.push_str(&format!(
            "Current task plan \"{}\" ({}/{} steps complete):\n",
            if self.title.is_empty() {
                "Plan"
            } else {
                &self.title
            },
            done,
            total
        ));
        for task in &self.tasks {
            let (td, tn) = task.step_progress();
            out.push_str(&format!("- [{}] {} ({}/{})\n", task.id, task.subject, td, tn));
            for step in &task.steps {
                out.push_str(&format!(
                    "    {} ({}) {}\n",
                    step.status.checkbox(),
                    step.status.as_str(),
                    step.content
                ));
            }
        }
        out
    }
}
