//! Tests for src/tools/executor.rs

use forge_agent::tools::executor::*;

#[test]
fn executor_creates_correctly() {
    let exec = ToolExecutor::new(100);
    // Just verify it builds
    assert!(true);
}
