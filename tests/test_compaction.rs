//! Tests for src/agent/compaction.rs

use forge_agent::agent::compaction::*;
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
