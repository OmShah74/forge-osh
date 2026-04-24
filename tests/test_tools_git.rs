//! Tests for src/tools/git.rs — permission levels and schema validation

use forge_agent::tools::Tool;
use forge_agent::tools::ToolRegistry;
use forge_agent::types::*;

#[test]
fn git_status_is_readonly() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_status").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
}

#[test]
fn git_diff_is_readonly() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_diff").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
}

#[test]
fn git_log_is_readonly() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_log").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
}

#[test]
fn git_blame_is_readonly() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_blame").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
}

#[test]
fn git_show_is_readonly() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_show").unwrap().permission_level(),
        PermissionLevel::ReadOnly
    );
}

#[test]
fn git_add_is_mutating() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_add").unwrap().permission_level(),
        PermissionLevel::Mutating
    );
}

#[test]
fn git_commit_is_mutating() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_commit").unwrap().permission_level(),
        PermissionLevel::Mutating
    );
}

#[test]
fn git_branch_is_mutating() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_branch").unwrap().permission_level(),
        PermissionLevel::Mutating
    );
}

#[test]
fn git_checkout_is_mutating() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_checkout").unwrap().permission_level(),
        PermissionLevel::Mutating
    );
}

#[test]
fn git_stash_is_mutating() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_stash").unwrap().permission_level(),
        PermissionLevel::Mutating
    );
}

#[test]
fn git_reset_is_destructive() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_reset").unwrap().permission_level(),
        PermissionLevel::Destructive
    );
}

#[test]
fn git_fetch_is_network() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_fetch").unwrap().permission_level(),
        PermissionLevel::Network
    );
}

#[test]
fn git_push_is_network() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_push").unwrap().permission_level(),
        PermissionLevel::Network
    );
}

#[test]
fn git_pull_is_network() {
    let r = ToolRegistry::with_builtins();
    assert_eq!(
        r.get("git_pull").unwrap().permission_level(),
        PermissionLevel::Network
    );
}

#[test]
fn all_git_tools_have_parameters_schema() {
    let r = ToolRegistry::with_builtins();
    let git_tools = [
        "git_status",
        "git_diff",
        "git_log",
        "git_add",
        "git_commit",
        "git_branch",
        "git_checkout",
        "git_stash",
        "git_blame",
        "git_show",
        "git_reset",
        "git_fetch",
        "git_push",
        "git_pull",
    ];
    for name in git_tools {
        let schema = r.get(name).unwrap().parameters_schema();
        assert!(schema.is_object(), "{name} schema not an object");
    }
}
