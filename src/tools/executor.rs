use tokio::sync::oneshot;

use crate::error::{ForgeError, Result};
use crate::types::*;
use super::ToolRegistry;

/// Executes tool calls with permission checking
pub struct ToolExecutor {
    max_output_chars: usize,
}

impl ToolExecutor {
    pub fn new(max_output_chars: usize) -> Self {
        Self { max_output_chars }
    }

    /// Execute a tool call. The `permission_fn` is called for tools that need
    /// user confirmation; it should return the user's response.
    pub async fn execute<F, Fut>(
        &self,
        tool_call: &ToolCall,
        ctx: &ToolContext,
        registry: &ToolRegistry,
        permission_fn: F,
    ) -> ToolOutput
    where
        F: FnOnce(String, String, PermissionLevel) -> Fut,
        Fut: std::future::Future<Output = PermissionResponse>,
    {
        let tool = match registry.get(&tool_call.name) {
            Some(t) => t,
            None => {
                return ToolOutput::error(format!("Unknown tool: {}", tool_call.name));
            }
        };

        let perm_level = tool.permission_level();

        // Check if we need permission
        let needs_permission = match &perm_level {
            PermissionLevel::ReadOnly => false,
            _ if ctx.trust_mode => false,
            _ => true,
        };

        if needs_permission {
            let description = format_tool_description(&tool_call.name, &tool_call.input);
            let response = permission_fn(
                tool_call.name.clone(),
                description,
                perm_level,
            )
            .await;

            match response {
                PermissionResponse::Allow | PermissionResponse::AlwaysAllow | PermissionResponse::TrustMode => {}
                PermissionResponse::Deny => {
                    return ToolOutput::error(format!(
                        "Permission denied for tool: {}",
                        tool_call.name
                    ));
                }
            }
        }

        // Execute the tool
        let mut output = tool.execute(tool_call.input.clone(), ctx).await;

        // Truncate output if too long
        if output.content.len() > self.max_output_chars {
            output.content = format!(
                "{}\n\n... [truncated, showing first {} of {} chars]",
                &output.content[..self.max_output_chars],
                self.max_output_chars,
                output.content.len()
            );
        }

        output
    }
}

/// Format a human-readable description of a tool call for the confirmation dialog
fn format_tool_description(name: &str, input: &serde_json::Value) -> String {
    match name {
        "bash" => {
            let cmd = input["command"].as_str().unwrap_or("(no command)");
            format!("Execute command: {cmd}")
        }
        "write_file" | "create_file" => {
            let path = input["path"].as_str().unwrap_or("(unknown)");
            format!("Write to file: {path}")
        }
        "edit_file" => {
            let path = input["path"].as_str().unwrap_or("(unknown)");
            format!("Edit file: {path}")
        }
        "delete_file" => {
            let path = input["path"].as_str().unwrap_or("(unknown)");
            format!("Delete file: {path}")
        }
        "move_file" => {
            let src = input["source"].as_str().unwrap_or("?");
            let dst = input["destination"].as_str().unwrap_or("?");
            format!("Move {src} -> {dst}")
        }
        "git_commit" => {
            let msg = input["message"].as_str().unwrap_or("(no message)");
            format!("Git commit: {msg}")
        }
        "git_checkout" => {
            let branch = input["branch"].as_str().unwrap_or("?");
            format!("Git checkout: {branch}")
        }
        "web_fetch" => {
            let url = input["url"].as_str().unwrap_or("?");
            format!("Fetch URL: {url}")
        }
        _ => {
            format!("{name}: {}", serde_json::to_string_pretty(input).unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_description() {
        let desc = format_tool_description(
            "bash",
            &serde_json::json!({"command": "ls -la"}),
        );
        assert!(desc.contains("ls -la"));

        let desc = format_tool_description(
            "delete_file",
            &serde_json::json!({"path": "/tmp/test.txt"}),
        );
        assert!(desc.contains("/tmp/test.txt"));
    }
}
