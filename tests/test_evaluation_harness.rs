#[path = "support/eval.rs"]
mod eval;

use forge_agent::agent::compaction;
use forge_agent::agent::permissions::PermissionStore;
use forge_agent::skills::{apply_skill, SkillLoader};
use forge_agent::tools::executor::ToolExecutor;
use forge_agent::tools::ToolRegistry;
use forge_agent::tui::{
    analyze_paste_budget, normalize_clipboard_text, PasteBudget, PasteRecommendation,
};
use forge_agent::types::{
    AssistantContent, Message, PermissionMode, PermissionResponse, ToolCall, ToolContext,
    UserContent,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use eval::{text_response, tool_use_response, EvalHarness, ScriptedProvider};

#[tokio::test(flavor = "current_thread")]
async fn golden_agent_turn_records_tool_call_and_tool_result() {
    let read_call = ToolCall {
        id: "read-1".to_string(),
        name: "read_file".to_string(),
        input: json!({ "path": "sample.txt" }),
    };
    let harness = EvalHarness::new(vec![
        tool_use_response(read_call),
        text_response("I read sample.txt successfully."),
    ]);
    std::fs::write(harness.workspace_path("sample.txt"), "hello from eval").unwrap();

    harness
        .agent
        .run("read sample.txt".to_string())
        .await
        .unwrap();

    let requests = harness.provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].model, "eval-model");
    assert!(requests[0]
        .tools
        .as_ref()
        .expect("tool definitions")
        .iter()
        .any(|tool| tool.name == "read_file"));
    assert!(matches!(
        &requests[0].messages[0],
        Message::User(UserContent::Text(text)) if text == "read sample.txt"
    ));
    assert!(requests[1].messages.iter().any(|message| {
        matches!(
            message,
            Message::Tool(result)
                if result.tool_use_id == "read-1" && result.content.contains("hello from eval")
        )
    }));

    let history = harness.history().await;
    assert!(history.iter().any(|message| {
        matches!(
            message,
            Message::Assistant(AssistantContent::ToolUse(calls))
                if calls.iter().any(|call| call.name == "read_file")
        )
    }));
    assert!(history.iter().any(|message| {
        matches!(
            message,
            Message::Assistant(AssistantContent::Text(text))
                if text.contains("I read sample.txt successfully")
        )
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn golden_compaction_uses_mock_provider_and_installs_summary_only() {
    let provider = ScriptedProvider::new(vec![text_response(
        "## Context & Goal\nThe user wanted stable compaction.\n\n## Files Touched\n- src/session/history.rs preserved summary behavior.\n\n## Current State & Next Step\nThe compacted summary should replace older messages.",
    )]);
    let mut messages = Vec::new();
    for i in 0..4 {
        messages.push(Message::User(UserContent::Text(format!("user {i}"))));
        messages.push(Message::Assistant(AssistantContent::Text(format!(
            "assistant {i}"
        ))));
    }

    let summary = compaction::summarize_messages(&messages, &[], &provider, "eval-model", 128_000)
        .await
        .unwrap();
    assert!(summary.contains("## Context & Goal"));

    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "eval-model");
    assert!(requests[0].tools.is_none());
    assert!(matches!(
        &requests[0].messages[0],
        Message::User(UserContent::Text(text)) if text.contains("TRANSCRIPT TO COMPRESS")
    ));

    let mut history = forge_agent::session::history::ConversationHistory::new("eval".into());
    for message in messages {
        match message {
            Message::User(uc) => history.add_user_content(uc),
            Message::Assistant(content) => history.add_assistant(content),
            Message::Tool(result) => history.add_tool_result(result),
        }
    }
    history.summarize_old(summary.clone(), 0);
    assert_eq!(history.message_count(), 1);
    assert!(matches!(
        &history.messages()[0],
        Message::User(UserContent::Text(text))
            if text.contains("[Previous conversation summary]") && text.contains(&summary)
    ));
}

#[test]
fn golden_paste_budget_classifies_inline_warning_and_reject() {
    let inline = analyze_paste_budget(
        "small paste",
        PasteBudget {
            history_tokens: 100,
            system_tokens: 100,
            tool_tokens: 100,
            context_limit: 8_000,
            max_response_tokens: 512,
            safety_margin_tokens: 256,
        },
    );
    assert_eq!(inline.recommendation, PasteRecommendation::InsertInline);

    let near_limit = analyze_paste_budget(
        &"word ".repeat(2_500),
        PasteBudget {
            history_tokens: 2_000,
            system_tokens: 1_000,
            tool_tokens: 1_000,
            context_limit: 8_000,
            max_response_tokens: 512,
            safety_margin_tokens: 256,
        },
    );
    assert_eq!(
        near_limit.recommendation,
        PasteRecommendation::InsertInlineWithWarning
    );

    let rejected = analyze_paste_budget(
        &"word ".repeat(20_000),
        PasteBudget {
            history_tokens: 0,
            system_tokens: 0,
            tool_tokens: 0,
            context_limit: 8_000,
            max_response_tokens: 512,
            safety_margin_tokens: 256,
        },
    );
    assert_eq!(rejected.recommendation, PasteRecommendation::RejectTooLarge);
    assert_eq!(
        normalize_clipboard_text("\u{1b}[200~a\r\nb\r\0\u{1b}[201~"),
        "a\nb\n"
    );
}

#[test]
fn golden_skill_invocation_loads_project_skill_with_constraints() {
    let _guard = eval::env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("FORGE_CONFIG_DIR", dir.path().join("config"));
    std::env::set_var("FORGE_DATA_DIR", dir.path().join("data"));
    let skill_dir = dir.path().join(".claude").join("skills").join("eval-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Eval Skill\ndescription: Eval skill\nallowed_tools:\n  - read_file\nexecution_mode: inline\nuser_invocable: true\n---\nUse only safe reads for ${FORGE_SESSION_ID}.",
    )
    .unwrap();

    let registry = SkillLoader::load(dir.path());
    let applied = apply_skill(&registry, "eval-skill", Some("arg"), "session-123").unwrap();
    assert_eq!(applied.skill_name, "eval-skill");
    assert_eq!(applied.allowed_tools, vec!["read_file"]);
    assert!(applied.materialized_prompt.contains("session-123"));
}

#[tokio::test(flavor = "current_thread")]
async fn golden_permission_and_diff_review_prevent_unapproved_file_write() {
    let _guard = eval::env_lock();
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let executor = ToolExecutor::new(10_000);
    let call = ToolCall {
        id: "write-1".to_string(),
        name: "write_file".to_string(),
        input: json!({
            "path": "created.txt",
            "content": "should not be written"
        }),
    };
    let ctx = ToolContext {
        working_dir: dir.path().to_path_buf(),
        home_dir: dir.path().to_path_buf(),
        session_id: "eval".to_string(),
        trust_mode: false,
        permission_mode: PermissionMode::Default,
        diff_review: true,
        file_cache: None,
        active_skill_scope: None,
        skill_registry: None,
        output_chunk_tx: None,
        tool_call_id: None,
        team_blackboard: None,
    };

    let output = executor
        .execute(
            &call,
            &ctx,
            &registry,
            &PermissionStore::default(),
            &CancellationToken::new(),
            |_name, description, level| async move {
                assert_eq!(level, forge_agent::types::PermissionLevel::Mutating);
                assert!(description.contains("write_file"));
                assert!(description.contains("Patch Review"));
                PermissionResponse::Deny
            },
        )
        .await;

    assert!(output.is_error);
    assert!(!dir.path().join("created.txt").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn golden_plan_mode_denies_mutating_tool_without_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::with_builtins();
    let executor = ToolExecutor::new(10_000);
    let call = ToolCall {
        id: "write-1".to_string(),
        name: "write_file".to_string(),
        input: json!({
            "path": "blocked.txt",
            "content": "blocked"
        }),
    };
    let ctx = ToolContext {
        working_dir: dir.path().to_path_buf(),
        home_dir: dir.path().to_path_buf(),
        session_id: "eval".to_string(),
        trust_mode: false,
        permission_mode: PermissionMode::Plan,
        diff_review: true,
        file_cache: None,
        active_skill_scope: None,
        skill_registry: None,
        output_chunk_tx: None,
        tool_call_id: None,
        team_blackboard: None,
    };

    let output = executor
        .execute(
            &call,
            &ctx,
            &registry,
            &PermissionStore::default(),
            &CancellationToken::new(),
            |_name, _description, _level| async {
                panic!("plan mode should deny mutating tools before prompting")
            },
        )
        .await;

    assert!(output.is_error);
    assert!(output.content.contains("plan mode"));
    assert!(!dir.path().join("blocked.txt").exists());
}
