//! Hooks system — shell commands that fire at specific agent lifecycle events.
//!
//! Config format (~/.forge-osh/hooks.json):
//! {
//!   "PreToolUse":       [ { "matcher": "bash", "command": "…", "blocking": true } ],
//!   "PostToolUse":      [ { "matcher": "*",    "command": "…" } ],
//!   "UserPromptSubmit": [ { "command": "…" } ],
//!   "SessionStart":     [ { "command": "…" } ],
//!   "SessionEnd":       [ { "command": "…" } ],
//!   "PreCompact":       [ { "command": "…" } ],
//!   "Stop":             [ { "command": "…" } ],
//!   "Notification":     [ { "command": "…" } ]
//! }
//!
//! Environment variables set when hooks run:
//!   TOOL_NAME      — name of the tool (Pre/PostToolUse only)
//!   TOOL_INPUT     — JSON-serialized tool input (Pre/PostToolUse only)
//!   TOOL_OUTPUT    — tool output (PostToolUse only)
//!   IS_ERROR       — "1" if tool errored (PostToolUse only)
//!   USER_PROMPT    — raw user prompt text (UserPromptSubmit only)
//!   SESSION_ID     — current session UUID (SessionStart/End, PreCompact)
//!
//! A `PreToolUse` hook with `"blocking": true` that exits non-zero VETOES the
//! tool call — its stderr is surfaced to the model as the tool result so the
//! agent can adapt. Non-blocking hooks (the default) are always fire-and-forget.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::config::config_dir;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    /// Tool name or glob pattern to match. "*" matches all tools.
    #[serde(default = "default_matcher")]
    pub matcher: String,
    /// Shell command to execute
    pub command: String,
    /// Timeout in seconds (default: 10)
    #[serde(default = "default_hook_timeout")]
    pub timeout_seconds: u64,
    /// If true, a non-zero exit on a PreToolUse hook cancels the tool call.
    /// For non-PreToolUse events this field is ignored.
    #[serde(default)]
    pub blocking: bool,
}

fn default_matcher() -> String { "*".to_string() }
fn default_hook_timeout() -> u64 { 10 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(rename = "PreToolUse", default)]
    pub pre_tool_use: Vec<HookEntry>,
    #[serde(rename = "PostToolUse", default)]
    pub post_tool_use: Vec<HookEntry>,
    #[serde(rename = "Stop", default)]
    pub stop: Vec<HookEntry>,
    #[serde(rename = "Notification", default)]
    pub notification: Vec<HookEntry>,
    #[serde(rename = "UserPromptSubmit", default)]
    pub user_prompt_submit: Vec<HookEntry>,
    #[serde(rename = "SessionStart", default)]
    pub session_start: Vec<HookEntry>,
    #[serde(rename = "SessionEnd", default)]
    pub session_end: Vec<HookEntry>,
    #[serde(rename = "PreCompact", default)]
    pub pre_compact: Vec<HookEntry>,
}

impl HooksConfig {
    fn storage_path() -> PathBuf {
        config_dir().join("hooks.json")
    }

    /// Load hooks from disk
    pub fn load() -> Self {
        let path = Self::storage_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
        Self::default()
    }

    /// Save to disk
    pub fn save(&self) {
        let path = Self::storage_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Check if there are any hooks configured
    pub fn is_empty(&self) -> bool {
        self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
            && self.stop.is_empty()
            && self.notification.is_empty()
            && self.user_prompt_submit.is_empty()
            && self.session_start.is_empty()
            && self.session_end.is_empty()
            && self.pre_compact.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Hook executor
// ---------------------------------------------------------------------------

/// Context passed to each hook execution
pub struct HookContext {
    pub tool_name: String,
    pub tool_input: String,
    pub tool_output: Option<String>,
    pub is_error: Option<bool>,
    pub user_prompt: Option<String>,
    pub session_id: Option<String>,
    pub working_dir: PathBuf,
}

/// Result of running a hook.
#[derive(Debug, Clone, Default)]
pub struct HookResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub spawn_failed: bool,
}

impl HookResult {
    pub fn succeeded(&self) -> bool {
        !self.timed_out && !self.spawn_failed && self.exit_code == Some(0)
    }
}

/// Execute a single hook, capturing stdout/stderr and honouring the timeout.
async fn run_hook(hook: &HookEntry, ctx: &HookContext) -> HookResult {
    let (shell, flag) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let mut cmd = Command::new(shell);
    cmd.arg(flag)
        .arg(&hook.command)
        .current_dir(&ctx.working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .env("TOOL_NAME", &ctx.tool_name)
        .env("TOOL_INPUT", &ctx.tool_input);

    if let Some(ref out) = ctx.tool_output {
        cmd.env("TOOL_OUTPUT", out);
    }
    if let Some(is_err) = ctx.is_error {
        cmd.env("IS_ERROR", if is_err { "1" } else { "0" });
    }
    if let Some(ref prompt) = ctx.user_prompt {
        cmd.env("USER_PROMPT", prompt);
    }
    if let Some(ref sid) = ctx.session_id {
        cmd.env("SESSION_ID", sid);
    }

    let timeout = std::time::Duration::from_secs(hook.timeout_seconds.max(1));

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            return HookResult { spawn_failed: true, ..Default::default() };
        }
    };

    // Read stdout+stderr concurrently
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let stdout_reader = child.stdout.take();
    let stderr_reader = child.stderr.take();

    let collect_task = async {
        if let Some(mut so) = stdout_reader {
            let _ = so.read_to_end(&mut stdout_buf).await;
        }
        if let Some(mut se) = stderr_reader {
            let _ = se.read_to_end(&mut stderr_buf).await;
        }
    };

    let wait_result = tokio::time::timeout(timeout, async {
        collect_task.await;
        child.wait().await
    })
    .await;

    match wait_result {
        Err(_) => {
            // Timed out — kill the child and report
            let _ = child.kill().await;
            HookResult {
                timed_out: true,
                stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
                stderr: String::from_utf8_lossy(&stderr_buf).into_owned(),
                ..Default::default()
            }
        }
        Ok(Ok(status)) => HookResult {
            exit_code: status.code(),
            stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_buf).into_owned(),
            ..Default::default()
        },
        Ok(Err(_)) => HookResult {
            spawn_failed: true,
            stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_buf).into_owned(),
            ..Default::default()
        },
    }
}

fn matches(hook: &HookEntry, tool_name: &str) -> bool {
    if hook.matcher == "*" || hook.matcher == tool_name {
        return true;
    }
    glob::Pattern::new(&hook.matcher)
        .map(|p| p.matches(tool_name))
        .unwrap_or(false)
}

/// Run all matching hooks of an event for fire-and-forget semantics.
/// Results are returned for optional logging; callers typically discard them.
pub async fn run_hooks(hooks: &[HookEntry], ctx: &HookContext) -> Vec<HookResult> {
    let mut out = Vec::with_capacity(hooks.len());
    for hook in hooks {
        if !matches(hook, &ctx.tool_name) { continue; }
        out.push(run_hook(hook, ctx).await);
    }
    out
}

// ---------------------------------------------------------------------------
// Event wrappers
// ---------------------------------------------------------------------------

/// Outcome of the PreToolUse event.
#[derive(Debug, Clone)]
pub enum PreToolOutcome {
    /// No blocking hook vetoed — proceed with the tool call.
    Proceed,
    /// A blocking PreToolUse hook exited non-zero. `reason` is the hook's
    /// stderr (or a synthetic message) and should be surfaced as the tool
    /// result so the LLM can adapt instead of retrying blindly.
    Veto { reason: String, hook: String },
}

/// Run PreToolUse hooks. Any `blocking: true` hook that exits non-zero
/// returns `Veto`. Non-blocking hooks are fired but never cancel.
pub async fn pre_tool_use(
    config: &HooksConfig,
    tool_name: &str,
    input: &serde_json::Value,
    working_dir: PathBuf,
    session_id: Option<String>,
) -> PreToolOutcome {
    if config.pre_tool_use.is_empty() {
        return PreToolOutcome::Proceed;
    }
    let ctx = HookContext {
        tool_name: tool_name.to_string(),
        tool_input: serde_json::to_string(input).unwrap_or_default(),
        tool_output: None,
        is_error: None,
        user_prompt: None,
        session_id,
        working_dir,
    };
    for hook in &config.pre_tool_use {
        if !matches(hook, tool_name) { continue; }
        let result = run_hook(hook, &ctx).await;
        if hook.blocking && !result.succeeded() {
            let reason = if !result.stderr.trim().is_empty() {
                result.stderr.trim().to_string()
            } else if result.timed_out {
                format!("hook timed out after {}s", hook.timeout_seconds)
            } else if result.spawn_failed {
                "hook could not be launched".to_string()
            } else {
                format!("hook exited with code {:?}", result.exit_code)
            };
            return PreToolOutcome::Veto { reason, hook: hook.command.clone() };
        }
    }
    PreToolOutcome::Proceed
}

/// Run PostToolUse hooks (fire-and-forget; results discarded).
pub async fn post_tool_use(
    config: &HooksConfig,
    tool_name: &str,
    input: &serde_json::Value,
    output: &str,
    is_error: bool,
    working_dir: PathBuf,
    session_id: Option<String>,
) {
    if config.post_tool_use.is_empty() { return; }
    let ctx = HookContext {
        tool_name: tool_name.to_string(),
        tool_input: serde_json::to_string(input).unwrap_or_default(),
        tool_output: Some(output.to_string()),
        is_error: Some(is_error),
        user_prompt: None,
        session_id,
        working_dir,
    };
    let _ = run_hooks(&config.post_tool_use, &ctx).await;
}

/// Run Stop hooks (called when agent loop finishes)
pub async fn run_stop_hooks(config: &HooksConfig, working_dir: PathBuf, session_id: Option<String>) {
    if config.stop.is_empty() { return; }
    let ctx = HookContext {
        tool_name: String::new(),
        tool_input: String::new(),
        tool_output: None,
        is_error: None,
        user_prompt: None,
        session_id,
        working_dir,
    };
    let _ = run_hooks(&config.stop, &ctx).await;
}

/// Run UserPromptSubmit hooks. Any blocking hook that exits non-zero cancels
/// the prompt and returns the stderr as the reason; the caller should surface
/// it to the user and skip sending the message. Non-blocking hooks are
/// fire-and-forget.
pub async fn user_prompt_submit(
    config: &HooksConfig,
    prompt: &str,
    working_dir: PathBuf,
    session_id: Option<String>,
) -> Result<(), String> {
    if config.user_prompt_submit.is_empty() { return Ok(()); }
    let ctx = HookContext {
        tool_name: String::new(),
        tool_input: String::new(),
        tool_output: None,
        is_error: None,
        user_prompt: Some(prompt.to_string()),
        session_id,
        working_dir,
    };
    for hook in &config.user_prompt_submit {
        if !matches(hook, "") && hook.matcher != "*" { continue; }
        let result = run_hook(hook, &ctx).await;
        if hook.blocking && !result.succeeded() {
            let reason = if !result.stderr.trim().is_empty() {
                result.stderr.trim().to_string()
            } else if result.timed_out {
                format!("UserPromptSubmit hook timed out after {}s", hook.timeout_seconds)
            } else {
                format!("UserPromptSubmit hook exited with code {:?}", result.exit_code)
            };
            return Err(reason);
        }
    }
    Ok(())
}

/// Run SessionStart hooks (when a session is loaded/created).
pub async fn session_start(config: &HooksConfig, working_dir: PathBuf, session_id: String) {
    if config.session_start.is_empty() { return; }
    let ctx = HookContext {
        tool_name: String::new(),
        tool_input: String::new(),
        tool_output: None,
        is_error: None,
        user_prompt: None,
        session_id: Some(session_id),
        working_dir,
    };
    let _ = run_hooks(&config.session_start, &ctx).await;
}

/// Run SessionEnd hooks (when the TUI exits / session is saved).
pub async fn session_end(config: &HooksConfig, working_dir: PathBuf, session_id: String) {
    if config.session_end.is_empty() { return; }
    let ctx = HookContext {
        tool_name: String::new(),
        tool_input: String::new(),
        tool_output: None,
        is_error: None,
        user_prompt: None,
        session_id: Some(session_id),
        working_dir,
    };
    let _ = run_hooks(&config.session_end, &ctx).await;
}

/// Run PreCompact hooks (before context compaction).
pub async fn pre_compact(config: &HooksConfig, working_dir: PathBuf, session_id: Option<String>) {
    if config.pre_compact.is_empty() { return; }
    let ctx = HookContext {
        tool_name: String::new(),
        tool_input: String::new(),
        tool_output: None,
        is_error: None,
        user_prompt: None,
        session_id,
        working_dir,
    };
    let _ = run_hooks(&config.pre_compact, &ctx).await;
}
