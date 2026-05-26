//! Tests for src/tools/search.rs

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
    }
}

#[tokio::test]
async fn search_files_basic() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("haystack.txt"),
        "needle in a haystack\nother line",
    )
    .unwrap();
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("search_files").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "path": dir.path().to_str().unwrap(),
                "pattern": "needle"
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(output.content.contains("needle"));
}

#[tokio::test]
async fn search_files_no_match() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file.txt"), "hello world").unwrap();
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("search_files").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "path": dir.path().to_str().unwrap(),
                "pattern": "zzzznotfound"
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error);
}

#[tokio::test]
async fn find_files_glob() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.path().join("test.py"), "print('hi')").unwrap();
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("find_files").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(
            serde_json::json!({
                "path": dir.path().to_str().unwrap(),
                "pattern": "*.rs"
            }),
            &ctx,
        )
        .await;
    assert!(!output.is_error);
    assert!(output.content.contains("main.rs"));
}

#[test]
fn search_files_is_readonly() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("search_files").unwrap();
    assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
}

#[test]
fn find_files_is_readonly() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("find_files").unwrap();
    assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
}
