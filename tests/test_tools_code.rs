//! Tests for src/tools/code.rs

use forge_agent::tools::ToolRegistry;
use forge_agent::types::*;

#[test]
fn code_tools_exist() {
    let r = ToolRegistry::with_builtins();
    assert!(r.get("run_linter").is_some());
    assert!(r.get("run_tests").is_some());
    assert!(r.get("run_formatter").is_some());
}

#[test]
fn code_tools_are_shell_permission() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("run_linter").unwrap().permission_level(),
        PermissionLevel::Shell
    );
    assert_eq!(
        r.get("run_tests").unwrap().permission_level(),
        PermissionLevel::Shell
    );
    assert_eq!(
        r.get("run_formatter").unwrap().permission_level(),
        PermissionLevel::Shell
    );
}
