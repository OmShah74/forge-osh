//! Tests for src/tools/agent_tools.rs

use forge_agent::tools::Tool;
use forge_agent::tools::ToolRegistry;
use forge_agent::types::*;

#[test]
fn ask_user_tool_exists_and_has_schema() {
    let r = ToolRegistry::with_builtins();
    let tool = r.get("ask_user").unwrap();
    assert!(!tool.description().is_empty());
    assert!(tool.parameters_schema().is_object());
}

#[test]
fn enter_plan_mode_tool_exists() {
    let r = ToolRegistry::with_builtins();
    let tool = r.get("enter_plan_mode").unwrap();
    assert!(!tool.description().is_empty());
}

#[test]
fn exit_plan_mode_tool_exists() {
    let r = ToolRegistry::with_builtins();
    let tool = r.get("exit_plan_mode").unwrap();
    assert!(!tool.description().is_empty());
}

#[test]
fn agent_tools_are_readonly() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("ask_user").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
    assert_eq!(
        r.get("enter_plan_mode").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
    assert_eq!(
        r.get("exit_plan_mode").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
}
