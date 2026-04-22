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

use crate::types::*;
use super::Tool;

// ---------------------------------------------------------------------------
// TodoWriteTool — writes a structured TODO list to .forge-osh/todos.md
// ---------------------------------------------------------------------------

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str { "todo_write" }

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

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Mutating }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let todos = match input["todos"].as_array() {
            Some(t) => t,
            None => return ToolOutput::error("Missing 'todos' parameter"),
        };

        let mut lines = vec![
            "# forge-osh Task List".to_string(),
            format!("*Updated: {}*", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
            String::new(),
        ];

        // Group by status
        let statuses = ["in_progress", "pending", "blocked", "completed"];
        let labels = ["In Progress", "Pending", "Blocked", "Completed"];
        let icons = ["▶", "○", "✖", "✓"];

        for (status, label, icon) in statuses.iter().zip(labels.iter()).zip(icons.iter()).map(|((s, l), i)| (s, l, i)) {
            let group: Vec<&Value> = todos.iter()
                .filter(|t| t["status"].as_str().unwrap_or("pending") == *status)
                .collect();

            if group.is_empty() { continue; }

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

                lines.push(format!("- {} **{}** {}{}", check, id, content, priority_badge));
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
                let done = todos.iter()
                    .filter(|t| t["status"].as_str().unwrap_or("") == "completed")
                    .count();
                let in_progress = todos.iter()
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
    fn name(&self) -> &str { "task_create" }

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

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

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
    fn name(&self) -> &str { "task_update" }

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

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

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
    fn name(&self) -> &str { "task_get" }
    fn is_concurrency_safe(&self) -> bool { true }
    fn description(&self) -> &str { "Get details of a specific task by ID." }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

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
                    format!("Created: {}", task.created_at.format("%Y-%m-%d %H:%M:%S UTC")),
                    format!("Updated: {}", task.updated_at.format("%Y-%m-%d %H:%M:%S UTC")),
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
    fn name(&self) -> &str { "task_list" }
    fn is_concurrency_safe(&self) -> bool { true }
    fn description(&self) -> &str { "List all tasks in the current session." }

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

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let filter = input["status_filter"].as_str().unwrap_or("all");
        let registry = TASK_REGISTRY.lock();

        let tasks: Vec<&TaskEntry> = registry.iter()
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

        let lines: Vec<String> = tasks.iter().map(|t| {
            format!("{} [{}] {} — {}", icon(&t.status), t.id, t.subject, t.status)
        }).collect();

        ToolOutput::success(format!(
            "Tasks ({}):\n\n{}",
            tasks.len(),
            lines.join("\n")
        ))
    }
}
