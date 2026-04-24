//! Tests for src/session/ — ConversationHistory, Session, CostTracker, TokenCounter

use forge_agent::session::history::ConversationHistory;
use forge_agent::session::tokens::{CostTracker, TokenCounter};
use forge_agent::session::Session;
use forge_agent::types::*;

// ─── ConversationHistory ─────────────────────────────────────────────────

#[test]
fn history_new_is_empty() {
    let h = ConversationHistory::new("test".into());
    assert_eq!(h.message_count(), 0);
    assert!(h.messages().is_empty());
}

#[test]
fn history_add_user_message() {
    let mut h = ConversationHistory::new("test".into());
    h.add_user("hello".into());
    assert_eq!(h.message_count(), 1);
    match &h.messages()[0] {
        Message::User(UserContent::Text(t)) => assert_eq!(t, "hello"),
        _ => panic!("Expected User message"),
    }
}

#[test]
fn history_add_assistant_message() {
    let mut h = ConversationHistory::new("test".into());
    h.add_assistant(AssistantContent::Text("world".into()));
    assert_eq!(h.message_count(), 1);
}

#[test]
fn history_add_tool_result() {
    let mut h = ConversationHistory::new("test".into());
    h.add_tool_result(ToolResult {
        tool_use_id: "tc1".into(),
        content: "result".into(),
        is_error: false,
    });
    assert_eq!(h.message_count(), 1);
}

#[test]
fn history_last_n() {
    let mut h = ConversationHistory::new("test".into());
    for i in 0..10 {
        h.add_user(format!("msg {i}"));
    }
    let last3 = h.last_n(3);
    assert_eq!(last3.len(), 3);
}

#[test]
fn history_last_n_more_than_available() {
    let mut h = ConversationHistory::new("test".into());
    h.add_user("only one".into());
    let last5 = h.last_n(5);
    assert_eq!(last5.len(), 1);
}

#[test]
fn history_summarize_old() {
    let mut h = ConversationHistory::new("test".into());
    for i in 0..20 {
        h.add_user(format!("msg {i}"));
        h.add_assistant(AssistantContent::Text(format!("resp {i}")));
    }
    assert_eq!(h.message_count(), 40);
    h.summarize_old("summary of old msgs".into(), 4);
    assert_eq!(h.message_count(), 5); // 1 summary + 4 kept
}

#[test]
fn history_summarize_noop_when_small() {
    let mut h = ConversationHistory::new("test".into());
    h.add_user("hello".into());
    h.summarize_old("summary".into(), 10);
    assert_eq!(h.message_count(), 1); // unchanged
}

#[test]
fn history_compact() {
    let mut h = ConversationHistory::new("test".into());
    for i in 0..20 {
        h.add_user(format!("msg {i}"));
    }
    h.compact(5);
    assert_eq!(h.message_count(), 5);
}

#[test]
fn history_compact_noop_when_small() {
    let mut h = ConversationHistory::new("test".into());
    h.add_user("hello".into());
    h.compact(10);
    assert_eq!(h.message_count(), 1);
}

#[test]
fn history_clear() {
    let mut h = ConversationHistory::new("test".into());
    h.add_user("hello".into());
    h.clear();
    assert_eq!(h.message_count(), 0);
}

// ─── TokenCounter ────────────────────────────────────────────────────────

#[test]
fn token_counter_empty_messages() {
    let msgs: Vec<Message> = vec![];
    assert_eq!(TokenCounter::count_messages(&msgs), 0);
}

#[test]
fn token_counter_user_message() {
    let msgs = vec![Message::User(UserContent::Text("hello world".into()))];
    let count = TokenCounter::count_messages(&msgs);
    assert!(count > 0, "Token count should be positive");
}

#[test]
fn token_counter_mixed_messages() {
    let msgs = vec![
        Message::User(UserContent::Text("hello".into())),
        Message::Assistant(AssistantContent::Text("hi there".into())),
        Message::Tool(ToolResult {
            tool_use_id: "tc1".into(),
            content: "result".into(),
            is_error: false,
        }),
    ];
    let count = TokenCounter::count_messages(&msgs);
    assert!(count > 0);
}

#[test]
fn token_counter_text() {
    assert_eq!(TokenCounter::count_text(""), 0);
    let count = TokenCounter::count_text("hello world test");
    assert!(count > 0);
}

// ─── CostTracker ─────────────────────────────────────────────────────────

#[test]
fn cost_tracker_new_is_zero() {
    let ct = CostTracker::new();
    assert_eq!(ct.total_input_tokens, 0);
    assert_eq!(ct.total_output_tokens, 0);
    assert_eq!(ct.total_cost_usd, 0.0);
    assert_eq!(ct.call_count(), 0);
    assert_eq!(ct.format_cost(), "Free");
}

#[test]
fn cost_tracker_add_usage() {
    let mut ct = CostTracker::new();
    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 500,
        ..Default::default()
    };
    ct.add(&usage, 3.0, 15.0);
    assert_eq!(ct.total_input_tokens, 1000);
    assert_eq!(ct.total_output_tokens, 500);
    assert!(ct.total_cost_usd > 0.0);
    assert_eq!(ct.call_count(), 1);
}

#[test]
fn cost_tracker_multiple_adds() {
    let mut ct = CostTracker::new();
    for _ in 0..5 {
        ct.add(
            &Usage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
            3.0,
            15.0,
        );
    }
    assert_eq!(ct.total_input_tokens, 500);
    assert_eq!(ct.total_output_tokens, 250);
    assert_eq!(ct.call_count(), 5);
}

#[test]
fn cost_tracker_format_cost_free() {
    let ct = CostTracker::new();
    assert_eq!(ct.format_cost(), "Free");
}

#[test]
fn cost_tracker_format_cost_small() {
    let mut ct = CostTracker::new();
    ct.total_cost_usd = 0.005;
    let formatted = ct.format_cost();
    assert!(formatted.starts_with('$'));
}

#[test]
fn cost_tracker_format_cost_large() {
    let mut ct = CostTracker::new();
    ct.total_cost_usd = 1.234;
    let formatted = ct.format_cost();
    assert!(formatted.starts_with('$'));
}

#[test]
fn cost_tracker_format_tokens_small() {
    let mut ct = CostTracker::new();
    ct.total_input_tokens = 500;
    ct.total_output_tokens = 200;
    let f = ct.format_tokens();
    assert!(f.contains("tokens"));
}

#[test]
fn cost_tracker_format_tokens_kilo() {
    let mut ct = CostTracker::new();
    ct.total_input_tokens = 5000;
    ct.total_output_tokens = 3000;
    let f = ct.format_tokens();
    assert!(f.contains("K tokens"));
}

#[test]
fn cost_tracker_format_tokens_mega() {
    let mut ct = CostTracker::new();
    ct.total_input_tokens = 800_000;
    ct.total_output_tokens = 400_000;
    let f = ct.format_tokens();
    assert!(f.contains("M tokens"));
}

// ─── Session ─────────────────────────────────────────────────────────────

#[test]
fn session_new_defaults() {
    let s = Session::new("test".into(), "openai".into(), "gpt-4o".into(), ".".into());
    assert!(!s.id.is_empty());
    assert_eq!(s.name, "test");
    assert_eq!(s.provider_id, "openai");
    assert_eq!(s.model_id, "gpt-4o");
    assert_eq!(s.effort_level, 3);
    assert_eq!(s.history.message_count(), 0);
}

#[test]
fn session_record_usage() {
    let mut s = Session::new("test".into(), "openai".into(), "gpt-4o".into(), ".".into());
    s.record_usage(
        &Usage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        },
        5.0,
        15.0,
    );
    assert!(s.cost_tracker.total_cost_usd > 0.0);
}

#[test]
fn session_format_cost() {
    let s = Session::new("test".into(), "openai".into(), "gpt-4o".into(), ".".into());
    assert_eq!(s.format_cost(), "Free");
}

#[test]
fn session_format_tokens() {
    let s = Session::new("test".into(), "openai".into(), "gpt-4o".into(), ".".into());
    let t = s.format_tokens();
    assert!(t.contains("tokens"));
}
