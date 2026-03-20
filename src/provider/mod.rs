pub mod anthropic;
pub mod gemini;
pub mod ollama;
pub mod openai_compat;
pub mod router;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::Result;
use crate::types::*;

/// The core provider trait. Every LLM backend implements this.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Provider identifier (e.g., "anthropic", "openai", "ollama")
    fn id(&self) -> &str;

    /// Human-readable name
    fn name(&self) -> &str;

    /// List available models
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;

    /// Send a chat completion request, streaming tokens via sender
    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<ChatResponse>;

    /// Count tokens for a message list (approximate if provider doesn't support it)
    fn count_tokens(&self, messages: &[Message]) -> u32 {
        // Default: rough approximation at ~4 chars per token
        let total_chars: usize = messages
            .iter()
            .map(|m| match m {
                Message::User(UserContent::Text(t)) => t.len(),
                Message::Assistant(content) => {
                    content.text().map(|t| t.len()).unwrap_or(0)
                        + content
                            .tool_calls()
                            .iter()
                            .map(|tc| tc.input.to_string().len())
                            .sum::<usize>()
                }
                Message::Tool(result) => result.content.len(),
            })
            .sum();
        (total_chars as f64 / 4.0).ceil() as u32
    }

    /// Whether this provider supports tool/function calling
    fn supports_tools(&self) -> bool;

    /// Whether this provider supports vision (image inputs)
    fn supports_vision(&self) -> bool;

    /// Maximum context window in tokens for the current model
    fn context_window(&self) -> u32;

    /// Cost per 1M input tokens (USD, 0.0 for local)
    fn input_cost_per_million(&self) -> f64;

    /// Cost per 1M output tokens (USD, 0.0 for local)
    fn output_cost_per_million(&self) -> f64;

    /// Current model id
    fn model_id(&self) -> &str;
}
