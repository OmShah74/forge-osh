use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::types::*;
use super::Tool;

pub struct BashTool {
    pub default_timeout: u64,
    pub max_timeout: u64,
    pub blocked_commands: Vec<String>,
}

impl Default for BashTool {
    fn default() -> Self {
        Self {
            default_timeout: 30,
            max_timeout: 300,
            blocked_commands: vec![
                "rm -rf /".to_string(),
                "sudo rm -rf /".to_string(),
                "mkfs".to_string(),
                ":(){:|:&};:".to_string(),
            ],
        }
    }
}

impl BashTool {
    fn is_blocked(&self, command: &str) -> bool {
        self.blocked_commands
            .iter()
            .any(|blocked| command.contains(blocked))
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }

    fn description(&self) -> &str {
        "Execute a bash command. Returns stdout and stderr. Commands run in the current working directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to execute" },
                "timeout_seconds": { "type": "integer", "description": "Timeout in seconds (default: 30, max: 300)" },
                "working_dir": { "type": "string", "description": "Override working directory (optional)" }
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

        if self.is_blocked(command) {
            return ToolOutput::error(format!("Command is blocked for safety: {command}"));
        }

        let timeout = input["timeout_seconds"]
            .as_u64()
            .unwrap_or(self.default_timeout)
            .min(self.max_timeout);

        let work_dir = input["working_dir"]
            .as_str()
            .map(|p| {
                let path = Path::new(p);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    ctx.working_dir.join(path)
                }
            })
            .unwrap_or_else(|| ctx.working_dir.clone());

        // Choose shell based on OS
        let (shell, flag) = if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

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

                let mut stdout = String::new();
                let mut stderr = String::new();

                if let Some(mut out) = child.stdout.take() {
                    out.read_to_string(&mut stdout)
                        .await
                        .map_err(|e| format!("Failed to read stdout: {e}"))?;
                }
                if let Some(mut err) = child.stderr.take() {
                    err.read_to_string(&mut stderr)
                        .await
                        .map_err(|e| format!("Failed to read stderr: {e}"))?;
                }

                let status = child
                    .wait()
                    .await
                    .map_err(|e| format!("Failed to wait for process: {e}"))?;

                Ok::<(String, String, i32), String>((
                    stdout,
                    stderr,
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

                // Strip ANSI codes
                let cleaned = String::from_utf8(
                    strip_ansi_escapes::strip(output.as_bytes())
                ).unwrap_or(output);

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
                "Command timed out after {timeout} seconds: {command}"
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
        let output = tool
            .execute(json!({"command": "echo hello"}), &ctx)
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("hello"));
    }

    #[tokio::test]
    async fn test_blocked_command() {
        let tool = BashTool::default();
        let ctx = test_ctx();
        let output = tool
            .execute(json!({"command": "rm -rf /"}), &ctx)
            .await;
        assert!(output.is_error);
        assert!(output.content.contains("blocked"));
    }
}
