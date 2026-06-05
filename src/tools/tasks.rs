//! TodoWriteTool + Task management tools (TaskCreate/Update/Get/List)
//!
//! These tools let the agent manage its own task list in `.forge-osh/todos.md`
//! and track in-session tasks with status (pending → in_progress → completed).

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::fs;

use super::Tool;
use crate::session::task_plan::{PlanStep, PlanTask, StepStatus, TaskPlan};
use crate::types::*;

// ---------------------------------------------------------------------------
// TodoWriteTool — writes a structured TODO list to .forge-osh/todos.md
// ---------------------------------------------------------------------------

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }

    fn description(&self) -> &str {
        "Write a structured TODO list to .forge-osh/todos.md so the agent can track its work plan. \
        Use this to plan steps before executing a complex task, or to update progress. \
        Pass an array of todo items, each with a status and content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "List of todo items",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Unique ID (e.g. '1', '2a')" },
                            "content": { "type": "string", "description": "Task description" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "blocked"],
                                "description": "Task status"
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["high", "medium", "low"],
                                "description": "Priority level"
                            },
                            "notes": { "type": "string", "description": "Additional notes" }
                        },
                        "required": ["id", "content", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let todos = match input["todos"].as_array() {
            Some(t) => t,
            None => return ToolOutput::error("Missing 'todos' parameter"),
        };

        let mut lines = vec![
            "# forge-osh Task List".to_string(),
            format!(
                "*Updated: {}*",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            ),
            String::new(),
        ];

        // Group by status
        let statuses = ["in_progress", "pending", "blocked", "completed"];
        let labels = ["In Progress", "Pending", "Blocked", "Completed"];
        let icons = ["▶", "○", "✖", "✓"];

        for (status, label, icon) in statuses
            .iter()
            .zip(labels.iter())
            .zip(icons.iter())
            .map(|((s, l), i)| (s, l, i))
        {
            let group: Vec<&Value> = todos
                .iter()
                .filter(|t| t["status"].as_str().unwrap_or("pending") == *status)
                .collect();

            if group.is_empty() {
                continue;
            }

            lines.push(format!("## {} {}", icon, label));
            lines.push(String::new());

            for item in group {
                let id = item["id"].as_str().unwrap_or("?");
                let content = item["content"].as_str().unwrap_or("(empty)");
                let priority = item["priority"].as_str().unwrap_or("medium");
                let notes = item["notes"].as_str().unwrap_or("");

                let priority_badge = match priority {
                    "high" => " `HIGH`",
                    "low" => " `LOW`",
                    _ => "",
                };

                let check = match *status {
                    "completed" => "[x]",
                    "in_progress" => "[~]",
                    "blocked" => "[!]",
                    _ => "[ ]",
                };

                lines.push(format!(
                    "- {} **{}** {}{}",
                    check, id, content, priority_badge
                ));
                if !notes.is_empty() {
                    lines.push(format!("  > {}", notes));
                }
            }
            lines.push(String::new());
        }

        let content = lines.join("\n");

        // Write to .forge-osh/todos.md in working dir
        let todos_dir = ctx.working_dir.join(".forge-osh");
        if let Err(e) = fs::create_dir_all(&todos_dir).await {
            return ToolOutput::error(format!("Failed to create .forge-osh directory: {e}"));
        }

        let todos_path = todos_dir.join("todos.md");
        match fs::write(&todos_path, &content).await {
            Ok(_) => {
                let total = todos.len();
                let done = todos
                    .iter()
                    .filter(|t| t["status"].as_str().unwrap_or("") == "completed")
                    .count();
                let in_progress = todos
                    .iter()
                    .filter(|t| t["status"].as_str().unwrap_or("") == "in_progress")
                    .count();
                ToolOutput::success(format!(
                    "TODO list updated: {total} tasks ({in_progress} in progress, {done} completed) → {}",
                    todos_path.display()
                ))
            }
            Err(e) => ToolOutput::error(format!("Failed to write todos.md: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// UpdatePlanTool — the persistent, real-time task planner
//
// This is the primary planner. The model declares the full plan (a list of
// tasks, each with individual steps and statuses); every call replaces the
// persisted plan for the session. Because each call rewrites the whole plan,
// ticking a step off is just another call with that step's status flipped to
// `completed`. The agent loop re-emits the plan to the TUI after every call so
// steps tick off live, and persists it to disk so the plan survives restarts
// and interruptions.
// ---------------------------------------------------------------------------

pub struct UpdatePlanTool;

/// Parse one step from either a plain string or an object.
fn parse_step(v: &Value, idx: usize) -> Option<PlanStep> {
    if let Some(s) = v.as_str() {
        if s.trim().is_empty() {
            return None;
        }
        return Some(PlanStep {
            id: format!("{}", idx + 1),
            content: s.to_string(),
            status: StepStatus::Pending,
            note: None,
        });
    }
    let obj = v.as_object()?;
    let content = obj
        .get("content")
        .or_else(|| obj.get("step"))
        .or_else(|| obj.get("text"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    if content.trim().is_empty() {
        return None;
    }
    let status = obj
        .get("status")
        .and_then(|s| s.as_str())
        .map(StepStatus::from_str)
        .unwrap_or(StepStatus::Pending);
    let id = obj
        .get("id")
        .and_then(|i| i.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("{}", idx + 1));
    let note = obj
        .get("note")
        .and_then(|n| n.as_str())
        .filter(|n| !n.trim().is_empty())
        .map(String::from);
    Some(PlanStep {
        id,
        content,
        status,
        note,
    })
}

/// Parse the `tasks` array (full form) or a single-task `steps` array.
fn parse_tasks(input: &Value) -> Vec<PlanTask> {
    let now = chrono::Utc::now();
    let mut out = Vec::new();

    if let Some(tasks) = input.get("tasks").and_then(|t| t.as_array()) {
        for (ti, tv) in tasks.iter().enumerate() {
            let obj = match tv.as_object() {
                Some(o) => o,
                None => continue,
            };
            let subject = obj
                .get("subject")
                .or_else(|| obj.get("title"))
                .or_else(|| obj.get("name"))
                .and_then(|s| s.as_str())
                .unwrap_or("Task")
                .to_string();
            let id = obj
                .get("id")
                .and_then(|i| i.as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("task-{}", ti + 1));
            let steps = obj
                .get("steps")
                .and_then(|s| s.as_array())
                .map(|arr| {
                    arr.iter()
                        .enumerate()
                        .filter_map(|(i, s)| parse_step(s, i))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            out.push(PlanTask {
                id,
                subject,
                steps,
                created_at: now,
                updated_at: now,
            });
        }
        return out;
    }

    // Simple single-task form: a top-level `steps` array.
    if let Some(steps) = input.get("steps").and_then(|s| s.as_array()) {
        let subject = input
            .get("title")
            .or_else(|| input.get("subject"))
            .and_then(|s| s.as_str())
            .unwrap_or("Plan")
            .to_string();
        let parsed: Vec<PlanStep> = steps
            .iter()
            .enumerate()
            .filter_map(|(i, s)| parse_step(s, i))
            .collect();
        out.push(PlanTask {
            id: "task-1".to_string(),
            subject,
            steps: parsed,
            created_at: now,
            updated_at: now,
        });
    }
    out
}

#[async_trait]
impl Tool for UpdatePlanTool {
    fn name(&self) -> &str {
        "update_plan"
    }

    fn description(&self) -> &str {
        "Create or update the live, persistent task plan for this session. This is the PRIMARY way \
        to track multi-step work: lay out the steps up front, then call this again to tick steps off \
        as you complete them. Each call replaces the whole plan, so to mark progress you re-send the \
        full task list with updated `status` values. The plan is shown to the user as a live checklist \
        that ticks off in real time, and it persists across restarts and interruptions. \
        Set exactly one step to `in_progress` at a time (the one you are currently working on), mark \
        finished steps `completed`, and use `blocked` for steps you cannot finish. \
        Pass `tasks` (a list of {id?, subject, steps:[{id?, content, status?, note?}]}). For a simple \
        single-track plan you may instead pass a top-level `steps` array."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short heading for the plan (e.g. 'Plan', 'Updated Plan')."
                },
                "tasks": {
                    "type": "array",
                    "description": "Central list of tasks, each with its own steps.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Stable task id (e.g. 'task-1'). Auto-assigned if omitted." },
                            "subject": { "type": "string", "description": "Short task title." },
                            "steps": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "content": { "type": "string", "description": "Step description." },
                                        "status": {
                                            "type": "string",
                                            "enum": ["pending", "in_progress", "completed", "blocked"]
                                        },
                                        "note": { "type": "string", "description": "Optional short note." }
                                    },
                                    "required": ["content"]
                                }
                            }
                        },
                        "required": ["subject"]
                    }
                },
                "steps": {
                    "type": "array",
                    "description": "Shortcut for a single-task plan: a flat list of steps.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "blocked"]
                            }
                        },
                        "required": ["content"]
                    }
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // ReadOnly so the live planner never interrupts the flow with a
        // permission prompt — it only writes session-private metadata under
        // ~/.forge-osh/tasks, never the user's workspace.
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let tasks = parse_tasks(&input);
        if tasks.is_empty() {
            return ToolOutput::error(
                "update_plan needs a non-empty 'tasks' array (each with a 'subject' and 'steps'), \
                 or a top-level 'steps' array for a single-track plan.",
            );
        }

        let title = input
            .get("title")
            .and_then(|t| t.as_str())
            .map(String::from);

        let mut plan = TaskPlan::load(&ctx.session_id);
        plan.session_id = ctx.session_id.clone();
        plan.apply_update(title, tasks);

        if let Err(e) = plan.save() {
            return ToolOutput::error(format!("Failed to persist task plan: {e}"));
        }

        let (done, total) = plan.progress();
        let summary = format!(
            "Task plan updated: {} task(s), {}/{} steps complete.\n{}",
            plan.tasks.len(),
            done,
            total,
            plan.to_prompt_block()
        );

        let mut out = ToolOutput::success(summary);
        // The agent loop reads this metadata and re-emits the plan to the TUI
        // as AgentEvent::PlanUpdated so steps tick off live.
        out.metadata = Some(json!({
            "plan_update": serde_json::to_value(&plan).unwrap_or(Value::Null)
        }));
        out
    }
}

// ---------------------------------------------------------------------------
// In-session Task Registry (shared state via Arc<Mutex<>>)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: TaskStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Failed => write!(f, "failed"),
        }
    }
}

impl TaskStatus {
    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => TaskStatus::InProgress,
            "completed" => TaskStatus::Completed,
            "failed" => TaskStatus::Failed,
            _ => TaskStatus::Pending,
        }
    }
}

/// Global in-session task registry
static TASK_REGISTRY: once_cell::sync::Lazy<Arc<Mutex<Vec<TaskEntry>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

// ---------------------------------------------------------------------------
// TaskCreateTool
// ---------------------------------------------------------------------------

pub struct TaskCreateTool;

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "task_create"
    }

    fn description(&self) -> &str {
        "Create a tracked task in this session. Returns the task ID. \
        Use this to track parallel workstreams or long-running operations."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string", "description": "Short task title (< 80 chars)" },
                "description": { "type": "string", "description": "Detailed task description" }
            },
            "required": ["subject"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let subject = match input["subject"].as_str() {
            Some(s) => s.to_string(),
            None => return ToolOutput::error("Missing 'subject' parameter"),
        };
        let description = input["description"].as_str().unwrap_or("").to_string();

        let mut registry = TASK_REGISTRY.lock();
        let id = format!("task-{}", registry.len() + 1);
        let now = chrono::Utc::now();

        registry.push(TaskEntry {
            id: id.clone(),
            subject: subject.clone(),
            description,
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
            output: None,
        });

        ToolOutput::success(format!("Created task {id}: {subject}"))
    }
}

// ---------------------------------------------------------------------------
// TaskUpdateTool
// ---------------------------------------------------------------------------

pub struct TaskUpdateTool;

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "task_update"
    }

    fn description(&self) -> &str {
        "Update a task's status (pending → in_progress → completed/failed) and optionally set its output."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Task ID returned by task_create" },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed"]
                },
                "output": { "type": "string", "description": "Optional output or result message" }
            },
            "required": ["id", "status"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let id = match input["id"].as_str() {
            Some(s) => s,
            None => return ToolOutput::error("Missing 'id' parameter"),
        };
        let status_str = match input["status"].as_str() {
            Some(s) => s,
            None => return ToolOutput::error("Missing 'status' parameter"),
        };
        let output_msg = input["output"].as_str().map(String::from);

        let mut registry = TASK_REGISTRY.lock();
        match registry.iter_mut().find(|t| t.id == id) {
            Some(task) => {
                task.status = TaskStatus::from_str(status_str);
                task.updated_at = chrono::Utc::now();
                if let Some(out) = output_msg {
                    task.output = Some(out);
                }
                ToolOutput::success(format!("Task {id} → {status_str}"))
            }
            None => ToolOutput::error(format!("Task not found: {id}")),
        }
    }
}

// ---------------------------------------------------------------------------
// TaskGetTool
// ---------------------------------------------------------------------------

pub struct TaskGetTool;

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        "task_get"
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn description(&self) -> &str {
        "Get details of a specific task by ID."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let id = match input["id"].as_str() {
            Some(s) => s,
            None => return ToolOutput::error("Missing 'id' parameter"),
        };

        let registry = TASK_REGISTRY.lock();
        match registry.iter().find(|t| t.id == id) {
            Some(task) => {
                let mut lines = vec![
                    format!("Task: {}", task.id),
                    format!("Subject: {}", task.subject),
                    format!("Status: {}", task.status),
                    format!("Description: {}", task.description),
                    format!(
                        "Created: {}",
                        task.created_at.format("%Y-%m-%d %H:%M:%S UTC")
                    ),
                    format!(
                        "Updated: {}",
                        task.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
                    ),
                ];
                if let Some(ref out) = task.output {
                    lines.push(format!("Output: {out}"));
                }
                ToolOutput::success(lines.join("\n"))
            }
            None => ToolOutput::error(format!("Task not found: {id}")),
        }
    }
}

// ---------------------------------------------------------------------------
// TaskListTool
// ---------------------------------------------------------------------------

pub struct TaskListTool;

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "task_list"
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn description(&self) -> &str {
        "List all tasks in the current session."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status_filter": {
                    "type": "string",
                    "enum": ["all", "pending", "in_progress", "completed", "failed"],
                    "description": "Filter by status (default: all)"
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let filter = input["status_filter"].as_str().unwrap_or("all");
        let registry = TASK_REGISTRY.lock();

        let tasks: Vec<&TaskEntry> = registry
            .iter()
            .filter(|t| filter == "all" || t.status.to_string() == filter)
            .collect();

        if tasks.is_empty() {
            return ToolOutput::success(if filter == "all" {
                "No tasks in this session. Use task_create to create tasks.".to_string()
            } else {
                format!("No tasks with status '{filter}'.")
            });
        }

        let icon = |s: &TaskStatus| match s {
            TaskStatus::Pending => "○",
            TaskStatus::InProgress => "▶",
            TaskStatus::Completed => "✓",
            TaskStatus::Failed => "✖",
        };

        let lines: Vec<String> = tasks
            .iter()
            .map(|t| {
                format!(
                    "{} [{}] {} — {}",
                    icon(&t.status),
                    t.id,
                    t.subject,
                    t.status
                )
            })
            .collect();

        ToolOutput::success(format!("Tasks ({}):\n\n{}", tasks.len(), lines.join("\n")))
    }
}
