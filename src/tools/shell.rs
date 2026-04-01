use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::types::*;
use super::Tool;

// ---------------------------------------------------------------------------
// EndTruncatingAccumulator — keeps last N bytes of output (like Claude Code)
// When output is huge, keeps the end (most recent) rather than the beginning.
// ---------------------------------------------------------------------------
struct EndTruncatingAccumulator {
    max_bytes: usize,
    buffer: Vec<u8>,
    total_bytes: usize,
    truncated: bool,
}

impl EndTruncatingAccumulator {
    fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            buffer: Vec::new(),
            total_bytes: 0,
            truncated: false,
        }
    }

    fn push(&mut self, data: &[u8]) {
        self.total_bytes += data.len();

        if self.buffer.len() + data.len() <= self.max_bytes {
            self.buffer.extend_from_slice(data);
        } else {
            // Keep the end: combine what we have + new data, then trim from front
            self.buffer.extend_from_slice(data);
            if self.buffer.len() > self.max_bytes {
                let excess = self.buffer.len() - self.max_bytes;
                self.buffer.drain(..excess);
                self.truncated = true;
            }
        }
    }

    fn finish(self) -> String {
        let s = String::from_utf8_lossy(&self.buffer).to_string();
        if self.truncated {
            format!(
                "[Output truncated — showing last {} of {} total bytes]\n...\n{}",
                self.buffer.len(),
                self.total_bytes,
                s
            )
        } else {
            s
        }
    }
}

// ---------------------------------------------------------------------------
// Dangerous command patterns (blocklist)
// ---------------------------------------------------------------------------

const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    "sudo rm -rf /",
    "mkfs",
    ":(){:|:&};:",          // fork bomb
    "dd if=/dev/zero of=/dev/", // disk wipe
    "chmod -R 777 /",
    "chown -R root /",
    "> /dev/sda",
];

fn is_blocked(command: &str) -> Option<&'static str> {
    for pattern in BLOCKED_PATTERNS {
        if command.contains(pattern) {
            return Some(pattern);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// BashTool
// ---------------------------------------------------------------------------

pub struct BashTool {
    pub default_timeout: u64,
    pub max_timeout: u64,
    pub max_output_bytes: usize,
}

impl Default for BashTool {
    fn default() -> Self {
        Self {
            default_timeout: 30,
            max_timeout: 300,
            max_output_bytes: 200_000, // 200 KB — keeps tail of output
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }

    fn description(&self) -> &str {
        "Execute a bash/shell command. Returns combined stdout and stderr. \
        Commands run in the current working directory. \
        Large outputs are truncated from the front (tail is preserved). \
        Use timeout_seconds to override the per-command timeout (max 300s)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Per-command timeout in seconds (default: 30, max: 300)"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Override the working directory for this command (optional)"
                }
            },
            "required": ["command"]
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Shell }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let command = match input["command"].as_str() {
            Some(c) => c,
            None => return ToolOutput::error("Missing 'command' parameter"),
        };

        // Safety check
        if let Some(blocked) = is_blocked(command) {
            return ToolOutput::error(format!(
                "Command blocked for safety (matches pattern '{blocked}'): {command}"
            ));
        }

        let timeout = input["timeout_seconds"]
            .as_u64()
            .unwrap_or(self.default_timeout)
            .min(self.max_timeout);

        let work_dir = input["working_dir"]
            .as_str()
            .map(|p| {
                let path = Path::new(p);
                if path.is_absolute() { path.to_path_buf() } else { ctx.working_dir.join(path) }
            })
            .unwrap_or_else(|| ctx.working_dir.clone());

        // Choose shell based on OS
        let (shell, flag) = if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let max_output_bytes = self.max_output_bytes;

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            async {
                let mut child = Command::new(shell)
                    .arg(flag)
                    .arg(command)
                    .current_dir(&work_dir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn process: {e}"))?;

                let mut stdout_acc = EndTruncatingAccumulator::new(max_output_bytes / 2);
                let mut stderr_acc = EndTruncatingAccumulator::new(max_output_bytes / 2);

                // Read stdout and stderr
                let mut stdout_buf = Vec::new();
                let mut stderr_buf = Vec::new();

                if let Some(mut out) = child.stdout.take() {
                    out.read_to_end(&mut stdout_buf)
                        .await
                        .map_err(|e| format!("Failed to read stdout: {e}"))?;
                    stdout_acc.push(&stdout_buf);
                }
                if let Some(mut err) = child.stderr.take() {
                    err.read_to_end(&mut stderr_buf)
                        .await
                        .map_err(|e| format!("Failed to read stderr: {e}"))?;
                    stderr_acc.push(&stderr_buf);
                }

                let status = child
                    .wait()
                    .await
                    .map_err(|e| format!("Failed to wait for process: {e}"))?;

                Ok::<(String, String, i32), String>((
                    stdout_acc.finish(),
                    stderr_acc.finish(),
                    status.code().unwrap_or(-1),
                ))
            },
        )
        .await;

        match result {
            Ok(Ok((stdout, stderr, exit_code))) => {
                let mut output = String::new();

                if !stdout.is_empty() {
                    output.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str("[stderr]\n");
                    output.push_str(&stderr);
                }

                // Strip ANSI escape codes for clean display
                let cleaned = strip_ansi_escapes::strip(output.as_bytes());
                let cleaned = String::from_utf8(cleaned).unwrap_or(output);

                if exit_code == 0 {
                    ToolOutput::success(if cleaned.is_empty() {
                        "(command completed successfully with no output)".to_string()
                    } else {
                        cleaned
                    })
                } else {
                    ToolOutput {
                        content: format!("Exit code: {exit_code}\n{cleaned}"),
                        is_error: true,
                        metadata: None,
                    }
                }
            }
            Ok(Err(e)) => ToolOutput::error(e),
            Err(_) => ToolOutput::error(format!(
                "Command timed out after {timeout}s: {command}\n\
                (Use timeout_seconds parameter to extend the timeout)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolContext {
        ToolContext {
            working_dir: std::env::current_dir().unwrap(),
            home_dir: dirs::home_dir().unwrap_or_default(),
            session_id: "test".to_string(),
            trust_mode: true,
        }
    }

    #[tokio::test]
    async fn test_echo() {
        let tool = BashTool::default();
        let ctx = test_ctx();
        let output = tool.execute(json!({"command": "echo hello"}), &ctx).await;
        assert!(!output.is_error);
        assert!(output.content.contains("hello"));
    }

    #[tokio::test]
    async fn test_blocked_command() {
        let tool = BashTool::default();
        let ctx = test_ctx();
        let output = tool.execute(json!({"command": "rm -rf /"}), &ctx).await;
        assert!(output.is_error);
        assert!(output.content.contains("blocked"));
    }

    #[test]
    fn test_end_truncating_accumulator() {
        let mut acc = EndTruncatingAccumulator::new(10);
        acc.push(b"hello world"); // 11 bytes — should truncate
        let result = acc.finish();
        assert!(result.contains("ello world") || result.contains("truncated"));
    }

    #[test]
    fn test_accumulator_no_truncation() {
        let mut acc = EndTruncatingAccumulator::new(100);
        acc.push(b"hello");
        let result = acc.finish();
        assert_eq!(result, "hello");
    }
}
