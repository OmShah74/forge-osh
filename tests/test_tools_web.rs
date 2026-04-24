//! Tests for src/tools/web.rs

use forge_agent::tools::ToolRegistry;
use forge_agent::types::PermissionLevel;

#[test]
fn web_fetch_tool_exists() {
    let r = ToolRegistry::with_builtins();
    assert!(r.get("web_fetch").is_some());
    assert_eq!(
        r.get("web_fetch").unwrap().permission_level(),
        PermissionLevel::Network
    );
}

#[test]
fn web_search_tool_exists() {
    let r = ToolRegistry::with_builtins();
    assert!(r.get("web_search").is_some());
    assert_eq!(
        r.get("web_search").unwrap().permission_level(),
        PermissionLevel::Network
    );
}
