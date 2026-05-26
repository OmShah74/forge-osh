//! Tests for src/tools/notebook.rs

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

#[test]
fn notebook_read_exists() {
    let r = ToolRegistry::with_builtins();
    assert!(r.get("notebook_read").is_some());
}

#[test]
fn notebook_read_is_readonly() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("notebook_read").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
}

#[tokio::test]
async fn notebook_read_valid_notebook() {
    let dir = tempfile::tempdir().unwrap();
    let notebook = serde_json::json!({
        "cells": [
            {
                "cell_type": "code",
                "source": ["print('hello')"],
                "outputs": []
            },
            {
                "cell_type": "markdown",
                "source": ["# Title"],
                "outputs": []
            }
        ],
        "metadata": {},
        "nbformat": 4,
        "nbformat_minor": 2
    });
    let path = dir.path().join("test.ipynb");
    std::fs::write(&path, serde_json::to_string_pretty(&notebook).unwrap()).unwrap();

    let r = ToolRegistry::with_builtins();
    let tool = r.get("notebook_read").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(serde_json::json!({"path": path.to_str().unwrap()}), &ctx)
        .await;
    assert!(!output.is_error, "Error: {}", output.content);
    assert!(output.content.contains("hello") || output.content.contains("print"));
}

#[tokio::test]
async fn notebook_read_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let r = ToolRegistry::with_builtins();
    let tool = r.get("notebook_read").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(serde_json::json!({"path": "/nonexistent.ipynb"}), &ctx)
        .await;
    assert!(output.is_error);
}
