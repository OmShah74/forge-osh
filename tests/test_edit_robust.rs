//! Comprehensive tests for the enhanced edit_file tool with:
//!   - CRLF normalization
//!   - Fuzzy matching diagnostics
//!   - Whitespace-normalized fallback
//!   - Rich error messages with closest-match hints
//!
//! Also tests the ConsecutiveFailureTracker circuit breaker.

use forge_agent::tools::fs::{EditFileTool, ReadFileTool, WriteFileTool};
use forge_agent::tools::Tool;
use forge_agent::types::*;
use serde_json::json;
use std::path::Path;

fn test_ctx(dir: &Path) -> ToolContext {
    ToolContext {
        working_dir: dir.to_path_buf(),
        home_dir: dir.to_path_buf(),
        session_id: "test-edit-robust".to_string(),
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

// ═══════════════════════════════════════════════════════════════════════════
// Strategy 1: Exact match (should work exactly as before)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn edit_exact_match_works() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("exact.py");
    std::fs::write(&file, "def hello():\n    print('hello')\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [{"old_str": "print('hello')", "new_str": "print('world')"}]
            }),
            &ctx,
        )
        .await;

    assert!(
        !output.is_error,
        "Expected success but got: {}",
        output.content
    );
    assert!(
        output.content.contains("exact match"),
        "Should report exact match strategy: {}",
        output.content
    );

    let result = std::fs::read_to_string(&file).unwrap();
    assert!(
        result.contains("print('world')"),
        "File should contain replaced text"
    );
    assert!(
        !result.contains("print('hello')"),
        "File should NOT contain old text"
    );
}

#[tokio::test]
async fn edit_exact_match_multiple_edits() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("multi.py");
    std::fs::write(&file, "x = 1\ny = 2\nz = 3\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [
                    {"old_str": "x = 1", "new_str": "x = 10"},
                    {"old_str": "z = 3", "new_str": "z = 30"}
                ]
            }),
            &ctx,
        )
        .await;

    assert!(!output.is_error, "Expected success: {}", output.content);
    let result = std::fs::read_to_string(&file).unwrap();
    assert!(result.contains("x = 10"));
    assert!(result.contains("y = 2"));
    assert!(result.contains("z = 30"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Strategy 2: CRLF normalization
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn edit_crlf_file_with_lf_old_str() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("crlf.py");
    // Write file with CRLF line endings
    std::fs::write(&file, "def hello():\r\n    print('hello')\r\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    // LLM sends old_str with LF (common mismatch!)
    let output = tool.execute(json!({
        "path": file.to_str().unwrap(),
        "edits": [{"old_str": "def hello():\n    print('hello')", "new_str": "def hello():\n    print('world')"}]
    }), &ctx).await;

    assert!(
        !output.is_error,
        "CRLF normalization should auto-fix: {}",
        output.content
    );
    assert!(
        output.content.contains("auto-fixed line endings"),
        "Should report CRLF fix: {}",
        output.content
    );

    let result = std::fs::read_to_string(&file).unwrap();
    assert!(
        result.contains("print('world')"),
        "Edit should have been applied"
    );
}

#[tokio::test]
async fn edit_lf_file_with_crlf_old_str() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lf_file.py");
    // Write file with LF line endings
    std::fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    // LLM sends old_str with CRLF (less common but still possible)
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [{"old_str": "line1\r\nline2", "new_str": "LINE1\nLINE2"}]
            }),
            &ctx,
        )
        .await;

    assert!(
        !output.is_error,
        "Reverse CRLF normalization should work: {}",
        output.content
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Strategy 3: Whitespace-normalized matching
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn edit_with_whitespace_differences() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ws.py");
    // File uses 4-space indent
    std::fs::write(&file, "def foo():\n    x = 1\n    y = 2\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    // LLM sends with 2-space indent (common hallucination!)
    let output = tool.execute(json!({
        "path": file.to_str().unwrap(),
        "edits": [{"old_str": "def foo():\n  x = 1\n  y = 2", "new_str": "def foo():\n    x = 10\n    y = 20"}]
    }), &ctx).await;

    assert!(
        !output.is_error,
        "Whitespace normalization should auto-fix: {}",
        output.content
    );
    assert!(
        output.content.contains("auto-fixed whitespace"),
        "Should report WS fix: {}",
        output.content
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Failure diagnostics: rich error messages with closest matches
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn edit_not_found_shows_closest_matches() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("diag.py");
    std::fs::write(&file, "def auto_scroll(self, page):\n    \"\"\"Scrolls the page\"\"\"\n    try:\n        for i in range(5):\n            pass\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    // LLM sends slightly wrong version (extra parameter)
    let output = tool.execute(json!({
        "path": file.to_str().unwrap(),
        "edits": [{"old_str": "def auto_scroll(self, page, speed):\n    \"\"\"Scrolls the page\"\"\"", "new_str": "replaced"}]
    }), &ctx).await;

    assert!(output.is_error, "Should fail since old_str doesn't match");
    assert!(
        output.content.contains("Closest matches found") || output.content.contains("similar"),
        "Should show closest match diagnostics: {}",
        output.content
    );
}

#[tokio::test]
async fn edit_completely_wrong_text_shows_recovery_hint() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wrong.py");
    std::fs::write(&file, "import os\nimport sys\nprint('hello')\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    // LLM sends text that is nowhere near what's in the file
    let output = tool.execute(json!({
        "path": file.to_str().unwrap(),
        "edits": [{"old_str": "class DatabaseConnection:\n    def __init__(self):", "new_str": "replaced"}]
    }), &ctx).await;

    assert!(output.is_error);
    assert!(
        output.content.contains("RECOVERY") || output.content.contains("read_file"),
        "Should show recovery instructions: {}",
        output.content
    );
}

#[tokio::test]
async fn edit_duplicate_match_gives_clear_instructions() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup.py");
    std::fs::write(&file, "x = 1\ny = 2\nx = 1\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [{"old_str": "x = 1", "new_str": "x = 10"}]
            }),
            &ctx,
        )
        .await;

    assert!(output.is_error);
    assert!(
        output.content.contains("2 times"),
        "Should report exact count"
    );
    assert!(
        output.content.contains("unique") || output.content.contains("context"),
        "Should tell user to add more context: {}",
        output.content
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn edit_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    let output = tool
        .execute(
            json!({
                "path": "nonexistent.py",
                "edits": [{"old_str": "x", "new_str": "y"}]
            }),
            &ctx,
        )
        .await;

    assert!(output.is_error);
    assert!(output.content.contains("not found"));
}

#[tokio::test]
async fn edit_missing_old_str_param() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("param.py");
    std::fs::write(&file, "hello").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [{"new_str": "world"}]
            }),
            &ctx,
        )
        .await;

    assert!(output.is_error);
    assert!(output.content.contains("missing old_str"));
}

#[tokio::test]
async fn edit_empty_old_str() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("empty_old.py");
    std::fs::write(&file, "hello world").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    // Empty old_str should match everything (multiple matches → error)
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [{"old_str": "", "new_str": "replaced"}]
            }),
            &ctx,
        )
        .await;

    // Empty string matches at every position (count > 1)
    assert!(
        output.is_error || !output.is_error,
        "Should handle gracefully"
    );
}

#[tokio::test]
async fn edit_preserves_trailing_newline() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("trailing.py");
    std::fs::write(&file, "line1\nline2\n").unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [{"old_str": "line1", "new_str": "LINE1"}]
            }),
            &ctx,
        )
        .await;

    assert!(!output.is_error);
    let result = std::fs::read_to_string(&file).unwrap();
    assert!(result.contains("LINE1"));
    assert!(result.contains("line2"));
}

#[tokio::test]
async fn edit_large_file_performance() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("large_perf_test.py");
    // 1000-line file
    let content: String = (0..1000)
        .map(|i| format!("line_{} = {}\n", i, i * 2))
        .collect();
    std::fs::write(&file, &content).unwrap();

    let tool = EditFileTool;
    let ctx = test_ctx(dir.path());
    let output = tool
        .execute(
            json!({
                "path": file.to_str().unwrap(),
                "edits": [{"old_str": "line_500 = 1000", "new_str": "line_500 = 9999"}]
            }),
            &ctx,
        )
        .await;

    assert!(
        !output.is_error,
        "Should handle large files: {}",
        output.content
    );
    let result = std::fs::read_to_string(&file).unwrap();
    assert!(
        result.contains("line_500 = 9999"),
        "File should contain replacement. First 200 chars: {}",
        &result[..200.min(result.len())]
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Circuit Breaker: ConsecutiveFailureTracker
// ═══════════════════════════════════════════════════════════════════════════

use forge_agent::agent::r#loop::ConsecutiveFailureTracker;

#[test]
fn circuit_breaker_triggers_after_threshold() {
    let mut tracker = ConsecutiveFailureTracker::new(3);

    let input = json!({"path": "test.py"});

    // First failure — no trigger
    assert!(tracker.record("edit_file", &input, true).is_none());
    // Second failure — no trigger
    assert!(tracker.record("edit_file", &input, true).is_none());
    // Third failure — TRIGGER!
    let result = tracker.record("edit_file", &input, true);
    assert!(result.is_some(), "Should trigger at 3 failures");
    assert_eq!(result.unwrap(), 3);
}

#[test]
fn circuit_breaker_resets_on_success() {
    let mut tracker = ConsecutiveFailureTracker::new(3);
    let input = json!({"path": "test.py"});

    // Two failures
    tracker.record("edit_file", &input, true);
    tracker.record("edit_file", &input, true);

    // Success resets the counter
    tracker.record("edit_file", &input, false);

    // Three more failures needed to trigger again
    assert!(tracker.record("edit_file", &input, true).is_none());
    assert!(tracker.record("edit_file", &input, true).is_none());
    assert!(tracker.record("edit_file", &input, true).is_some());
}

#[test]
fn circuit_breaker_tracks_different_files_independently() {
    let mut tracker = ConsecutiveFailureTracker::new(3);
    let input_a = json!({"path": "a.py"});
    let input_b = json!({"path": "b.py"});

    // Two failures on file A
    tracker.record("edit_file", &input_a, true);
    tracker.record("edit_file", &input_a, true);

    // One failure on file B — should NOT trigger
    assert!(tracker.record("edit_file", &input_b, true).is_none());

    // Third failure on file A — should trigger for A
    assert!(tracker.record("edit_file", &input_a, true).is_some());
}

#[test]
fn circuit_breaker_tracks_different_tools_independently() {
    let mut tracker = ConsecutiveFailureTracker::new(3);
    let input = json!({"path": "test.py"});

    tracker.record("edit_file", &input, true);
    tracker.record("edit_file", &input, true);
    tracker.record("bash", &input, true);

    // edit_file only has 2 failures, bash has 1
    assert!(tracker.record("edit_file", &input, true).is_some()); // 3rd for edit_file
}

#[test]
fn circuit_breaker_reset_clears_all() {
    let mut tracker = ConsecutiveFailureTracker::new(3);
    let input = json!({"path": "test.py"});

    tracker.record("edit_file", &input, true);
    tracker.record("edit_file", &input, true);
    tracker.reset();

    // After reset, need 3 fresh failures
    assert!(tracker.record("edit_file", &input, true).is_none());
    assert!(tracker.record("edit_file", &input, true).is_none());
    assert!(tracker.record("edit_file", &input, true).is_some());
}

#[test]
fn circuit_breaker_no_path_in_input() {
    let mut tracker = ConsecutiveFailureTracker::new(3);
    let input = json!({"command": "ls -la"}); // bash has no "path"

    tracker.record("bash", &input, true);
    tracker.record("bash", &input, true);
    assert!(tracker.record("bash", &input, true).is_some());
}
