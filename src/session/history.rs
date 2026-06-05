use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationHistory {
    pub messages: Vec<Message>,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConversationHistory {
    pub fn new(session_id: String) -> Self {
        let now = Utc::now();
        Self {
            messages: Vec::new(),
            session_id,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_user(&mut self, text: String) {
        self.messages.push(Message::User(UserContent::Text(text)));
        self.updated_at = Utc::now();
    }

    /// Add a user message that may include images (multimodal content).
    pub fn add_user_content(&mut self, content: UserContent) {
        self.messages.push(Message::User(content));
        self.updated_at = Utc::now();
    }

    pub fn add_assistant(&mut self, content: AssistantContent) {
        self.messages.push(Message::Assistant(content));
        self.updated_at = Utc::now();
    }

    pub fn add_tool_result(&mut self, result: ToolResult) {
        self.messages.push(Message::Tool(result));
        self.updated_at = Utc::now();
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn last_n(&self, n: usize) -> &[Message] {
        let start = self.messages.len().saturating_sub(n);
        &self.messages[start..]
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Replace the prefix of `messages` with a single summary message,
    /// keeping the last `keep_last` messages verbatim.
    ///
    /// With `keep_last = 0` every existing message is replaced by the
    /// summary (the typical auto-compact case). The previous off-by-one
    /// guard `<= keep_last + 1` caused a 1-message conversation to silently
    /// skip compaction — now we only bail when there is strictly nothing
    /// older than `keep_last` to drop.
    pub fn summarize_old(&mut self, summary: String, keep_last: usize) {
        if self.messages.len() <= keep_last {
            return;
        }
        let keep_from = self.messages.len().saturating_sub(keep_last);
        let kept = self.messages.split_off(keep_from);
        self.messages.clear();
        self.messages.push(Message::User(UserContent::Text(format!(
            "[Previous conversation summary]: {summary}"
        ))));
        self.messages.extend(kept);
        self.updated_at = Utc::now();
    }

    /// Keep only the last `keep_last` messages, discarding earlier ones.
    /// Used by the /compact command to free context window space.
    pub fn compact(&mut self, keep_last: usize) {
        if self.messages.len() <= keep_last {
            return;
        }
        let keep_from = self.messages.len().saturating_sub(keep_last);
        let kept = self.messages.split_off(keep_from);
        self.messages = kept;
        self.updated_at = Utc::now();
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_history() {
        let mut history = ConversationHistory::new("test".to_string());
        assert_eq!(history.message_count(), 0);

        history.add_user("hello".to_string());
        assert_eq!(history.message_count(), 1);

        history.add_assistant(AssistantContent::Text("hi there".to_string()));
        assert_eq!(history.message_count(), 2);

        let last = history.last_n(1);
        assert_eq!(last.len(), 1);
    }

    #[test]
    fn test_summarize() {
        let mut history = ConversationHistory::new("test".to_string());
        for i in 0..10 {
            history.add_user(format!("message {i}"));
            history.add_assistant(AssistantContent::Text(format!("response {i}")));
        }
        assert_eq!(history.message_count(), 20);

        history.summarize_old("Summary of first 16 messages".to_string(), 4);
        // Should have: 1 summary + 4 kept
        assert_eq!(history.message_count(), 5);
    }
}
