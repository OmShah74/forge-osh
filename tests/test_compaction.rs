//! Tests for src/agent/compaction.rs

use forge_agent::agent::compaction::*;
use forge_agent::session::checkpoint::Checkpoint;
use forge_agent::session::tokens::TokenCounter;
use forge_agent::session::Session;
use forge_agent::types::*;

#[test]
fn split_compaction_normal() {
    let msgs: Vec<Message> = (0..20)
        .map(|i| Message::User(UserContent::Text(format!("msg {i}"))))
        .collect();
    let (to_summarize, to_keep) = split_for_compaction(&msgs, 8);
    assert_eq!(to_summarize.len(), 12);
    assert_eq!(to_keep.len(), 8);
}

#[test]
fn split_compaction_nothing_to_do() {
    let msgs: Vec<Message> = vec![
        Message::User(UserContent::Text("hello".into())),
        Message::User(UserContent::Text("world".into())),
    ];
    let (to_summarize, to_keep) = split_for_compaction(&msgs, 10);
    assert_eq!(to_summarize.len(), 0);
    assert_eq!(to_keep.len(), 2);
}

#[test]
fn split_compaction_exact_boundary() {
    let msgs: Vec<Message> = (0..8)
        .map(|i| Message::User(UserContent::Text(format!("msg {i}"))))
        .collect();
    let (to_summarize, to_keep) = split_for_compaction(&msgs, 8);
    assert_eq!(to_summarize.len(), 0);
    assert_eq!(to_keep.len(), 8);
}

#[test]
fn split_compaction_empty() {
    let msgs: Vec<Message> = vec![];
    let (to_summarize, to_keep) = split_for_compaction(&msgs, 8);
    assert_eq!(to_summarize.len(), 0);
    assert_eq!(to_keep.len(), 0);
}

#[test]
fn split_compaction_keep_one() {
    let msgs: Vec<Message> = (0..5)
        .map(|i| Message::User(UserContent::Text(format!("msg {i}"))))
        .collect();
    let (to_summarize, to_keep) = split_for_compaction(&msgs, 1);
    assert_eq!(to_summarize.len(), 4);
    assert_eq!(to_keep.len(), 1);
}

#[test]
fn default_keep_last_constant() {
    // Auto-compaction replaces the entire conversation with a summary (keep
    // nothing verbatim). Users can override per-invocation via `/compact <n>`.
    assert_eq!(DEFAULT_KEEP_LAST, 0);
}

#[test]
fn compacted_history_persists_as_summary_only() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("FORGE_DATA_DIR", dir.path());

    let mut session = Session::new(
        "compact-persist".into(),
        "openai".into(),
        "gpt-4o".into(),
        ".".into(),
    );
    for i in 0..10 {
        session.history.add_user(format!("user message {i}"));
        session
            .history
            .add_assistant(AssistantContent::Text(format!("assistant message {i}")));
    }

    let summary = "summary survives reload";
    session.history.summarize_old(summary.to_string(), 0);
    session.cost_tracker.last_prompt_tokens =
        TokenCounter::count_messages(session.history.messages());
    session.cost_tracker.last_output_tokens = 0;
    session.save().unwrap();

    let loaded = Checkpoint::load(&session.id).unwrap();
    assert_eq!(loaded.history.message_count(), 1);
    match &loaded.history.messages()[0] {
        Message::User(UserContent::Text(text)) => {
            assert!(text.contains(summary));
            assert!(!text.contains("user message 0"));
            assert!(!text.contains("assistant message 9"));
        }
        other => panic!("expected compacted summary user message, got {other:?}"),
    }
    assert_eq!(
        loaded.cost_tracker.context_tokens_estimate(),
        session.cost_tracker.last_prompt_tokens as u64
    );
}
