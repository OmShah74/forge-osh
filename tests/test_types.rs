//! Tests for src/types.rs — core data structures

use forge_agent::types::*;

// ─── Message Enum ────────────────────────────────────────────────────────

#[test]
fn message_user_variant() {
    let msg = Message::User(UserContent::Text("hello".into()));
    match &msg {
        Message::User(UserContent::Text(t)) => assert_eq!(t, "hello"),
        _ => panic!("Expected User variant"),
    }
}

#[test]
fn message_assistant_text_variant() {
    let msg = Message::Assistant(AssistantContent::Text("hi".into()));
    match &msg {
        Message::Assistant(content) => {
            assert_eq!(content.text(), Some("hi"));
            assert!(content.tool_calls().is_empty());
        }
        _ => panic!("Expected Assistant variant"),
    }
}

#[test]
fn message_assistant_tool_use_variant() {
    let tc = ToolCall {
        id: "tc1".into(),
        name: "read_file".into(),
        input: serde_json::json!({"path": "/tmp/test.rs"}),
    };
    let msg = Message::Assistant(AssistantContent::ToolUse(vec![tc]));
    match &msg {
        Message::Assistant(content) => {
            assert!(content.text().is_none());
            assert_eq!(content.tool_calls().len(), 1);
            assert_eq!(content.tool_calls()[0].name, "read_file");
        }
        _ => panic!("Expected Assistant variant"),
    }
}

#[test]
fn message_assistant_mixed_variant() {
    let tc = ToolCall {
        id: "tc2".into(),
        name: "bash".into(),
        input: serde_json::json!({"command": "ls"}),
    };
    let content = AssistantContent::Mixed {
        text: "Let me check.".into(),
        tool_calls: vec![tc],
    };
    assert_eq!(content.text(), Some("Let me check."));
    assert_eq!(content.tool_calls().len(), 1);
}

#[test]
fn message_tool_result_variant() {
    let result = ToolResult {
        tool_use_id: "tc1".into(),
        content: "file contents here".into(),
        is_error: false,
    };
    let msg = Message::Tool(result);
    match &msg {
        Message::Tool(r) => {
            assert!(!r.is_error);
            assert_eq!(r.tool_use_id, "tc1");
        }
        _ => panic!("Expected Tool variant"),
    }
}

// ─── Usage ───────────────────────────────────────────────────────────────

#[test]
fn usage_total_tokens() {
    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 500,
        ..Default::default()
    };
    assert_eq!(usage.total_tokens(), 1500);
}

#[test]
fn usage_total_with_zero() {
    let usage = Usage::default();
    assert_eq!(usage.total_tokens(), 0);
}

#[test]
fn usage_cache_tokens_optional() {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: Some(30),
        cache_write_tokens: None,
    };
    assert_eq!(usage.cache_read_tokens, Some(30));
    assert_eq!(usage.cache_write_tokens, None);
}

// ─── ModelInfo ────────────────────────────────────────────────────────────

#[test]
fn model_info_cost_calculation() {
    let model = ModelInfo {
        id: "gpt-4o".into(),
        name: "GPT-4o".into(),
        context_window: 128000,
        supports_tools: true,
        supports_vision: true,
        input_cost_per_million: 5.0,
        output_cost_per_million: 15.0,
        provider_id: "openai".into(),
    };
    let cost = model.cost_for(1_000_000, 1_000_000);
    assert!((cost - 20.0).abs() < 0.001);
}

#[test]
fn model_info_zero_cost() {
    let model = ModelInfo {
        id: "local".into(),
        name: "Local".into(),
        context_window: 4096,
        supports_tools: false,
        supports_vision: false,
        input_cost_per_million: 0.0,
        output_cost_per_million: 0.0,
        provider_id: "ollama".into(),
    };
    assert_eq!(model.cost_for(1000, 1000), 0.0);
}

// ─── ChatRequest ─────────────────────────────────────────────────────────

#[test]
fn chat_request_defaults() {
    let req = ChatRequest::default();
    assert!(req.model.is_empty());
    assert!(req.messages.is_empty());
    assert!(req.tools.is_none());
    assert_eq!(req.max_tokens, 4096);
    assert!((req.temperature - 0.7).abs() < 0.01);
    assert!(req.system.is_none());
    assert!(req.stop_sequences.is_empty());
}

// ─── CompletionReason ────────────────────────────────────────────────────

#[test]
fn completion_reason_equality() {
    assert_eq!(CompletionReason::EndTurn, CompletionReason::EndTurn);
    assert_ne!(CompletionReason::EndTurn, CompletionReason::ToolUse);
    assert_ne!(CompletionReason::MaxTokens, CompletionReason::StopSequence);
}

// ─── PermissionLevel ─────────────────────────────────────────────────────

#[test]
fn permission_level_variants() {
    let levels = vec![
        PermissionLevel::ReadOnly,
        PermissionLevel::Mutating,
        PermissionLevel::Destructive,
        PermissionLevel::Network,
        PermissionLevel::Shell,
    ];
    assert_eq!(levels.len(), 5);
    assert_eq!(PermissionLevel::ReadOnly, PermissionLevel::ReadOnly);
    assert_ne!(PermissionLevel::ReadOnly, PermissionLevel::Mutating);
}

// ─── ToolOutput ──────────────────────────────────────────────────────────

#[test]
fn tool_output_success() {
    let out = ToolOutput::success("done");
    assert!(!out.is_error);
    assert_eq!(out.content, "done");
    assert!(out.metadata.is_none());
}

#[test]
fn tool_output_error() {
    let out = ToolOutput::error("failed");
    assert!(out.is_error);
    assert_eq!(out.content, "failed");
}

// ─── ToolContext ──────────────────────────────────────────────────────────

#[test]
fn tool_context_construction() {
    let ctx = ToolContext {
        working_dir: std::path::PathBuf::from("/project"),
        home_dir: std::path::PathBuf::from("/home/user"),
        session_id: "sess-123".into(),
        trust_mode: false,
        permission_mode: forge_agent::types::PermissionMode::Default,
        diff_review: true,
        file_cache: None,
        active_skill_scope: None,
        skill_registry: None,
        output_chunk_tx: None,
        tool_call_id: None,
    };
    assert_eq!(ctx.session_id, "sess-123");
    assert!(!ctx.trust_mode);
}

// ─── Serialization round-trips ───────────────────────────────────────────

#[test]
fn message_serialization_roundtrip() {
    let original = Message::User(UserContent::Text("test message".into()));
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: Message = serde_json::from_str(&json).unwrap();
    match deserialized {
        Message::User(UserContent::Text(t)) => assert_eq!(t, "test message"),
        _ => panic!("Deserialization failed"),
    }
}

#[test]
fn usage_serialization_roundtrip() {
    let original = Usage {
        input_tokens: 42,
        output_tokens: 17,
        cache_read_tokens: Some(5),
        cache_write_tokens: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.input_tokens, 42);
    assert_eq!(deserialized.output_tokens, 17);
    assert_eq!(deserialized.cache_read_tokens, Some(5));
}

#[test]
fn tool_definition_structure() {
    let td = ToolDefinition {
        name: "read_file".into(),
        description: "Read file contents".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            }
        }),
    };
    assert_eq!(td.name, "read_file");
    assert!(td.parameters.is_object());
}
