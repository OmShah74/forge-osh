//! PowerShellTool — execute PowerShell commands on Windows.
//! On non-Windows platforms this tool reports that PowerShell is unavailable.
//! Mirrors BashTool's design: output truncation, timeout, safety blocklist.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::Tool;
use crate::types::*;

fn strip_copied_prompt_marker(command: &str) -> &str {
    let trimmed = command.trim_start();
    for marker in ["PS> ", "PS ", "$ ", "> "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    trimmed
}

// ---------------------------------------------------------------------------
// Dangerous PowerShell command patterns
// ---------------------------------------------------------------------------

const BLOCKED_PS_PATTERNS: &[&str] = &[
    "Remove-Item -Recurse -Force /",
    "Remove-Item -Recurse -Force C:\\",
    "Format-Volume",
    "Clear-Disk",
    "Remove-Partition",
    "Set-ExecutionPolicy Unrestricted",
    "Invoke-Expression",
    "IEX",
];

fn is_blocked_ps(command: &str) -> Option<&'static str> {
    let lower = command.to_lowercase();
    for pattern in BLOCKED_PS_PATTERNS {
        if lower.contains(&pattern.to_lowercase()) {
            return Some(pattern);
        }
    }
    None
}

/// PowerShell read-only cmdlets and commands
const PS_READ_ONLY_CMDLETS: &[&str] = &[
    "get-",
    "select-",
    "where-",
    "format-",
    "measure-",
    "test-path",
    "test-connection",
    "resolve-path",
    "get-help",
    "get-command",
    "get-module",
    "get-childitem",
    "get-content",
    "get-item",
    "get-itemproperty",
    "get-process",
    "get-service",
    "get-eventlog",
    "get-date",
    "get-location",
    "get-variable",
    "get-env",
    "dir",
    "ls",
    "cat",
    "echo",
    "write-host",
    "write-output",
];

pub fn is_read_only_ps_command(command: &str) -> bool {
    let lower = strip_copied_prompt_marker(command).trim().to_lowercase();
    if lower.contains('>')
        || lower.contains('<')
        || lower.contains('|')
        || lower.contains(';')
        || lower.contains(" set-content")
        || lower.contains(" add-content")
        || lower.contains(" out-file")
        || lower.contains("remove-")
        || lower.contains("new-")
        || lower.contains("set-")
        || lower.contains("copy-")
        || lower.contains("move-")
        || lower.contains("rename-")
        || lower.contains("start-process")
        || lower.contains("invoke-expression")
        || lower.contains(" iex ")
    {
        return false;
    }
    PS_READ_ONLY_CMDLETS
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

// ---------------------------------------------------------------------------
// PowerShellTool
// ---------------------------------------------------------------------------

pub struct PowerShellTool {
    pub default_timeout: u64,
    pub max_timeout: u64,
    pub max_output_bytes: usize,
}

impl Default for PowerShellTool {
    fn default() -> Self {
        Self {
            default_timeout: 30,
            max_timeout: 300,
            max_output_bytes: 200_000,
        }
    }
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        "powershell"
    }

    fn description(&self) -> &str {
        "Execute a PowerShell command (Windows). Returns combined stdout and stderr. \
        Use this for Windows-specific operations like registry access, WMI queries, \
        .NET operations, or when PowerShell syntax is preferred over bash. \
        Read-only Get-* cmdlets do not require permission prompts."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The PowerShell command or script to execute"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Per-command timeout in seconds (default: 30, max: 300)"
                }
            },
            "required": ["command"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Shell
    }

    fn effective_permission_level(&self, input: &Value) -> PermissionLevel {
        if let Some(cmd) = input["command"].as_str() {
            if is_read_only_ps_command(cmd) {
                return PermissionLevel::ReadOnly;
            }
        }
        PermissionLevel::Shell
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let command = match input["command"].as_str() {
            Some(c) => c,
            None => return ToolOutput::error("Missing 'command' parameter"),
        };
        let command = strip_copied_prompt_marker(command);

        // Safety check
        if let Some(blocked) = is_blocked_ps(command) {
            return ToolOutput::error(format!(
                "PowerShell command blocked for safety (matches pattern '{blocked}'): {command}"
            ));
        }

        let timeout = input["timeout_seconds"]
            .as_u64()
            .unwrap_or(self.default_timeout)
            .min(self.max_timeout);

        #[cfg(not(target_os = "windows"))]
        {
            // On non-Windows, try pwsh (PowerShell Core) if available
            let ps_exe = which::which("pwsh")
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            if ps_exe.is_empty() {
                return ToolOutput::error(
                    "PowerShell is not available on this platform. \
                    Install PowerShell Core (pwsh) to use this tool, \
                    or use the 'bash' tool instead."
                        .to_string(),
                );
            }
        }

        let max_output_bytes = self.max_output_bytes;
        let work_dir = ctx.working_dir.clone();

        #[cfg(target_os = "windows")]
        let (ps_prog, ps_args_prefix): (&str, Vec<&str>) = (
            "powershell.exe",
            vec!["-NoProfile", "-NonInteractive", "-Command"],
        );

        #[cfg(not(target_os = "windows"))]
        let (ps_prog, ps_args_prefix): (&str, Vec<&str>) =
            ("pwsh", vec!["-NoProfile", "-NonInteractive", "-Command"]);

        let result = tokio::time::timeout(std::time::Duration::from_secs(timeout), async {
            let work_dir_path = Path::new(&work_dir);

            let mut cmd = Command::new(ps_prog);
            cmd.args(&ps_args_prefix)
                .arg(command)
                .current_dir(work_dir_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let mut child = cmd
                .spawn()
                .map_err(|e| format!("Failed to spawn PowerShell: {e}"))?;

            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            if let Some(mut out) = child.stdout.take() {
                out.read_to_end(&mut stdout_buf)
                    .await
                    .map_err(|e| format!("Failed to read stdout: {e}"))?;
            }
            if let Some(mut err) = child.stderr.take() {
                err.read_to_end(&mut stderr_buf)
                    .await
                    .map_err(|e| format!("Failed to read stderr: {e}"))?;
            }

            let status = child
                .wait()
                .await
                .map_err(|e| format!("Failed to wait for process: {e}"))?;

            // Truncate if too long (keep tail)
            let trim_tail = |buf: Vec<u8>| -> String {
                if buf.len() > max_output_bytes / 2 {
                    let start = buf.len() - max_output_bytes / 2;
                    let s = String::from_utf8_lossy(&buf[start..]).to_string();
                    format!("[...truncated...]\n{s}")
                } else {
                    String::from_utf8_lossy(&buf).to_string()
                }
            };

            let stdout = trim_tail(stdout_buf);
            let stderr = trim_tail(stderr_buf);
            let exit_code = status.code().unwrap_or(-1);

            Ok::<(String, String, i32), String>((stdout, stderr, exit_code))
        })
        .await;

        match result {
            Ok(Ok((stdout, stderr, exit_code))) => {
                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str("[stderr]\n");
                    combined.push_str(&stderr);
                }
                let cleaned = strip_ansi_escapes::strip(combined.as_bytes());
                let cleaned = String::from_utf8(cleaned).unwrap_or(combined);

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
                "PowerShell command timed out after {timeout}s: {command}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_prompt_marker_is_stripped_but_variables_remain() {
        assert_eq!(
            strip_copied_prompt_marker("$ $lines=Get-Content src\\tools\\search.rs"),
            "$lines=Get-Content src\\tools\\search.rs"
        );
        assert_eq!(
            strip_copied_prompt_marker("PS> Get-Content README.md"),
            "Get-Content README.md"
        );
        assert_eq!(
            strip_copied_prompt_marker("$lines=Get-Content README.md"),
            "$lines=Get-Content README.md"
        );
    }
}
