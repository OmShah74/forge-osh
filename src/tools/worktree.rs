//! Git worktree tools: enter_worktree / exit_worktree / list_worktrees.
//!
//! Worktrees let the agent work in an isolated checkout without disturbing
//! the main working tree. Risky refactors or experiments can run inside a
//! temporary worktree; the main branch stays clean.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::Tool;
use crate::types::*;

/// Registry of active worktrees created in this session.
static WORKTREE_REGISTRY: Lazy<Arc<Mutex<Vec<WorktreeEntry>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

#[derive(Debug, Clone)]
struct WorktreeEntry {
    path: String,
    #[allow(dead_code)]
    branch: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn run_git(args: &[&str], working_dir: &std::path::Path) -> Result<String, String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(working_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to spawn git: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("{}{}", stdout, stderr))
    }
}

// ---------------------------------------------------------------------------
// enter_worktree
// ---------------------------------------------------------------------------

pub struct EnterWorktreeTool;

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        "enter_worktree"
    }

    fn description(&self) -> &str {
        "Create a new git worktree at the given path, optionally on a new branch. \
        Use this to isolate risky or experimental changes from the main working tree. \
        Returns the path of the created worktree."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path for the new worktree (created if absent)"
                },
                "branch": {
                    "type": "string",
                    "description": "Branch name to create/checkout in the worktree. \
                                   If omitted a name is auto-generated."
                },
                "base_branch": {
                    "type": "string",
                    "description": "Branch to base the new branch on (default: current HEAD)"
                }
            },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };

        // Resolve path
        let wt_path = if std::path::Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            ctx.working_dir.join(path_str)
        };

        // Generate branch name if not provided
        let branch = input["branch"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!(
                    "forge-worktree-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                )
            });

        // Build git worktree add command
        let path_display = wt_path.to_string_lossy().to_string();
        let mut args: Vec<&str> = vec!["worktree", "add", "-b", &branch, &path_display];

        let base = input["base_branch"].as_str().unwrap_or("HEAD");
        args.push(base);

        match run_git(&args, &ctx.working_dir).await {
            Ok(_) => {
                WORKTREE_REGISTRY.lock().await.push(WorktreeEntry {
                    path: path_display.clone(),
                    branch: branch.clone(),
                });
                ToolOutput::success(format!(
                    "Created worktree at: {path_display}\nBranch: {branch}\n\
                    Use exit_worktree with the same path to remove it when done."
                ))
            }
            Err(e) => ToolOutput::error(format!("Failed to create worktree: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// exit_worktree
// ---------------------------------------------------------------------------

pub struct ExitWorktreeTool;

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "exit_worktree"
    }

    fn description(&self) -> &str {
        "Remove a git worktree previously created with enter_worktree. \
        Cleans up the directory and git metadata. The branch is preserved."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path of the worktree to remove (must match what was passed to enter_worktree)"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force removal even if there are uncommitted changes (default: false)"
                }
            },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Destructive
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };

        let wt_path = if std::path::Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            ctx.working_dir.join(path_str)
        };
        let path_display = wt_path.to_string_lossy().to_string();

        let force = input["force"].as_bool().unwrap_or(false);
        let mut args: Vec<&str> = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path_display);

        match run_git(&args, &ctx.working_dir).await {
            Ok(_) => {
                // Remove from registry
                let mut reg = WORKTREE_REGISTRY.lock().await;
                reg.retain(|e| e.path != path_display);
                ToolOutput::success(format!("Removed worktree: {path_display}"))
            }
            Err(e) => ToolOutput::error(format!("Failed to remove worktree: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// list_worktrees
// ---------------------------------------------------------------------------

pub struct ListWorktreesTool;

#[async_trait]
impl Tool for ListWorktreesTool {
    fn name(&self) -> &str {
        "list_worktrees"
    }

    fn description(&self) -> &str {
        "List all git worktrees in the repository, including which ones were \
        created by this session."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolOutput {
        match run_git(&["worktree", "list", "--porcelain"], &ctx.working_dir).await {
            Ok(output) => {
                let session_wts: Vec<String> = {
                    WORKTREE_REGISTRY
                        .lock()
                        .await
                        .iter()
                        .map(|e| e.path.clone())
                        .collect()
                };

                let mut lines = vec!["Git worktrees:".to_string()];
                for block in output.split("\n\n") {
                    let block = block.trim();
                    if block.is_empty() {
                        continue;
                    }
                    let is_session = session_wts.iter().any(|p| block.contains(p.as_str()));
                    let tag = if is_session { " [this session]" } else { "" };
                    lines.push(format!("{block}{tag}"));
                    lines.push(String::new());
                }
                ToolOutput::success(lines.join("\n"))
            }
            Err(e) => ToolOutput::error(format!("Failed to list worktrees: {e}")),
        }
    }
}
