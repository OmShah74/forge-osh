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
            tiktoken_rs::cl100k_base().expect("cl100k_base tokenizer is baked into the binary")
        })
    }

    /// Count tokens in an arbitrary text string.
    pub fn count_text(text: &str) -> u32 {
        if text.is_empty() {
            return 0;
        }
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
                Message::User(uc) => {
                    total = total.saturating_add(Self::count_text(&uc.to_text()));
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

/// Per-provider cache pricing multipliers, applied to the provider's base
/// input cost. These match the publicly documented discounts as of 2026:
///
/// - Anthropic: cache reads are 0.10× input cost; cache writes are 1.25×
///   (extra 25% surcharge to populate the cache for 5 minutes).
/// - OpenAI / DeepSeek: cache reads are 0.50× input cost (automatic);
///   there is no separate write surcharge — the first uncached request
///   pays normal input price and the prefix is cached implicitly.
/// - Gemini: cache reads are 0.25× input cost (implicit/automatic).
/// - All other providers: no documented caching → multiplier = 1.0 so the
///   accounting still works if cache_read_tokens is somehow populated.
#[derive(Debug, Clone, Copy)]
pub struct CacheMultipliers {
    pub read: f64,
    pub write: f64,
}

impl CacheMultipliers {
    pub fn for_provider(provider_id: &str) -> Self {
        Self::for_route(provider_id, "")
    }

    /// Provider-aware cache pricing. When provider_id is "openrouter", the
    /// model id is inspected to pick the right underlying multiplier
    /// (anthropic/* → Anthropic prices, openai/* → OpenAI, etc.).
    pub fn for_route(provider_id: &str, model_id: &str) -> Self {
        let resolved = if provider_id == "openrouter" {
            let m = model_id.to_ascii_lowercase();
            if m.starts_with("anthropic/") {
                "anthropic"
            } else if m.starts_with("openai/") {
                "openai"
            } else if m.starts_with("google/") || m.starts_with("gemini/") {
                "gemini"
            } else if m.starts_with("deepseek/") {
                "deepseek"
            } else if m.starts_with("z-ai/")
                || m.starts_with("zhipuai/")
                || m.starts_with("zai/")
                || m.contains("/glm-")
                || m.starts_with("glm-")
            {
                "glm"
            } else {
                "openrouter-other"
            }
        } else {
            provider_id
        };
        match resolved {
            "anthropic" => Self {
                read: 0.10,
                write: 1.25,
            },
            "openai" | "deepseek" | "glm" => Self {
                read: 0.50,
                write: 1.0,
            },
            "gemini" => Self {
                read: 0.25,
                write: 1.0,
            },
            _ => Self {
                read: 1.0,
                write: 1.0,
            },
        }
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
    pub total_cache_read_tokens: u64,
    #[serde(default)]
    pub total_cache_write_tokens: u64,
    #[serde(default)]
    pub total_cost_usd: f64,
    /// What the same usage would have cost without any caching applied.
    /// Useful for showing "you saved $X by caching" in `/cost`.
    #[serde(default)]
    pub total_uncached_cost_usd: f64,
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
    /// Cache read/write tokens from the most recent API call. Cleared to 0
    /// when the next call returns no cache activity.
    #[serde(default)]
    pub last_cache_read_tokens: u32,
    #[serde(default)]
    pub last_cache_write_tokens: u32,
    #[serde(default)]
    entries: Vec<CostEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct CostEntry {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_read_tokens: u32,
    #[serde(default)]
    cache_write_tokens: u32,
    input_cost_per_million: f64,
    output_cost_per_million: f64,
    cost_usd: f64,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record usage from a single API call.
    ///
    /// Backwards-compat shim: callers that don't know which provider the
    /// usage came from get no cache discount. Prefer `add_with_route`.
    pub fn add(&mut self, usage: &Usage, input_cost_per_m: f64, output_cost_per_m: f64) {
        self.add_with_route(usage, "", "", input_cost_per_m, output_cost_per_m);
    }

    /// Record usage with just the provider id (legacy — assumes provider_id
    /// alone resolves the cache pricing; for OpenRouter, prefer `add_with_route`).
    pub fn add_with_provider(
        &mut self,
        usage: &Usage,
        provider_id: &str,
        input_cost_per_m: f64,
        output_cost_per_m: f64,
    ) {
        self.add_with_route(usage, provider_id, "", input_cost_per_m, output_cost_per_m);
    }

    /// Record usage from a single API call, applying provider+model-aware
    /// cache pricing. `cache_read_tokens` and `cache_write_tokens` on the
    /// `Usage` drive the discount; `input_tokens` is the uncached portion
    /// (callers must normalize that — see provider impls).
    pub fn add_with_route(
        &mut self,
        usage: &Usage,
        provider_id: &str,
        model_id: &str,
        input_cost_per_m: f64,
        output_cost_per_m: f64,
    ) {
        let mults = CacheMultipliers::for_route(provider_id, model_id);
        let cache_read = usage.cache_read_tokens.unwrap_or(0);
        let cache_write = usage.cache_write_tokens.unwrap_or(0);

        let per_million = |n: u32, rate: f64| (n as f64 / 1_000_000.0) * rate;

        let uncached_input_cost = per_million(usage.input_tokens, input_cost_per_m);
        let cache_read_cost = per_million(cache_read, input_cost_per_m * mults.read);
        let cache_write_cost = per_million(cache_write, input_cost_per_m * mults.write);
        let output_cost = per_million(usage.output_tokens, output_cost_per_m);
        let cost = uncached_input_cost + cache_read_cost + cache_write_cost + output_cost;

        // What it would have cost without caching: every input token billed
        // at the full input rate (cache reads + writes folded into input).
        let total_input_eq = usage.input_tokens + cache_read + cache_write;
        let uncached_cost =
            per_million(total_input_eq, input_cost_per_m) + output_cost;

        self.total_input_tokens += usage.input_tokens as u64;
        self.total_output_tokens += usage.output_tokens as u64;
        self.total_cache_read_tokens += cache_read as u64;
        self.total_cache_write_tokens += cache_write as u64;
        self.total_cost_usd += cost;
        self.total_uncached_cost_usd += uncached_cost;
        // last_prompt_tokens should reflect what's actually in the model's
        // context window — the uncached PLUS cached input both occupy
        // context space, so we sum them for the progress bar.
        self.last_prompt_tokens = usage.input_tokens + cache_read + cache_write;
        self.last_output_tokens = usage.output_tokens;
        self.last_cache_read_tokens = cache_read;
        self.last_cache_write_tokens = cache_write;

        self.entries.push(CostEntry {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            input_cost_per_million: input_cost_per_m,
            output_cost_per_million: output_cost_per_m,
            cost_usd: cost,
        });
    }

    /// USD saved by prompt caching this session (positive number means
    /// caching helped). Always non-negative for sane inputs.
    pub fn cache_savings_usd(&self) -> f64 {
        (self.total_uncached_cost_usd - self.total_cost_usd).max(0.0)
    }

    /// Cache hit-rate across the lifetime of the session: cache reads as a
    /// percentage of all input tokens (cached + uncached). Returns 0.0 when
    /// no input tokens have been observed.
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.total_input_tokens
            + self.total_cache_read_tokens
            + self.total_cache_write_tokens;
        if total == 0 {
            0.0
        } else {
            (self.total_cache_read_tokens as f64 / total as f64) * 100.0
        }
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

    /// Format an arbitrary cost figure with the same rules as [`Self::format_cost`].
    /// Exposed so the TUI can render a LIVE (base + in-flight estimate) cost
    /// during streaming without mutating the tracker.
    pub fn format_cost_total(cost: f64) -> String {
        if cost == 0.0 {
            "Free".to_string()
        } else if cost < 0.01 {
            format!("${cost:.4}")
        } else {
            format!("${cost:.3}")
        }
    }

    /// Format an arbitrary token total with the same rules as [`Self::format_tokens`].
    pub fn format_tokens_total(total: u64) -> String {
        if total > 1_000_000 {
            format!("{:.1}M tokens", total as f64 / 1_000_000.0)
        } else if total > 1_000 {
            format!("{:.1}K tokens", total as f64 / 1_000.0)
        } else {
            format!("{total} tokens")
        }
    }

    /// Total tokens accounted so far (input + output + cache read/write).
    pub fn total_tokens_all(&self) -> u64 {
        self.total_input_tokens
            + self.total_output_tokens
            + self.total_cache_read_tokens
            + self.total_cache_write_tokens
    }

    /// Get formatted cost string
    pub fn format_cost(&self) -> String {
        Self::format_cost_total(self.total_cost_usd)
    }

    /// Get formatted token usage string. Includes cached tokens because they
    /// still occupy context window space and contribute to the bill (even
    /// if at a discounted rate).
    pub fn format_tokens(&self) -> String {
        Self::format_tokens_total(self.total_tokens_all())
    }

    /// One-line cache summary suitable for the /cost modal:
    ///   "1.2K cached read · 800 cached write · 64.2% hit · saved $0.42"
    pub fn format_cache_summary(&self) -> String {
        let r = self.total_cache_read_tokens;
        let w = self.total_cache_write_tokens;
        if r == 0 && w == 0 {
            return "No cached tokens yet — try a longer session or enable caching".to_string();
        }
        let fmt = |n: u64| {
            if n >= 1_000_000 {
                format!("{:.1}M", n as f64 / 1_000_000.0)
            } else if n >= 1_000 {
                format!("{:.1}K", n as f64 / 1_000.0)
            } else {
                n.to_string()
            }
        };
        let savings = self.cache_savings_usd();
        let savings_s = if savings < 0.01 {
            format!("${savings:.4}")
        } else {
            format!("${savings:.3}")
        };
        format!(
            "{} cached read · {} cached write · {:.1}% hit · saved {}",
            fmt(r),
            fmt(w),
            self.cache_hit_rate(),
            savings_s
        )
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
        assert!(
            precise >= 8 && precise <= 14,
            "tiktoken count should be in the realistic range, got {precise}"
        );
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
