//! Durable task board for coordinator-managed agent teams.
//!
//! The board is intentionally provider-agnostic: it tracks lifecycle, worker
//! assignment, artifacts, review state, and conflict hints while the existing
//! `Worker` type performs the actual LLM/tool execution.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamBusConfig {
    pub max_parallel_workers: usize,
    pub require_review: bool,
    pub conflict_strategy: String,
}

impl Default for TeamBusConfig {
    fn default() -> Self {
        Self {
            max_parallel_workers: 3,
            require_review: true,
            conflict_strategy: "Prefer disjoint file ownership. If two workers report the same artifact path, mark the board as conflict and require coordinator review.".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamPhase {
    Planning,
    Running,
    Reviewing,
    Completed,
    Failed,
    Conflict,
    Stopped,
}

impl fmt::Display for TeamPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TeamPhase::Planning => "planning",
            TeamPhase::Running => "running",
            TeamPhase::Reviewing => "reviewing",
            TeamPhase::Completed => "completed",
            TeamPhase::Failed => "failed",
            TeamPhase::Conflict => "conflict",
            TeamPhase::Stopped => "stopped",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamTaskKind {
    Worker,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamTaskStatus {
    Planned,
    Assigned,
    Running,
    Reviewing,
    Completed,
    Failed,
    Conflict,
    Merged,
    Stopped,
}

impl fmt::Display for TeamTaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TeamTaskStatus::Planned => "planned",
            TeamTaskStatus::Assigned => "assigned",
            TeamTaskStatus::Running => "running",
            TeamTaskStatus::Reviewing => "reviewing",
            TeamTaskStatus::Completed => "completed",
            TeamTaskStatus::Failed => "failed",
            TeamTaskStatus::Conflict => "conflict",
            TeamTaskStatus::Merged => "merged",
            TeamTaskStatus::Stopped => "stopped",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamArtifact {
    pub path: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    pub id: String,
    pub kind: TeamTaskKind,
    pub title: String,
    pub prompt: String,
    pub status: TeamTaskStatus,
    pub worker_id: Option<String>,
    pub result: Option<String>,
    pub artifacts: Vec<TeamArtifact>,
    pub review_notes: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TeamTask {
    fn worker(index: usize, title: String, prompt: String) -> Self {
        let now = Utc::now();
        Self {
            id: format!("task-{index:03}"),
            kind: TeamTaskKind::Worker,
            title,
            prompt,
            status: TeamTaskStatus::Planned,
            worker_id: None,
            result: None,
            artifacts: Vec::new(),
            review_notes: None,
            error: None,
            duration_ms: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn review(index: usize, prompt: String) -> Self {
        let now = Utc::now();
        Self {
            id: format!("review-{index:03}"),
            kind: TeamTaskKind::Review,
            title: "Peer review and integration report".to_string(),
            prompt,
            status: TeamTaskStatus::Planned,
            worker_id: None,
            result: None,
            artifacts: Vec::new(),
            review_notes: None,
            error: None,
            duration_ms: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamBoard {
    pub id: String,
    pub goal: String,
    pub phase: TeamPhase,
    pub bus_config: TeamBusConfig,
    pub working_dir: PathBuf,
    pub tasks: Vec<TeamTask>,
    pub events: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TeamBoard {
    pub fn from_goal(goal: String, working_dir: PathBuf) -> Self {
        let now = Utc::now();
        let id = format!("team-{}", &Uuid::new_v4().to_string()[..8]);
        let tasks = planned_tasks_from_goal(&goal);
        let mut board = Self {
            id,
            goal,
            phase: TeamPhase::Planning,
            bus_config: TeamBusConfig::default(),
            working_dir,
            tasks,
            events: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        board.record_event("team board created");
        board
    }

    pub fn storage_path(&self) -> PathBuf {
        team_storage_dir().join(format!("{}.json", self.id))
    }

    pub fn load_latest_for_dir(working_dir: &Path) -> Option<Self> {
        let entries = std::fs::read_dir(team_storage_dir()).ok()?;
        let target = normalize_path_key(&working_dir.display().to_string());
        entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| {
                let data = std::fs::read_to_string(entry.path()).ok()?;
                let board: TeamBoard = serde_json::from_str(&data).ok()?;
                let board_dir = normalize_path_key(&board.working_dir.display().to_string());
                if board_dir == target {
                    Some(board)
                } else {
                    None
                }
            })
            .max_by_key(|board| board.updated_at.timestamp_millis())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = self.storage_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        std::fs::write(path, data)
    }

    pub fn record_event(&mut self, event: impl Into<String>) {
        self.updated_at = Utc::now();
        self.events.push(format!(
            "{}  {}",
            self.updated_at.format("%Y-%m-%d %H:%M:%S UTC"),
            event.into()
        ));
        if self.events.len() > 200 {
            let overflow = self.events.len() - 200;
            self.events.drain(0..overflow);
        }
    }

    pub fn active_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    TeamTaskStatus::Assigned | TeamTaskStatus::Running | TeamTaskStatus::Reviewing
                )
            })
            .count()
    }

    pub fn queued_worker_ids(&self) -> Vec<String> {
        if matches!(
            self.phase,
            TeamPhase::Completed | TeamPhase::Failed | TeamPhase::Conflict | TeamPhase::Stopped
        ) {
            return Vec::new();
        }
        let available = self
            .bus_config
            .max_parallel_workers
            .saturating_sub(self.active_count());
        self.tasks
            .iter()
            .filter(|task| {
                task.kind == TeamTaskKind::Worker && task.status == TeamTaskStatus::Planned
            })
            .take(available)
            .map(|task| task.id.clone())
            .collect()
    }

    pub fn get_task(&self, task_id: &str) -> Option<&TeamTask> {
        self.tasks.iter().find(|task| task.id == task_id)
    }

    pub fn mark_spawned(&mut self, task_id: &str, worker_id: String) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == task_id) {
            task.worker_id = Some(worker_id);
            task.status = match task.kind {
                TeamTaskKind::Worker => TeamTaskStatus::Running,
                TeamTaskKind::Review => TeamTaskStatus::Reviewing,
            };
            task.updated_at = Utc::now();
            self.phase = match task.kind {
                TeamTaskKind::Worker => TeamPhase::Running,
                TeamTaskKind::Review => TeamPhase::Reviewing,
            };
            self.record_event(format!("{task_id} spawned"));
        }
    }

    pub fn mark_completed(
        &mut self,
        task_id: &str,
        result: String,
        duration_ms: u64,
    ) -> Vec<TeamArtifact> {
        let artifacts = extract_artifacts(&result);
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == task_id) {
            task.status = TeamTaskStatus::Completed;
            task.result = Some(result);
            task.duration_ms = Some(duration_ms);
            task.artifacts = artifacts.clone();
            task.updated_at = Utc::now();
            self.record_event(format!("{task_id} completed"));
        }
        artifacts
    }

    pub fn mark_failed(&mut self, task_id: &str, error: String, duration_ms: u64) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == task_id) {
            task.status = TeamTaskStatus::Failed;
            task.error = Some(error);
            task.duration_ms = Some(duration_ms);
            task.updated_at = Utc::now();
            self.record_event(format!("{task_id} failed"));
        }
    }

    pub fn mark_stopped(&mut self) {
        self.phase = TeamPhase::Stopped;
        for task in &mut self.tasks {
            if matches!(
                task.status,
                TeamTaskStatus::Planned
                    | TeamTaskStatus::Assigned
                    | TeamTaskStatus::Running
                    | TeamTaskStatus::Reviewing
            ) {
                task.status = TeamTaskStatus::Stopped;
                task.updated_at = Utc::now();
            }
        }
        self.record_event("team stopped");
    }

    pub fn unresolved_worker_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| {
                task.kind == TeamTaskKind::Worker
                    && !matches!(
                        task.status,
                        TeamTaskStatus::Completed
                            | TeamTaskStatus::Failed
                            | TeamTaskStatus::Stopped
                    )
            })
            .count()
    }

    pub fn all_workers_finished(&self) -> bool {
        self.tasks
            .iter()
            .filter(|task| task.kind == TeamTaskKind::Worker)
            .all(|task| {
                matches!(
                    task.status,
                    TeamTaskStatus::Completed | TeamTaskStatus::Failed | TeamTaskStatus::Stopped
                )
            })
    }

    pub fn has_review_task(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.kind == TeamTaskKind::Review)
    }

    pub fn review_finished(&self) -> bool {
        self.tasks
            .iter()
            .filter(|task| task.kind == TeamTaskKind::Review)
            .all(|task| {
                matches!(
                    task.status,
                    TeamTaskStatus::Completed | TeamTaskStatus::Failed | TeamTaskStatus::Stopped
                )
            })
    }

    pub fn review_failed(&self) -> bool {
        self.tasks
            .iter()
            .filter(|task| task.kind == TeamTaskKind::Review)
            .any(|task| {
                matches!(
                    task.status,
                    TeamTaskStatus::Failed | TeamTaskStatus::Stopped
                )
            })
    }

    pub fn conflicts(&self) -> Vec<String> {
        let mut owners: HashMap<String, Vec<String>> = HashMap::new();
        for task in self
            .tasks
            .iter()
            .filter(|task| task.kind == TeamTaskKind::Worker)
        {
            for artifact in &task.artifacts {
                let normalized = normalize_path_key(&artifact.path);
                if !normalized.is_empty() {
                    owners.entry(normalized).or_default().push(task.id.clone());
                }
            }
        }
        owners
            .into_iter()
            .filter_map(|(path, task_ids)| {
                if task_ids.len() > 1 {
                    Some(format!("{} touched by {}", path, task_ids.join(", ")))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn add_review_task(&mut self) -> Option<String> {
        if self.has_review_task() {
            return None;
        }
        let index = self.tasks.len() + 1;
        let prompt = build_review_prompt(self);
        let task = TeamTask::review(index, prompt);
        let id = task.id.clone();
        self.tasks.push(task);
        self.phase = TeamPhase::Reviewing;
        self.record_event("review task created");
        Some(id)
    }

    pub fn queued_review_ids(&self) -> Vec<String> {
        self.tasks
            .iter()
            .filter(|task| {
                task.kind == TeamTaskKind::Review && task.status == TeamTaskStatus::Planned
            })
            .map(|task| task.id.clone())
            .collect()
    }

    pub fn refresh_phase_after_drain(&mut self) {
        let conflicts = self.conflicts();
        if !conflicts.is_empty() {
            let already_conflicted = self.phase == TeamPhase::Conflict;
            self.phase = TeamPhase::Conflict;
            for task in &mut self.tasks {
                if task.kind == TeamTaskKind::Worker
                    && task.artifacts.iter().any(|artifact| {
                        conflicts.iter().any(|conflict| {
                            conflict.starts_with(&normalize_path_key(&artifact.path))
                        })
                    })
                {
                    task.status = TeamTaskStatus::Conflict;
                    task.updated_at = Utc::now();
                }
            }
            if !already_conflicted {
                self.record_event(format!("conflict detected: {}", conflicts.join("; ")));
            }
            return;
        }

        if self.all_workers_finished() && self.has_review_task() && self.review_finished() {
            if self.review_failed() {
                self.phase = TeamPhase::Failed;
                self.record_event("team review failed");
                return;
            }
            self.phase = TeamPhase::Completed;
            for task in &mut self.tasks {
                if task.status == TeamTaskStatus::Completed {
                    task.status = TeamTaskStatus::Merged;
                    task.updated_at = Utc::now();
                }
            }
            self.record_event("team completed and merged");
        } else if self.all_workers_finished()
            && !self.bus_config.require_review
            && !self.has_review_task()
        {
            self.phase = TeamPhase::Completed;
            for task in &mut self.tasks {
                if task.status == TeamTaskStatus::Completed {
                    task.status = TeamTaskStatus::Merged;
                    task.updated_at = Utc::now();
                }
            }
            self.record_event("team completed without review");
        } else if self
            .tasks
            .iter()
            .any(|task| task.status == TeamTaskStatus::Failed)
            && self.active_count() == 0
            && self.unresolved_worker_count() == 0
        {
            self.phase = TeamPhase::Failed;
        }
    }

    pub fn worker_prompt(&self, task_id: &str) -> Option<String> {
        let task = self.get_task(task_id)?;
        Some(build_team_bus_context(self, task))
    }

    pub fn format_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Agent Team Board: {}\n\n", self.id));
        out.push_str(&format!("Phase: **{}**\n", self.phase));
        out.push_str(&format!("Goal: {}\n", self.goal));
        out.push_str(&format!(
            "Working directory: {}\n",
            self.working_dir.display()
        ));
        out.push_str(&format!(
            "Bus: max {} worker(s), review {}, conflict strategy: {}\n",
            self.bus_config.max_parallel_workers,
            if self.bus_config.require_review {
                "required"
            } else {
                "optional"
            },
            self.bus_config.conflict_strategy
        ));
        out.push_str(&format!("Saved at: {}\n\n", self.storage_path().display()));
        out.push_str("## Tasks\n");
        for task in &self.tasks {
            out.push_str(&format!(
                "- `{}` [{:?}] {} - {}",
                task.id, task.kind, task.status, task.title
            ));
            if let Some(worker_id) = &task.worker_id {
                out.push_str(&format!(" ({worker_id})"));
            }
            out.push('\n');
            if let Some(error) = &task.error {
                out.push_str(&format!("  Error: {error}\n"));
            }
            if !task.artifacts.is_empty() {
                out.push_str("  Artifacts:\n");
                for artifact in &task.artifacts {
                    out.push_str(&format!("  - {} - {}\n", artifact.path, artifact.summary));
                }
            }
            if let Some(result) = &task.result {
                out.push_str("  Result preview: ");
                out.push_str(&truncate(result, 320));
                out.push('\n');
            }
        }
        let conflicts = self.conflicts();
        if !conflicts.is_empty() {
            out.push_str("\n## Conflicts\n");
            for conflict in conflicts {
                out.push_str(&format!("- {conflict}\n"));
            }
        }
        if !self.events.is_empty() {
            out.push_str("\n## Recent Events\n");
            for event in self.events.iter().rev().take(20).rev() {
                out.push_str(&format!("- {event}\n"));
            }
        }
        out
    }
}

fn planned_tasks_from_goal(goal: &str) -> Vec<TeamTask> {
    let explicit: Vec<String> = goal
        .lines()
        .flat_map(|line| line.split(';'))
        .map(|part| part.trim().trim_start_matches("- ").trim().to_string())
        .filter(|part| part.len() > 8)
        .collect();

    let parts = if explicit.len() >= 2 {
        explicit
    } else {
        vec![
            format!(
                "Map the codebase context and identify the safest implementation seams for: {goal}"
            ),
            format!("Implement the requested change for: {goal}"),
            format!(
                "Prepare focused verification checks and risk areas for the implementation of: {goal}"
            ),
        ]
    };

    parts
        .into_iter()
        .take(8)
        .enumerate()
        .map(|(idx, part)| {
            let title = truncate(&part, 80);
            TeamTask::worker(idx + 1, title, part)
        })
        .collect()
}

fn build_team_bus_context(board: &TeamBoard, task: &TeamTask) -> String {
    let roster = board
        .tasks
        .iter()
        .filter(|candidate| candidate.kind == TeamTaskKind::Worker)
        .map(|candidate| {
            format!(
                "- {}: {} [{}]",
                candidate.id, candidate.title, candidate.status
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"You are a worker in a forge-osh Agent Team.

Common Team Bus
- Team id: {team_id}
- Task id: {task_id}
- Team goal: {goal}
- Working directory: {working_dir}
- Conflict strategy: {conflict_strategy}
- Your assignment: {title}

Team Roster
{roster}

Protocol
- Stay within your assignment and avoid overlapping another task's likely file ownership.
- Prefer read-only investigation unless your task explicitly requires code changes.
- If you edit files, report every changed path under an "Artifacts:" section.
- If you notice conflicting work, state it clearly instead of silently overwriting.
- End with a compact result summary and this artifact format:

Artifacts:
- path: relative/or/absolute/path
  summary: what changed or what was discovered
"#,
        team_id = board.id,
        task_id = task.id,
        goal = board.goal,
        working_dir = board.working_dir.display(),
        conflict_strategy = board.bus_config.conflict_strategy,
        title = task.title,
        roster = roster
    )
}

fn build_review_prompt(board: &TeamBoard) -> String {
    let mut prompt = String::new();
    prompt.push_str("You are the peer-review and integration worker for a forge-osh Agent Team.\n");
    prompt.push_str("Review the completed worker outputs for correctness, conflicts, missing tests, and integration risk.\n");
    prompt.push_str("Do not make broad new changes unless a small fix is clearly necessary. Prefer a precise merge/review report.\n\n");
    prompt.push_str(&format!("Team id: {}\nGoal: {}\n\n", board.id, board.goal));
    for task in board
        .tasks
        .iter()
        .filter(|task| task.kind == TeamTaskKind::Worker)
    {
        prompt.push_str(&format!("## {} - {}\n", task.id, task.title));
        prompt.push_str(&format!("Status: {}\n", task.status));
        if !task.artifacts.is_empty() {
            prompt.push_str("Artifacts:\n");
            for artifact in &task.artifacts {
                prompt.push_str(&format!("- {} - {}\n", artifact.path, artifact.summary));
            }
        }
        if let Some(result) = &task.result {
            prompt.push_str("Result:\n");
            prompt.push_str(result);
            prompt.push('\n');
        }
        if let Some(error) = &task.error {
            prompt.push_str(&format!("Error: {error}\n"));
        }
        prompt.push('\n');
    }
    prompt.push_str("Return: integration verdict, conflict notes, verification performed, and remaining risks.\n");
    prompt
}

pub fn extract_artifacts(result: &str) -> Vec<TeamArtifact> {
    let mut artifacts = Vec::new();
    let mut last_path: Option<String> = None;
    for line in result.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = lower
            .strip_prefix("path:")
            .or_else(|| lower.strip_prefix("file:"))
            .or_else(|| lower.strip_prefix("artifact:"))
        {
            let original_rest = &trimmed[trimmed.len() - rest.len()..];
            let path = original_rest
                .split(" - ")
                .next()
                .unwrap_or(original_rest)
                .trim()
                .trim_matches('`')
                .to_string();
            if !path.is_empty() {
                last_path = Some(path.clone());
                artifacts.push(TeamArtifact {
                    path,
                    summary: String::new(),
                });
            }
        } else if let Some(rest) = lower.strip_prefix("summary:") {
            if let Some(path) = last_path.as_ref() {
                let original_rest = &trimmed[trimmed.len() - rest.len()..];
                if let Some(artifact) = artifacts.iter_mut().rev().find(|a| &a.path == path) {
                    artifact.summary = original_rest.trim().to_string();
                }
            }
        }
    }
    artifacts
}

fn normalize_path_key(path: &str) -> String {
    let portable = path.replace('\\', "/");
    Path::new(&portable)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
        .trim()
        .trim_matches('`')
        .to_ascii_lowercase()
}

fn team_storage_dir() -> PathBuf {
    crate::config::data_dir().join("teams")
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in s.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semicolon_goal_creates_explicit_worker_tasks() {
        let board = TeamBoard::from_goal(
            "inspect auth; update auth tests; review auth risk".to_string(),
            PathBuf::from("."),
        );
        assert_eq!(board.tasks.len(), 3);
        assert_eq!(board.tasks[0].id, "task-001");
        assert!(board.tasks[0].prompt.contains("inspect auth"));
    }

    #[test]
    fn vague_goal_creates_default_team_shape() {
        let board = TeamBoard::from_goal("improve reliability".to_string(), PathBuf::from("."));
        assert_eq!(board.tasks.len(), 3);
        assert!(board.tasks[0].prompt.contains("Map the codebase context"));
        assert!(board.tasks[1]
            .prompt
            .contains("Implement the requested change"));
        assert!(board.tasks[2]
            .prompt
            .contains("Prepare focused verification checks"));
    }

    #[test]
    fn artifact_extraction_reads_path_summary_pairs() {
        let result = r#"
Done.

Artifacts:
- path: src/agent/team.rs
  summary: added team board lifecycle
- file: README.md - docs updated
  summary: documented team commands
"#;
        let artifacts = extract_artifacts(result);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].path, "src/agent/team.rs");
        assert_eq!(artifacts[0].summary, "added team board lifecycle");
        assert_eq!(artifacts[1].path, "README.md");
        assert_eq!(artifacts[1].summary, "documented team commands");
    }

    #[test]
    fn duplicate_artifact_paths_are_conflicts() {
        let mut board = TeamBoard::from_goal("one; two".to_string(), PathBuf::from("."));
        board.tasks[0].artifacts = vec![TeamArtifact {
            path: "src/lib.rs".to_string(),
            summary: "first".to_string(),
        }];
        board.tasks[1].artifacts = vec![TeamArtifact {
            path: "SRC\\lib.rs".to_string(),
            summary: "second".to_string(),
        }];
        let conflicts = board.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("task-001"));
        assert!(conflicts[0].contains("task-002"));
    }

    #[test]
    fn conflict_event_is_not_recorded_repeatedly() {
        let mut board = TeamBoard::from_goal("one; two".to_string(), PathBuf::from("."));
        board.tasks[0].artifacts = vec![TeamArtifact {
            path: "src/lib.rs".to_string(),
            summary: "first".to_string(),
        }];
        board.tasks[1].artifacts = vec![TeamArtifact {
            path: "src/lib.rs".to_string(),
            summary: "second".to_string(),
        }];
        board.refresh_phase_after_drain();
        board.refresh_phase_after_drain();
        let conflict_events = board
            .events
            .iter()
            .filter(|event| event.contains("conflict detected"))
            .count();
        assert_eq!(conflict_events, 1);
    }

    #[test]
    fn failed_review_keeps_team_failed_not_completed() {
        let mut board = TeamBoard::from_goal("one; two".to_string(), PathBuf::from("."));
        for task in &mut board.tasks {
            task.status = TeamTaskStatus::Completed;
        }
        let review_id = board.add_review_task().expect("review task");
        board.mark_failed(&review_id, "review API error".to_string(), 10);
        board.refresh_phase_after_drain();
        assert_eq!(board.phase, TeamPhase::Failed);
    }
}
