use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::process::Command;

use crate::types::*;
use super::Tool;

/// Helper to run a git command and capture output
async fn run_git(args: &[&str], working_dir: &std::path::Path) -> std::result::Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

// ─── git_status ───────────────────────────────────────────────────────────

pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str { "git_status" }
    fn description(&self) -> &str { "Show the working tree status (like `git status --short`)." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, _input: Value, ctx: &ToolContext) -> ToolOutput {
        match run_git(&["status", "--short", "--branch"], &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(if out.trim().is_empty() {
                "Working tree clean".to_string()
            } else {
                out
            }),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_diff ─────────────────────────────────────────────────────────────

pub struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str { "git_diff" }
    fn description(&self) -> &str {
        "Show changes. Use 'staged' for staged changes, 'commit' to diff between commits."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "staged": { "type": "boolean", "description": "Show staged changes", "default": false },
                "commit": { "type": "string", "description": "Commit or range (e.g., 'HEAD~3..HEAD')" },
                "path": { "type": "string", "description": "Limit diff to specific path" }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let mut args = vec!["diff"];

        let staged = input["staged"].as_bool().unwrap_or(false);
        if staged {
            args.push("--cached");
        }

        let commit_val = input["commit"].as_str().map(String::from);
        if let Some(ref c) = commit_val {
            args.push(c);
        }

        args.push("--");

        let path_val = input["path"].as_str().map(String::from);
        if let Some(ref p) = path_val {
            args.push(p);
        }

        match run_git(&args, &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(if out.trim().is_empty() {
                "No differences found".to_string()
            } else {
                out
            }),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_log ──────────────────────────────────────────────────────────────

pub struct GitLogTool;

#[async_trait]
impl Tool for GitLogTool {
    fn name(&self) -> &str { "git_log" }
    fn description(&self) -> &str { "Show commit history." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer", "description": "Number of commits to show", "default": 10 },
                "oneline": { "type": "boolean", "default": true },
                "path": { "type": "string", "description": "Limit to specific path" }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let count = input["count"].as_u64().unwrap_or(10);
        let oneline = input["oneline"].as_bool().unwrap_or(true);
        let count_str = format!("-{count}");

        let mut args = vec!["log", &count_str];
        if oneline {
            args.push("--oneline");
        }
        args.push("--decorate");

        let path_val = input["path"].as_str().map(String::from);
        if path_val.is_some() {
            args.push("--");
        }
        if let Some(ref p) = path_val {
            args.push(p);
        }

        match run_git(&args, &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(out),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_add ──────────────────────────────────────────────────────────────

pub struct GitAddTool;

#[async_trait]
impl Tool for GitAddTool {
    fn name(&self) -> &str { "git_add" }
    fn description(&self) -> &str { "Stage files for commit." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Files to stage. Use ['.'] for all."
                }
            },
            "required": ["paths"]
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Mutating }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let paths = match input["paths"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>(),
            None => return ToolOutput::error("Missing 'paths' parameter"),
        };

        let mut args: Vec<&str> = vec!["add"];
        args.extend(paths.iter());

        match run_git(&args, &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(format!(
                "Staged {} file(s){}",
                paths.len(),
                if out.trim().is_empty() {
                    String::new()
                } else {
                    format!("\n{out}")
                }
            )),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_commit ───────────────────────────────────────────────────────────

pub struct GitCommitTool;

#[async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str { "git_commit" }
    fn description(&self) -> &str { "Create a git commit with staged changes." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Commit message" }
            },
            "required": ["message"]
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Mutating }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let message = match input["message"].as_str() {
            Some(m) => m,
            None => return ToolOutput::error("Missing 'message' parameter"),
        };

        match run_git(&["commit", "-m", message], &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(out),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_branch ───────────────────────────────────────────────────────────

pub struct GitBranchTool;

#[async_trait]
impl Tool for GitBranchTool {
    fn name(&self) -> &str { "git_branch" }
    fn description(&self) -> &str { "List, create, or delete branches." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "create", "delete"], "default": "list" },
                "name": { "type": "string", "description": "Branch name (for create/delete)" }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Mutating }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let action = input["action"].as_str().unwrap_or("list");
        let name = input["name"].as_str();

        match action {
            "list" => match run_git(&["branch", "-a"], &ctx.working_dir).await {
                Ok(out) => ToolOutput::success(out),
                Err(e) => ToolOutput::error(e),
            },
            "create" => {
                let branch = match name {
                    Some(n) => n,
                    None => return ToolOutput::error("Missing 'name' for branch creation"),
                };
                match run_git(&["branch", branch], &ctx.working_dir).await {
                    Ok(out) => ToolOutput::success(format!("Created branch: {branch}\n{out}")),
                    Err(e) => ToolOutput::error(e),
                }
            }
            "delete" => {
                let branch = match name {
                    Some(n) => n,
                    None => return ToolOutput::error("Missing 'name' for branch deletion"),
                };
                match run_git(&["branch", "-d", branch], &ctx.working_dir).await {
                    Ok(out) => ToolOutput::success(format!("Deleted branch: {branch}\n{out}")),
                    Err(e) => ToolOutput::error(e),
                }
            }
            _ => ToolOutput::error(format!("Unknown action: {action}")),
        }
    }
}

// ─── git_checkout ─────────────────────────────────────────────────────────

pub struct GitCheckoutTool;

#[async_trait]
impl Tool for GitCheckoutTool {
    fn name(&self) -> &str { "git_checkout" }
    fn description(&self) -> &str { "Switch branches or restore files." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "branch": { "type": "string", "description": "Branch to checkout" },
                "create": { "type": "boolean", "description": "Create new branch (-b)", "default": false },
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Restore specific files"
                }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Mutating }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let branch = input["branch"].as_str();
        let create = input["create"].as_bool().unwrap_or(false);
        let paths = input["paths"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            });

        if let Some(b) = branch {
            let mut args = vec!["checkout"];
            if create {
                args.push("-b");
            }
            args.push(b);

            match run_git(&args, &ctx.working_dir).await {
                Ok(out) => ToolOutput::success(format!("Checked out: {b}\n{out}")),
                Err(e) => ToolOutput::error(e),
            }
        } else if let Some(files) = paths {
            let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
            let mut args = vec!["checkout", "--"];
            args.extend(file_refs);

            match run_git(&args, &ctx.working_dir).await {
                Ok(out) => ToolOutput::success(format!("Restored files\n{out}")),
                Err(e) => ToolOutput::error(e),
            }
        } else {
            ToolOutput::error("Specify either 'branch' or 'paths'")
        }
    }
}
