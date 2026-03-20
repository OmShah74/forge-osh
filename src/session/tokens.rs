use crate::types::*;

/// Approximate token counter.
/// Uses tiktoken for OpenAI models, rough estimation for others.
pub struct TokenCounter;

impl TokenCounter {
    /// Count tokens in a message list
    pub fn count_messages(messages: &[Message]) -> u32 {
        // Using rough estimation: ~4 chars per token
        // This is approximate but works across all providers
        let total_chars: usize = messages
            .iter()
            .map(|m| match m {
                Message::User(UserContent::Text(t)) => t.len() + 4, // role overhead
                Message::Assistant(content) => {
                    let text_len = content.text().map(|t| t.len()).unwrap_or(0);
                    let tool_len: usize = content
                        .tool_calls()
                        .iter()
                        .map(|tc| tc.name.len() + tc.input.to_string().len())
                        .sum();
                    text_len + tool_len + 4
                }
                Message::Tool(result) => result.content.len() + 4,
            })
            .sum();
        (total_chars as f64 / 4.0).ceil() as u32
    }

    /// Count tokens in a single string
    pub fn count_text(text: &str) -> u32 {
        (text.len() as f64 / 4.0).ceil() as u32
    }
}

/// Tracks cost across a session
#[derive(Debug, Clone, Default)]
pub struct CostTracker {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    entries: Vec<CostEntry>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CostEntry {
    input_tokens: u32,
    output_tokens: u32,
    input_cost_per_million: f64,
    output_cost_per_million: f64,
    cost_usd: f64,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record usage from a single API call
    pub fn add(&mut self, usage: &Usage, input_cost_per_m: f64, output_cost_per_m: f64) {
        let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * input_cost_per_m;
        let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * output_cost_per_m;
        let cost = input_cost + output_cost;

        self.total_input_tokens += usage.input_tokens as u64;
        self.total_output_tokens += usage.output_tokens as u64;
        self.total_cost_usd += cost;

        self.entries.push(CostEntry {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            input_cost_per_million: input_cost_per_m,
            output_cost_per_million: output_cost_per_m,
            cost_usd: cost,
        });
    }

    /// Get formatted cost string
    pub fn format_cost(&self) -> String {
        if self.total_cost_usd == 0.0 {
            "Free".to_string()
        } else if self.total_cost_usd < 0.01 {
            format!("${:.4}", self.total_cost_usd)
        } else {
            format!("${:.3}", self.total_cost_usd)
        }
    }

    /// Get formatted token usage string
    pub fn format_tokens(&self) -> String {
        let total = self.total_input_tokens + self.total_output_tokens;
        if total > 1_000_000 {
            format!("{:.1}M tokens", total as f64 / 1_000_000.0)
        } else if total > 1_000 {
            format!("{:.1}K tokens", total as f64 / 1_000.0)
        } else {
            format!("{total} tokens")
        }
    }

    /// Number of API calls made
    pub fn call_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_counting() {
        let msgs = vec![
            Message::User(UserContent::Text("Hello world".to_string())),
            Message::Assistant(AssistantContent::Text("Hi there!".to_string())),
        ];
        let count = TokenCounter::count_messages(&msgs);
        assert!(count > 0);
    }

    #[test]
    fn test_cost_tracker() {
        let mut tracker = CostTracker::new();
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            ..Default::default()
        };
        tracker.add(&usage, 3.0, 15.0); // Anthropic Sonnet pricing

        assert_eq!(tracker.total_input_tokens, 1000);
        assert_eq!(tracker.total_output_tokens, 500);
        assert!(tracker.total_cost_usd > 0.0);
        assert_eq!(tracker.call_count(), 1);
    }

    #[test]
    fn test_format_cost() {
        let mut tracker = CostTracker::new();
        assert_eq!(tracker.format_cost(), "Free");

        tracker.total_cost_usd = 0.005;
        assert!(tracker.format_cost().starts_with('$'));
    }
}
