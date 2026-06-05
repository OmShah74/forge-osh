//! Tests for src/tools/shell.rs — bash tool

use forge_agent::tools::Tool;
use forge_agent::tools::ToolRegistry;
use forge_agent::types::*;

fn make_ctx(dir: &std::path::Path) -> ToolContext {
    ToolContext {
        working_dir: dir.to_path_buf(),
        home_dir: dir.to_path_buf(),
        session_id: "test".into(),
        trust_mode: true,
        permission_mode: forge_agent::types::PermissionMode::Default,
        diff_review: true,
        file_cache: None,
        active_skill_scope: None,
        skill_registry: None,
        output_chunk_tx: None,
        tool_call_id: None,
        team_blackboard: None,
    }
}

#[tokio::test]
async fn bash_echo() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("bash").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(serde_json::json!({"command": "echo hello_test"}), &ctx)
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(output.content.contains("hello_test"));
}

#[tokio::test]
async fn bash_missing_command_field() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("bash").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool.execute(serde_json::json!({}), &ctx).await;
    assert!(output.is_error);
}

#[test]
fn bash_readonly_commands_are_readonly() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("bash").unwrap();
    // ls should be ReadOnly
    let level = tool.effective_permission_level(&serde_json::json!({"command": "ls -la"}));
    assert_eq!(level, PermissionLevel::ReadOnly);
}

#[test]
fn bash_git_log_is_readonly() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("bash").unwrap();
    let level =
        tool.effective_permission_level(&serde_json::json!({"command": "git log --oneline"}));
    assert_eq!(level, PermissionLevel::ReadOnly);
}

#[test]
fn bash_mutating_commands_are_shell() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("bash").unwrap();
    let level = tool.effective_permission_level(&serde_json::json!({"command": "npm install"}));
    assert_eq!(level, PermissionLevel::Shell);
}
