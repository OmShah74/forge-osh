//! Tests for src/agent/context.rs — ContextManager

use forge_agent::agent::context::{ContextManager, ContextStatus};
use forge_agent::session::history::ConversationHistory;
use forge_agent::types::*;

#[test]
fn context_manager_ok_on_empty() {
    let cm = ContextManager::new(200_000);
    let history = ConversationHistory::new("test".into());
    let status = cm.check(&history);
    assert!(status.is_ok());
}

#[test]
fn context_manager_usage_percent_zero() {
    let cm = ContextManager::new(100_000);
    let history = ConversationHistory::new("test".into());
    let pct = cm.usage_percent(&history);
    assert!(pct < 1.0, "Empty history usage should be near 0%");
}

#[test]
fn context_manager_status_used_and_limit() {
    let cm = ContextManager::new(100_000);
    let history = ConversationHistory::new("test".into());
    let status = cm.check(&history);
    assert_eq!(status.limit(), 100_000);
    assert_eq!(status.used(), 0);
}

#[test]
fn context_manager_thresholds() {
    let cm = ContextManager::new(100_000);
    assert!((cm.warn_threshold - 0.80).abs() < 0.01);
    assert!((cm.summarize_threshold - 0.90).abs() < 0.01);
}
