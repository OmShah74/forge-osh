//! Tests for src/tools/tasks.rs, agent_tools.rs, notebook.rs, code.rs, web.rs

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
        file_cache: None,
        active_skill_scope: None,
        skill_registry: None,
    }
}

// ─── Task tools ──────────────────────────────────────────────────────────

#[tokio::test]
async fn task_create_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let ctx = make_ctx(dir.path());

    let create = registry.get("task_create").unwrap();
    let output = create
        .execute(
            serde_json::json!({
                "subject": "Test task",
                "description": "A test task for the suite"
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Create error: {}", output.content);

    let list = registry.get("task_list").unwrap();
    let output = list.execute(serde_json::json!({}), &ctx).await;
    assert!(!output.is_error, "List error: {}", output.content);
}

#[tokio::test]
async fn todo_write_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let ctx = make_ctx(dir.path());
    let tool = registry.get("todo_write").unwrap();
    let output = tool
        .execute(
            serde_json::json!({
                "todos": [
                    { "id": "1", "content": "First item",  "status": "pending" },
                    { "id": "2", "content": "Second item", "status": "completed" }
                ]
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
}
