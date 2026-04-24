//! Tests for src/tools/mod.rs — ToolRegistry

use forge_agent::tools::ToolRegistry;

#[test]
fn registry_with_builtins_has_tools() {
    let registry = ToolRegistry::with_builtins();
    let names = registry.tool_names();
    assert!(names.len() >= 30, "Expected 30+ tools, got {}", names.len());
}

#[test]
fn registry_contains_read_file() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("read_file").is_some());
}

#[test]
fn registry_contains_write_file() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("write_file").is_some());
}

#[test]
fn registry_contains_edit_file() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("edit_file").is_some());
}

#[test]
fn registry_contains_create_file() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("create_file").is_some());
}

#[test]
fn registry_contains_delete_file() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("delete_file").is_some());
}

#[test]
fn registry_contains_list_directory() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("list_directory").is_some());
}

#[test]
fn registry_contains_move_file() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("move_file").is_some());
}

#[test]
fn registry_contains_copy_file() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("copy_file").is_some());
}

#[test]
fn registry_contains_bash() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("bash").is_some());
}

#[test]
fn registry_contains_powershell() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("powershell").is_some());
}

#[test]
fn registry_contains_git_tools() {
    let registry = ToolRegistry::with_builtins();
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
        assert!(registry.get(name).is_some(), "Missing git tool: {name}");
    }
}

#[test]
fn registry_contains_search_tools() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("search_files").is_some());
    assert!(registry.get("find_files").is_some());
}

#[test]
fn registry_contains_web_tools() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("web_fetch").is_some());
    assert!(registry.get("web_search").is_some());
}

#[test]
fn registry_contains_code_quality() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("run_linter").is_some());
    assert!(registry.get("run_tests").is_some());
    assert!(registry.get("run_formatter").is_some());
}

#[test]
fn registry_contains_task_tools() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("todo_write").is_some());
    assert!(registry.get("task_create").is_some());
    assert!(registry.get("task_update").is_some());
    assert!(registry.get("task_get").is_some());
    assert!(registry.get("task_list").is_some());
}

#[test]
fn registry_contains_agent_orchestration() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("ask_user").is_some());
    assert!(registry.get("enter_plan_mode").is_some());
    assert!(registry.get("exit_plan_mode").is_some());
}

#[test]
fn registry_contains_notebook() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("notebook_read").is_some());
}

#[test]
fn registry_contains_worktree_tools() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("enter_worktree").is_some());
    assert!(registry.get("exit_worktree").is_some());
    assert!(registry.get("list_worktrees").is_some());
}

#[test]
fn registry_unknown_tool_returns_none() {
    let registry = ToolRegistry::with_builtins();
    assert!(registry.get("nonexistent_tool_xyz").is_none());
}

#[test]
fn registry_all_definitions_sorted() {
    let registry = ToolRegistry::with_builtins();
    let defs = registry.all_definitions();
    for window in defs.windows(2) {
        assert!(
            window[0].name <= window[1].name,
            "Definitions not sorted: {} > {}",
            window[0].name,
            window[1].name
        );
    }
}

#[test]
fn registry_tool_names_sorted() {
    let registry = ToolRegistry::with_builtins();
    let names = registry.tool_names();
    for window in names.windows(2) {
        assert!(window[0] <= window[1]);
    }
}

#[test]
fn registry_all_tools_have_descriptions() {
    let registry = ToolRegistry::with_builtins();
    let defs = registry.all_definitions();
    for def in &defs {
        assert!(
            !def.description.is_empty(),
            "Tool {} has empty description",
            def.name
        );
    }
}

#[test]
fn registry_all_tools_have_valid_parameters() {
    let registry = ToolRegistry::with_builtins();
    let defs = registry.all_definitions();
    for def in &defs {
        assert!(
            def.parameters.is_object(),
            "Tool {} parameters is not an object",
            def.name
        );
    }
}

#[test]
fn registry_empty() {
    let registry = ToolRegistry::new();
    assert!(registry.tool_names().is_empty());
    assert!(registry.all_definitions().is_empty());
}
