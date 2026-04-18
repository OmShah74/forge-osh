use crate::session::history::ConversationHistory;
use crate::session::tokens::TokenCounter;

/// Manages context window budgets
pub struct ContextManager {
    pub token_limit: u32,
    pub warn_threshold: f32,
    pub summarize_threshold: f32,
}

impl ContextManager {
    pub fn new(token_limit: u32) -> Self {
        Self {
            token_limit,
            warn_threshold: 0.80,
            summarize_threshold: 0.90,
        }
    }

    /// Check current token usage and take action if needed
    pub fn check(&self, history: &ConversationHistory) -> ContextStatus {
        let used = TokenCounter::count_messages(history.messages());
        let ratio = used as f32 / self.token_limit as f32;

        if ratio >= self.summarize_threshold {
            ContextStatus::NeedsSummarization {
                used,
                limit: self.token_limit,
            }
        } else if ratio >= self.warn_threshold {
            ContextStatus::Warning {
                used,
                limit: self.token_limit,
            }
        } else {
            ContextStatus::Ok {
                used,
                limit: self.token_limit,
            }
        }
    }

    /// Get the percentage of context used
    pub fn usage_percent(&self, history: &ConversationHistory) -> f32 {
        let used = TokenCounter::count_messages(history.messages());
        (used as f32 / self.token_limit as f32) * 100.0
    }
}

#[derive(Debug)]
pub enum ContextStatus {
    Ok { used: u32, limit: u32 },
    Warning { used: u32, limit: u32 },
    NeedsSummarization { used: u32, limit: u32 },
}

impl ContextStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, ContextStatus::Ok { .. })
    }

    pub fn used(&self) -> u32 {
        match self {
            ContextStatus::Ok { used, .. }
            | ContextStatus::Warning { used, .. }
            | ContextStatus::NeedsSummarization { used, .. } => *used,
        }
    }

    pub fn limit(&self) -> u32 {
        match self {
            ContextStatus::Ok { limit, .. }
            | ContextStatus::Warning { limit, .. }
            | ContextStatus::NeedsSummarization { limit, .. } => *limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_ok() {
        let cm = ContextManager::new(200_000);
        let history = ConversationHistory::new("test".to_string());
        let status = cm.check(&history);
        assert!(status.is_ok());
    }
}
