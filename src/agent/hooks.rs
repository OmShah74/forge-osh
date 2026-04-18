//! Hooks system — shell commands that fire at specific agent lifecycle events.
//!
//! Config format (~/.forge-osh/hooks.json):
//! {
//!   "PreToolUse": [
//!     { "matcher": "bash", "command": "echo 'About to run bash: $TOOL_INPUT'" }
//!   ],
//!   "PostToolUse": [
//!     { "matcher": "*", "command": "echo 'Tool $TOOL_NAME done'" }
//!   ],
//!   "Stop": [
//!     { "command": "notify-send 'forge-osh done'" }
//!   ]
//! }
//!
//! Environment variables set when hooks run:
//!   TOOL_NAME   — name of the tool (e.g. "bash")
//!   TOOL_INPUT  — JSON-serialized tool input
//!   TOOL_OUTPUT — tool output (PostToolUse only)
//!   IS_ERROR    — "1" if tool errored (PostToolUse only)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
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
    pub working_dir: PathBuf,
}

/// Run all matching hooks of a given event type, fire-and-forget style
pub async fn run_hooks(hooks: &[HookEntry], ctx: &HookContext) {
    for hook in hooks {
        // Check if this hook matches the tool
        if hook.matcher != "*" && hook.matcher != ctx.tool_name {
            // Simple glob match
            if let Ok(p) = glob::Pattern::new(&hook.matcher) {
                if !p.matches(&ctx.tool_name) {
                    continue;
                }
            } else {
                continue;
            }
        }

        run_hook(hook, ctx).await;
    }
}

async fn run_hook(hook: &HookEntry, ctx: &HookContext) {
    let (shell, flag) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let timeout = std::time::Duration::from_secs(hook.timeout_seconds);

    let mut cmd = Command::new(shell);
    cmd.arg(flag)
        .arg(&hook.command)
        .current_dir(&ctx.working_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("TOOL_NAME", &ctx.tool_name)
        .env("TOOL_INPUT", &ctx.tool_input);

    if let Some(ref out) = ctx.tool_output {
        cmd.env("TOOL_OUTPUT", out);
    }
    if let Some(is_err) = ctx.is_error {
        cmd.env("IS_ERROR", if is_err { "1" } else { "0" });
    }

    // Fire and forget with timeout
    if let Ok(mut child) = cmd.spawn() {
        let _ = tokio::time::timeout(timeout, child.wait()).await;
    }
}

/// Run PreToolUse hooks
pub async fn pre_tool_use(config: &HooksConfig, tool_name: &str, input: &serde_json::Value, working_dir: PathBuf) {
    if config.pre_tool_use.is_empty() { return; }
    let ctx = HookContext {
        tool_name: tool_name.to_string(),
        tool_input: serde_json::to_string(input).unwrap_or_default(),
        tool_output: None,
        is_error: None,
        working_dir,
    };
    run_hooks(&config.pre_tool_use, &ctx).await;
}

/// Run PostToolUse hooks
pub async fn post_tool_use(
    config: &HooksConfig,
    tool_name: &str,
    input: &serde_json::Value,
    output: &str,
    is_error: bool,
    working_dir: PathBuf,
) {
    if config.post_tool_use.is_empty() { return; }
    let ctx = HookContext {
        tool_name: tool_name.to_string(),
        tool_input: serde_json::to_string(input).unwrap_or_default(),
        tool_output: Some(output.to_string()),
        is_error: Some(is_error),
        working_dir,
    };
    run_hooks(&config.post_tool_use, &ctx).await;
}

/// Run Stop hooks (called when agent loop finishes)
pub async fn run_stop_hooks(config: &HooksConfig, working_dir: PathBuf) {
    if config.stop.is_empty() { return; }
    let ctx = HookContext {
        tool_name: String::new(),
        tool_input: String::new(),
        tool_output: None,
        is_error: None,
        working_dir,
    };
    run_hooks(&config.stop, &ctx).await;
}
