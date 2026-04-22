use serde::{Deserialize, Serialize};

use crate::types::*;

/// Precise token counter using tiktoken where possible.
///
/// Provider usage figures are always the ground truth for already-submitted
/// requests; this counter exists to (a) pre-flight requests against the
/// context window before we pay for them and (b) power the progress bar in
/// between turns. We use the `cl100k_base` tokenizer — a good approximation
/// for Claude's BPE and an exact match for most OpenAI-compatible models.
/// Falling back to `chars/4` would underestimate code-heavy prompts by 20-40%.
pub struct TokenCounter;

impl TokenCounter {
    fn encoder() -> &'static tiktoken_rs::CoreBPE {
        use std::sync::OnceLock;
        static ENCODER: OnceLock<tiktoken_rs::CoreBPE> = OnceLock::new();
        ENCODER.get_or_init(|| {
            tiktoken_rs::cl100k_base()
                .expect("cl100k_base tokenizer is baked into the binary")
        })
    }

    /// Count tokens in an arbitrary text string.
    pub fn count_text(text: &str) -> u32 {
        if text.is_empty() { return 0; }
        let enc = Self::encoder();
        enc.encode_with_special_tokens(text).len() as u32
    }

    /// Count tokens across the full message list, including per-message
    /// "role overhead" (Claude/OpenAI both surface a few tokens of framing
    /// per message — we add 4 as a conservative constant).
    pub fn count_messages(messages: &[Message]) -> u32 {
        const ROLE_OVERHEAD: u32 = 4;
        let mut total: u32 = 0;
        for m in messages {
            total = total.saturating_add(ROLE_OVERHEAD);
            match m {
                Message::User(UserContent::Text(t)) => {
                    total = total.saturating_add(Self::count_text(t));
                }
                Message::Assistant(content) => {
                    if let Some(text) = content.text() {
                        total = total.saturating_add(Self::count_text(text));
                    }
                    for tc in content.tool_calls() {
                        total = total.saturating_add(Self::count_text(&tc.name));
                        let input_str = tc.input.to_string();
                        total = total.saturating_add(Self::count_text(&input_str));
                    }
                }
                Message::Tool(result) => {
                    total = total.saturating_add(Self::count_text(&result.content));
                }
            }
        }
        total
    }

    /// Estimate the token cost of a request (system + messages + tool
    /// definitions). Useful before issuing a call to catch accidental
    /// context-window overflows.
    pub fn count_request(
        system: Option<&str>,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> u32 {
        let mut total = system.map(Self::count_text).unwrap_or(0);
        total = total.saturating_add(Self::count_messages(messages));
        if let Some(defs) = tools {
            for d in defs {
                total = total
                    .saturating_add(Self::count_text(&d.name))
                    .saturating_add(Self::count_text(&d.description))
                    .saturating_add(Self::count_text(&d.parameters.to_string()));
            }
        }
        total
    }
}

/// Tracks cost across a session
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostTracker {
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub total_cost_usd: f64,
    /// Last known input_tokens reported by the provider for the most recent
    /// API call. Providers typically report the FULL prompt token count every
    /// turn (not a delta), so this is the best estimate of "what's in the
    /// model's context window right now" — which is what we show in the
    /// progress bar. Cumulative `total_input_tokens` keeps growing across
    /// turns and does not reflect actual context usage.
    #[serde(default)]
    pub last_prompt_tokens: u32,
    #[serde(default)]
    pub last_output_tokens: u32,
    #[serde(default)]
    entries: Vec<CostEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        self.last_prompt_tokens = usage.input_tokens;
        self.last_output_tokens = usage.output_tokens;

        self.entries.push(CostEntry {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            input_cost_per_million: input_cost_per_m,
            output_cost_per_million: output_cost_per_m,
            cost_usd: cost,
        });
    }

    /// Best-estimate of tokens currently filling the model's context window.
    /// Uses the last reported prompt token count when available; otherwise
    /// falls back to the cumulative input tokens (which is a conservative
    /// overestimate but better than zero for providers that don't return
    /// usage).
    pub fn context_tokens_estimate(&self) -> u64 {
        if self.last_prompt_tokens > 0 {
            self.last_prompt_tokens as u64 + self.last_output_tokens as u64
        } else {
            self.total_input_tokens + self.total_output_tokens
        }
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
    fn test_tiktoken_is_not_chars_over_four() {
        // "def foo(x, y): return x + y" — 27 chars, but BPE gives ~10 tokens.
        // The old chars/4 estimator returned 7. This test locks us in to the
        // better estimator.
        let code = "def foo(x, y): return x + y";
        let precise = TokenCounter::count_text(code);
        assert!(precise >= 8 && precise <= 14,
            "tiktoken count should be in the realistic range, got {precise}");
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
