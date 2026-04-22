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
    fn is_concurrency_safe(&self) -> bool { true }
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
    fn is_concurrency_safe(&self) -> bool { true }
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
    fn is_concurrency_safe(&self) -> bool { true }
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

// ─── git_stash ────────────────────────────────────────────────────────────

pub struct GitStashTool;

#[async_trait]
impl Tool for GitStashTool {
    fn name(&self) -> &str { "git_stash" }
    fn description(&self) -> &str {
        "Stash uncommitted changes. Actions: 'push' (stash with optional message), 'pop' (restore latest stash), 'list' (show stashes)."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["push", "pop", "list"], "default": "list" },
                "message": { "type": "string", "description": "Stash message (for push)" }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Mutating }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let action = input["action"].as_str().unwrap_or("list");
        match action {
            "list" => {
                match run_git(&["stash", "list"], &ctx.working_dir).await {
                    Ok(out) => ToolOutput::success(if out.trim().is_empty() {
                        "No stashes found".to_string()
                    } else { out }),
                    Err(e) => ToolOutput::error(e),
                }
            }
            "push" => {
                let result = if let Some(m) = input["message"].as_str() {
                    run_git(&["stash", "push", "-m", m], &ctx.working_dir).await
                } else {
                    run_git(&["stash", "push"], &ctx.working_dir).await
                };
                match result {
                    Ok(out) => ToolOutput::success(format!("Changes stashed\n{out}")),
                    Err(e) => ToolOutput::error(e),
                }
            }
            "pop" => {
                match run_git(&["stash", "pop"], &ctx.working_dir).await {
                    Ok(out) => ToolOutput::success(format!("Stash applied\n{out}")),
                    Err(e) => ToolOutput::error(e),
                }
            }
            _ => ToolOutput::error(format!("Unknown action: {action}")),
        }
    }
}

// ─── git_blame ────────────────────────────────────────────────────────────

pub struct GitBlameTool;

#[async_trait]
impl Tool for GitBlameTool {
    fn name(&self) -> &str { "git_blame" }
    fn is_concurrency_safe(&self) -> bool { true }
    fn description(&self) -> &str { "Show who last modified each line of a file (git blame)." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": { "type": "string", "description": "File path to blame" },
                "line_range": { "type": "string", "description": "Optional line range, e.g. '10,20'" }
            },
            "required": ["file"]
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let file = match input["file"].as_str() {
            Some(f) => f,
            None => return ToolOutput::error("Missing 'file' parameter"),
        };
        let result = if let Some(range) = input["line_range"].as_str() {
            run_git(&["blame", "-L", range, file], &ctx.working_dir).await
        } else {
            run_git(&["blame", file], &ctx.working_dir).await
        };
        match result {
            Ok(out) => ToolOutput::success(out),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_show ─────────────────────────────────────────────────────────────

pub struct GitShowTool;

#[async_trait]
impl Tool for GitShowTool {
    fn name(&self) -> &str { "git_show" }
    fn is_concurrency_safe(&self) -> bool { true }
    fn description(&self) -> &str { "Show a commit, tag, or other git object (git show <ref>)." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ref": { "type": "string", "description": "Commit hash, tag, or ref to show", "default": "HEAD" },
                "stat": { "type": "boolean", "description": "Show stat summary instead of full diff", "default": false }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let git_ref = input["ref"].as_str().unwrap_or("HEAD");
        let stat = input["stat"].as_bool().unwrap_or(false);
        let mut args = vec!["show"];
        if stat {
            args.push("--stat");
        }
        args.push(git_ref);
        match run_git(&args, &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(out),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_reset ────────────────────────────────────────────────────────────

pub struct GitResetTool;

#[async_trait]
impl Tool for GitResetTool {
    fn name(&self) -> &str { "git_reset" }
    fn description(&self) -> &str {
        "Reset current HEAD to a specified state. Mode: 'soft' (keep staged), 'mixed' (unstage, keep files), 'hard' (discard all changes)."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["soft", "mixed", "hard"], "default": "mixed" },
                "ref": { "type": "string", "description": "Commit ref to reset to", "default": "HEAD" }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Destructive }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let mode = input["mode"].as_str().unwrap_or("mixed");
        let git_ref = input["ref"].as_str().unwrap_or("HEAD");
        let mode_flag = match mode {
            "soft" => "--soft",
            "hard" => "--hard",
            _ => "--mixed",
        };
        match run_git(&["reset", mode_flag, git_ref], &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(format!("Reset ({mode}) to {git_ref}\n{out}")),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_fetch ────────────────────────────────────────────────────────────

pub struct GitFetchTool;

#[async_trait]
impl Tool for GitFetchTool {
    fn name(&self) -> &str { "git_fetch" }
    fn description(&self) -> &str { "Fetch from a remote (git fetch <remote>)." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "remote": { "type": "string", "description": "Remote name", "default": "origin" },
                "all": { "type": "boolean", "description": "Fetch all remotes", "default": false }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Network }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let all = input["all"].as_bool().unwrap_or(false);
        let remote = input["remote"].as_str().unwrap_or("origin").to_string();
        let result = if all {
            run_git(&["fetch", "--all"], &ctx.working_dir).await
        } else {
            run_git(&["fetch", &remote], &ctx.working_dir).await
        };
        match result {
            Ok(out) => ToolOutput::success(format!("Fetched from {remote}\n{out}")),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_push ─────────────────────────────────────────────────────────────

pub struct GitPushTool;

#[async_trait]
impl Tool for GitPushTool {
    fn name(&self) -> &str { "git_push" }
    fn description(&self) -> &str { "Push commits to a remote repository (git push <remote> <branch>)." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "remote": { "type": "string", "description": "Remote name", "default": "origin" },
                "branch": { "type": "string", "description": "Branch to push (default: current branch)" },
                "force": { "type": "boolean", "description": "Force push (--force-with-lease for safety)", "default": false },
                "set_upstream": { "type": "boolean", "description": "Set upstream (-u)", "default": false }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Network }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let remote = input["remote"].as_str().unwrap_or("origin").to_string();
        let branch = input["branch"].as_str().map(|s| s.to_string());
        let force = input["force"].as_bool().unwrap_or(false);
        let set_upstream = input["set_upstream"].as_bool().unwrap_or(false);

        let mut args: Vec<&str> = vec!["push"];
        if force {
            args.push("--force-with-lease");
        }
        if set_upstream {
            args.push("-u");
        }
        args.push(&remote);
        if let Some(ref b) = branch {
            args.push(b.as_str());
        }

        match run_git(&args, &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(format!("Pushed to {remote}\n{out}")),
            Err(e) => ToolOutput::error(e),
        }
    }
}

// ─── git_pull ─────────────────────────────────────────────────────────────

pub struct GitPullTool;

#[async_trait]
impl Tool for GitPullTool {
    fn name(&self) -> &str { "git_pull" }
    fn description(&self) -> &str { "Pull changes from a remote repository (git pull <remote> <branch>)." }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "remote": { "type": "string", "description": "Remote name", "default": "origin" },
                "branch": { "type": "string", "description": "Branch to pull" },
                "rebase": { "type": "boolean", "description": "Use rebase instead of merge", "default": false }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Network }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let remote = input["remote"].as_str().unwrap_or("origin").to_string();
        let branch = input["branch"].as_str().map(|s| s.to_string());
        let rebase = input["rebase"].as_bool().unwrap_or(false);

        let mut args: Vec<&str> = vec!["pull"];
        if rebase {
            args.push("--rebase");
        }
        args.push(&remote);
        if let Some(ref b) = branch {
            args.push(b.as_str());
        }

        match run_git(&args, &ctx.working_dir).await {
            Ok(out) => ToolOutput::success(format!("Pulled from {remote}\n{out}")),
            Err(e) => ToolOutput::error(e),
        }
    }
}
