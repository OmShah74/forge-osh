use futures::FutureExt;
use std::any::Any;
use std::panic::AssertUnwindSafe;
use tokio_util::sync::CancellationToken;
use tracing::{debug, instrument};

use super::ToolRegistry;
use crate::agent::permissions::{effective_permission, EffectivePermission, PermissionStore};
use crate::types::*;

/// Executes tool calls with permission checking, schema validation, and
/// cancellation support.
pub struct ToolExecutor {
    max_output_chars: usize,
    /// When present, tool inputs are validated against the tool's
    /// `parameters_schema()` before execution. Initialised lazily by the
    /// caller via `Self::new(...)` — validation is always on in production
    /// paths; tests that bypass it can call `Self::new_unvalidated(...)`.
    validate_inputs: bool,
}

impl ToolExecutor {
    pub fn new(max_output_chars: usize) -> Self {
        Self {
            max_output_chars,
            validate_inputs: true,
        }
    }

    /// Skip JSON-schema validation. Primarily used in test fixtures that
    /// synthesise minimal tool inputs.
    pub fn new_unvalidated(max_output_chars: usize) -> Self {
        Self {
            max_output_chars,
            validate_inputs: false,
        }
    }

    /// Execute a tool call.
    ///
    /// The permission decision ordering is:
    ///   1. PermissionMode::Bypass           → Allow
    ///   2. PermissionMode::Plan + !ReadOnly → Deny
    ///   3. ReadOnly tools                   → Allow
    ///   4. PermissionStore rule (allow/deny)→ Allow / Deny
    ///   5. PermissionMode::AcceptEdits + Mutating → Allow
    ///   6. otherwise                        → Ask (prompt via `permission_fn`)
    #[instrument(skip_all, fields(tool = %tool_call.name))]
    pub async fn execute<F, Fut>(
        &self,
        tool_call: &ToolCall,
        ctx: &ToolContext,
        registry: &ToolRegistry,
        store: &PermissionStore,
        cancel: &CancellationToken,
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

        // ── Input validation against parameters_schema ───────────────────────
        if self.validate_inputs {
            if let Err(msg) =
                super::validate::validate_input(&tool.parameters_schema(), &tool_call.input)
            {
                return ToolOutput::error(format!(
                    "Invalid input for tool '{}': {msg}",
                    tool_call.name
                ));
            }
        }

        let perm_level = tool.effective_permission_level(&tool_call.input);

        // ── Permission decision ──────────────────────────────────────────────
        let decision =
            decide_permission(&tool_call.name, &tool_call.input, &perm_level, ctx, store);

        match decision {
            PermissionDecision::Allow => {}
            PermissionDecision::Deny(reason) => {
                return ToolOutput::error(reason);
            }
            PermissionDecision::Ask => {
                let description = format_tool_description(&tool_call.name, &tool_call.input);
                let response = permission_fn(tool_call.name.clone(), description, perm_level).await;

                match response {
                    PermissionResponse::Allow
                    | PermissionResponse::AlwaysAllow
                    | PermissionResponse::TrustMode => {}
                    PermissionResponse::Deny => {
                        return ToolOutput::error(format!(
                            "Permission denied for tool: {}",
                            tool_call.name
                        ));
                    }
                }
            }
        }

        // ── Execute with cancellation race ───────────────────────────────────
        debug!(tool = %tool_call.name, "executing");
        let start = std::time::Instant::now();
        let execute_fut =
            AssertUnwindSafe(tool.execute(tool_call.input.clone(), ctx)).catch_unwind();
        let execute_result = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return ToolOutput::error(format!(
                    "Tool '{}' cancelled by user before completion.",
                    tool_call.name
                ));
            }
            o = execute_fut => o,
        };
        let mut output = match execute_result {
            Ok(output) => output,
            Err(payload) => {
                return ToolOutput::error(format!(
                    "Tool '{}' panicked: {}",
                    tool_call.name,
                    panic_message(payload)
                ));
            }
        };

        // Truncate output if too long
        if self.max_output_chars > 0 && output.content.chars().count() > self.max_output_chars {
            let shown = first_chars(&output.content, self.max_output_chars);
            let total_chars = output.content.chars().count();
            output.content = format!(
                "{}\n\n... [truncated, showing first {} of {} chars]",
                shown, self.max_output_chars, total_chars
            );
        }

        debug!(
            tool = %tool_call.name,
            is_error = output.is_error,
            duration_ms = start.elapsed().as_millis() as u64,
            "tool finished",
        );
        output
    }
}

pub(crate) fn first_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

pub(crate) fn maybe_truncate_chars(s: String, max_chars: Option<usize>) -> String {
    let Some(max_chars) = max_chars.filter(|n| *n > 0) else {
        return s;
    };

    let total_chars = s.chars().count();
    if total_chars <= max_chars {
        return s;
    }

    format!(
        "{}\n\n... [truncated at {} chars, total {}]",
        first_chars(&s, max_chars),
        max_chars,
        total_chars
    )
}

pub(crate) fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

/// Internal permission decision.
enum PermissionDecision {
    Allow,
    Deny(String),
    Ask,
}

fn decide_permission(
    tool_name: &str,
    input: &serde_json::Value,
    level: &PermissionLevel,
    ctx: &ToolContext,
    store: &PermissionStore,
) -> PermissionDecision {
    // 1. Bypass mode: everything allowed.
    if ctx.permission_mode == PermissionMode::Bypass || ctx.trust_mode {
        return PermissionDecision::Allow;
    }

    // 1.5 Skill-scope narrowing: if a skill is active and it declares an
    // allowlist, deny tools outside the declared set before consulting
    // persistent rules or prompting.
    if let Some(scope) = &ctx.active_skill_scope {
        if !scope.allows_tool(tool_name) {
            return PermissionDecision::Deny(format!(
                "Tool '{tool_name}' is not allowed while skill '{}' is active.",
                scope.skill_name
            ));
        }
    }

    // 2. Plan mode: only ReadOnly allowed.
    if ctx.permission_mode == PermissionMode::Plan && *level != PermissionLevel::ReadOnly {
        return PermissionDecision::Deny(format!(
            "Tool '{tool_name}' is {level:?} but plan mode only allows ReadOnly tools. \
             Exit plan mode with `exit_plan_mode` before performing mutations."
        ));
    }

    // 3. ReadOnly tools never prompt.
    if *level == PermissionLevel::ReadOnly {
        return PermissionDecision::Allow;
    }

    // 4. Consult the persistent permission rules store.
    match effective_permission(tool_name, input, level, false, store) {
        EffectivePermission::Allow => PermissionDecision::Allow,
        EffectivePermission::Deny => PermissionDecision::Deny(format!(
            "Tool '{tool_name}' denied by stored permission rule. \
             Run `/permissions` to inspect or edit rules."
        )),
        EffectivePermission::Ask => {
            // 5. AcceptEdits: auto-allow Mutating; prompt for anything harsher.
            if ctx.permission_mode == PermissionMode::AcceptEdits
                && *level == PermissionLevel::Mutating
            {
                return PermissionDecision::Allow;
            }
            PermissionDecision::Ask
        }
    }
}

/// Format a human-readable description of a tool call for the confirmation dialog
pub(crate) fn format_tool_description(name: &str, input: &serde_json::Value) -> String {
    match name {
        "bash" | "powershell" => {
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
            format!(
                "{name}: {}",
                serde_json::to_string_pretty(input).unwrap_or_default()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_description() {
        let desc = format_tool_description("bash", &serde_json::json!({"command": "ls -la"}));
        assert!(desc.contains("ls -la"));

        let desc =
            format_tool_description("delete_file", &serde_json::json!({"path": "/tmp/test.txt"}));
        assert!(desc.contains("/tmp/test.txt"));
    }

    #[test]
    fn test_bypass_always_allows() {
        let store = PermissionStore::default();
        let ctx = ToolContext {
            working_dir: std::path::PathBuf::from("."),
            home_dir: std::path::PathBuf::from("."),
            session_id: "t".into(),
            trust_mode: true,
            permission_mode: PermissionMode::Bypass,
            file_cache: None,
            active_skill_scope: None,
            skill_registry: None,
        };
        match decide_permission(
            "bash",
            &serde_json::json!({"command":"rm -rf /"}),
            &PermissionLevel::Destructive,
            &ctx,
            &store,
        ) {
            PermissionDecision::Allow => {}
            _ => panic!("Bypass should allow"),
        }
    }

    #[test]
    fn test_plan_blocks_mutations() {
        let store = PermissionStore::default();
        let ctx = ToolContext {
            working_dir: std::path::PathBuf::from("."),
            home_dir: std::path::PathBuf::from("."),
            session_id: "t".into(),
            trust_mode: false,
            permission_mode: PermissionMode::Plan,
            file_cache: None,
            active_skill_scope: None,
            skill_registry: None,
        };
        match decide_permission(
            "write_file",
            &serde_json::json!({"path":"/tmp/x"}),
            &PermissionLevel::Mutating,
            &ctx,
            &store,
        ) {
            PermissionDecision::Deny(_) => {}
            _ => panic!("Plan mode must deny mutations"),
        }
    }

    #[test]
    fn test_store_rule_allows_without_prompt() {
        let mut store = PermissionStore::default();
        store
            .rules
            .push(crate::agent::permissions::PermissionRule::new_allow(
                "bash", "git *",
            ));
        let ctx = ToolContext {
            working_dir: std::path::PathBuf::from("."),
            home_dir: std::path::PathBuf::from("."),
            session_id: "t".into(),
            trust_mode: false,
            permission_mode: PermissionMode::Default,
            file_cache: None,
            active_skill_scope: None,
            skill_registry: None,
        };
        match decide_permission(
            "bash",
            &serde_json::json!({"command":"git status"}),
            &PermissionLevel::Shell,
            &ctx,
            &store,
        ) {
            PermissionDecision::Allow => {}
            _ => panic!("Stored allow rule should skip prompt"),
        }
    }

    #[test]
    fn test_unicode_truncation_is_char_safe() {
        let text = "abc↵def".to_string();
        let truncated = maybe_truncate_chars(text, Some(4));
        assert!(truncated.starts_with("abc↵"));
        assert!(truncated.contains("total 7"));
    }

    struct PanickingTool;

    #[async_trait::async_trait]
    impl crate::tools::Tool for PanickingTool {
        fn name(&self) -> &str {
            "panic_tool"
        }

        fn description(&self) -> &str {
            "Panics for executor recovery testing"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::ReadOnly
        }

        async fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolOutput {
            panic!("intentional panic");
        }
    }

    #[tokio::test]
    async fn test_executor_converts_tool_panic_to_error() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(PanickingTool));
        let executor = ToolExecutor::new(0);
        let ctx = ToolContext {
            working_dir: std::path::PathBuf::from("."),
            home_dir: std::path::PathBuf::from("."),
            session_id: "t".into(),
            trust_mode: false,
            permission_mode: PermissionMode::Default,
            file_cache: None,
            active_skill_scope: None,
            skill_registry: None,
        };
        let call = ToolCall {
            id: "panic-1".into(),
            name: "panic_tool".into(),
            input: serde_json::json!({}),
        };
        let output = executor
            .execute(
                &call,
                &ctx,
                &registry,
                &PermissionStore::default(),
                &CancellationToken::new(),
                |_, _, _| async { PermissionResponse::Allow },
            )
            .await;

        assert!(output.is_error);
        assert!(output.content.contains("panicked"));
        assert!(output.content.contains("intentional panic"));
    }
}
