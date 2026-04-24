//! Integration tests for the skills subsystem.
//!
//! Covers: loading a project-local skill from disk, invoking it through the
//! `invoke_skill` tool, verifying the tool output carries the materialized
//! prompt + metadata, and verifying the scope-narrowing logic in the executor.

use forge_agent::skills::{apply_skill, ActiveSkillScope, SkillExecutionMode, SkillLoader};
use forge_agent::tools::ToolRegistry;
use forge_agent::types::*;

fn make_ctx(dir: &std::path::Path) -> ToolContext {
    ToolContext {
        working_dir: dir.to_path_buf(),
        home_dir: dir.to_path_buf(),
        session_id: "skills-test".into(),
        trust_mode: true,
        permission_mode: PermissionMode::Default,
        file_cache: None,
        active_skill_scope: None,
        skill_registry: None,
    }
}

fn write_skill(root: &std::path::Path, name: &str, body: &str) {
    let dir = root.join(".claude").join("skills").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), body).unwrap();
}

#[tokio::test]
async fn invoke_skill_tool_returns_materialized_prompt_in_content() {
    let dir = tempfile::tempdir().unwrap();
    write_skill(
        dir.path(),
        "demo",
        "---\nname: Demo\ndescription: Test skill\nallowed_tools:\n  - read_file\n---\nDo the demo workflow.",
    );

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("invoke_skill").unwrap();
    let ctx = make_ctx(dir.path());
    let output = tool
        .execute(serde_json::json!({"skill": "demo"}), &ctx)
        .await;

    assert!(!output.is_error, "tool returned error: {}", output.content);
    assert!(
        output.content.contains("Do the demo workflow."),
        "materialized prompt should appear in tool_result content, got: {}",
        output.content
    );

    let meta = output.metadata.expect("metadata present");
    let inv = &meta["skill_invocation"];
    assert_eq!(inv["skill_name"], "demo");
    assert_eq!(inv["mode"], "inline");
    assert_eq!(inv["source"], "project");
    let tools = inv["applied_allowed_tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0], "read_file");
}

#[tokio::test]
async fn invoke_skill_tool_errors_on_unknown_skill() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("invoke_skill").unwrap();
    let ctx = make_ctx(dir.path());

    let output = tool
        .execute(serde_json::json!({"skill": "nonexistent"}), &ctx)
        .await;
    assert!(output.is_error);
    assert!(output.content.to_lowercase().contains("unknown"));
}

#[test]
fn apply_skill_resolves_project_skill() {
    let dir = tempfile::tempdir().unwrap();
    write_skill(
        dir.path(),
        "helper",
        "---\nname: Helper\ndescription: helper\n---\nBody text with ${ARGS}.",
    );
    let registry = SkillLoader::load(dir.path());
    let applied = apply_skill(&registry, "helper", Some("my-args"), "sess").unwrap();
    assert_eq!(applied.skill_name, "helper");
    assert_eq!(applied.mode, SkillExecutionMode::Inline);
    assert!(applied.materialized_prompt.contains("my-args"));
}

#[test]
fn active_scope_denies_tools_outside_allowlist() {
    let scope = ActiveSkillScope {
        skill_name: "demo".into(),
        allowed_tools: vec!["read_file".into()],
        model_override: None,
        hooks: Default::default(),
        execution_mode: SkillExecutionMode::Inline,
    };
    assert!(scope.allows_tool("read_file"));
    assert!(!scope.allows_tool("write_file"));
}

#[test]
fn active_scope_empty_allowlist_is_permissive() {
    let scope = ActiveSkillScope {
        skill_name: "demo".into(),
        allowed_tools: vec![],
        model_override: None,
        hooks: Default::default(),
        execution_mode: SkillExecutionMode::Inline,
    };
    assert!(scope.allows_tool("anything"));
}

#[tokio::test]
async fn invoke_skill_tool_is_not_concurrency_safe() {
    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("invoke_skill").unwrap();
    assert!(
        !tool.is_concurrency_safe(),
        "invoke_skill mutates shared session state and must not run in parallel"
    );
}

#[test]
fn skill_scaffold_creates_valid_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Manually scaffold a skill in the same shape /skill new does, then
    // round-trip it through SkillLoader to confirm it parses as intended.
    let skill_dir = root.join(".claude").join("skills").join("my-test");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let body = "---\n\
                name: my-test\n\
                description: Scaffolded test skill.\n\
                when_to_use: When running the scaffold test.\n\
                allowed_tools:\n  \
                  - read_file\n  \
                  - search_files\n\
                execution_mode: inline\n\
                user_invocable: true\n\
                ---\n\n\
                # my-test\n\n\
                Body goes here with ${ARGS}.\n";
    std::fs::write(skill_dir.join("SKILL.md"), body).unwrap();

    let registry = SkillLoader::load(root);
    let found = registry.find("my-test").expect("scaffold parses");
    assert_eq!(found.name, "my-test");
    assert_eq!(found.description, "Scaffolded test skill.");
    assert_eq!(found.allowed_tools, vec!["read_file", "search_files"]);
    assert_eq!(found.execution_mode, SkillExecutionMode::Inline);
    assert!(found.user_invocable);

    let applied = apply_skill(&registry, "my-test", Some("hello"), "sess").unwrap();
    assert!(applied.materialized_prompt.contains("hello"));
}

#[test]
fn project_overrides_bundled_skill() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".claude").join("skills").join("review");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Project override.\n---\nProject body.",
    )
    .unwrap();

    let registry = SkillLoader::load(root);
    let review = registry.find("review").expect("review exists");
    assert_eq!(review.description, "Project override.");
    assert!(matches!(
        review.source,
        forge_agent::skills::SkillSource::Project
    ));
}

#[tokio::test]
async fn invoke_skill_uses_shared_registry_when_available() {
    use parking_lot::RwLock;
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    write_skill(
        dir.path(),
        "shared-demo",
        "---\nname: shared-demo\ndescription: Shared registry path\n---\nShared body.",
    );

    let shared = Arc::new(RwLock::new(SkillLoader::load(dir.path())));
    let mut ctx = make_ctx(dir.path());
    ctx.skill_registry = Some(shared);

    let registry = ToolRegistry::with_builtins();
    let tool = registry.get("invoke_skill").unwrap();
    let output = tool
        .execute(serde_json::json!({"skill": "shared-demo"}), &ctx)
        .await;

    assert!(!output.is_error, "got: {}", output.content);
    assert!(output.content.contains("Shared body."));
}
