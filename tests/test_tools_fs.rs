//! Tests for src/tools/fs.rs — file system tools

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
    }
}

#[tokio::test]
async fn read_file_tool_reads_existing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hello.txt");
    std::fs::write(&path, "hello world").unwrap();

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("read_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(serde_json::json!({"path": path.to_str().unwrap()}), &ctx)
        .await;
    assert!(!output.is_error);
    assert!(output.content.contains("hello world"));
}

#[tokio::test]
async fn read_file_tool_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("read_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(serde_json::json!({"path": "/nonexistent/file.txt"}), &ctx)
        .await;
    assert!(output.is_error);
}

#[tokio::test]
async fn write_file_tool_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.txt");
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("write_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "content": "test content"
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(path.exists());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "test content");
}

#[tokio::test]
async fn create_file_tool_creates_new() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new_file.txt");
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("create_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "content": "new content"
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(path.exists());
}

#[tokio::test]
async fn list_directory_tool_lists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.rs"), "b").unwrap();

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("list_directory").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({"path": dir.path().to_str().unwrap()}),
            &ctx,
        )
        .await;
    assert!(!output.is_error);
    assert!(output.content.contains("a.txt"));
    assert!(output.content.contains("b.rs"));
}

#[tokio::test]
async fn edit_file_tool_replaces_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit_test.txt");
    std::fs::write(&path, "hello world\nfoo bar\n").unwrap();

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("edit_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "edits": [{"old_str": "hello world", "new_str": "goodbye world"}]
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("goodbye world"));
    assert!(!content.contains("hello world"));
}

#[tokio::test]
async fn copy_file_tool_copies() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("source.txt");
    let dst = dir.path().join("dest.txt");
    std::fs::write(&src, "copy me").unwrap();

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("copy_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "source": src.to_str().unwrap(),
                "destination": dst.to_str().unwrap()
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(dst.exists());
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "copy me");
}

#[tokio::test]
async fn move_file_tool_moves() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("to_move.txt");
    let dst = dir.path().join("moved.txt");
    std::fs::write(&src, "move me").unwrap();

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("move_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "source": src.to_str().unwrap(),
                "destination": dst.to_str().unwrap()
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(!src.exists());
    assert!(dst.exists());
}

#[tokio::test]
async fn delete_file_tool_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("to_delete.txt");
    std::fs::write(&path, "delete me").unwrap();

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("delete_file").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(serde_json::json!({"path": path.to_str().unwrap()}), &ctx)
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(!path.exists());
}

// ─── Permission levels ───────────────────────────────────────────────────

#[test]
fn read_file_is_readonly() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("read_file").unwrap();
    assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
}

#[test]
fn write_file_is_mutating() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("write_file").unwrap();
    assert_eq!(tool.permission_level(), PermissionLevel::Mutating);
}

#[test]
fn delete_file_is_destructive() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("delete_file").unwrap();
    assert_eq!(tool.permission_level(), PermissionLevel::Destructive);
}

#[test]
fn list_directory_is_readonly() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("list_directory").unwrap();
    assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
}
