use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::config::models;
use crate::error::{ForgeError, Result};
use crate::types::*;

use super::Provider;

/// A single provider implementation that handles all OpenAI-compatible APIs:
/// OpenAI, Groq, xAI (Grok), OpenRouter, Mistral, DeepSeek, Together, Fireworks, Perplexity, Cohere
pub struct OpenAICompatProvider {
    client: Client,
    provider_id: String,
    provider_name: String,
    api_key: String,
    base_url: String,
    model: String,
    model_info: ModelInfo,
    extra_headers: Vec<(String, String)>,
}

impl OpenAICompatProvider {
    pub fn new(
        provider_id: String,
        provider_name: String,
        api_key: String,
        base_url: String,
        model: String,
    ) -> Result<Self> {
        let all_models = models::models_for_provider(&provider_id);
        let model_info = all_models
            .iter()
            .find(|m| m.id == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo {
                id: model.clone(),
                name: model.clone(),
                context_window: 128_000,
                supports_tools: true,
                supports_vision: false,
                input_cost_per_million: 0.0,
                output_cost_per_million: 0.0,
                provider_id: provider_id.clone(),
            });

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ForgeError::Provider(format!("Failed to create HTTP client: {e}")))?;

        let extra_headers = if provider_id == "openrouter" {
            vec![
                (
                    "HTTP-Referer".to_string(),
                    "https://forge-osh.dev".to_string(),
                ),
                ("X-Title".to_string(), "forge-osh".to_string()),
            ]
        } else {
            vec![]
        };

        Ok(Self {
            client,
            provider_id,
            provider_name,
            api_key,
            base_url,
            model,
            model_info,
            extra_headers,
        })
    }

    // Convenience constructors for each provider
    pub fn openai(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "openai".into(),
            "OpenAI".into(),
            api_key,
            "https://api.openai.com/v1".into(),
            model,
        )
    }

    pub fn groq(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "groq".into(),
            "Groq".into(),
            api_key,
            "https://api.groq.com/openai/v1".into(),
            model,
        )
    }

    pub fn grok(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "grok".into(),
            "xAI (Grok)".into(),
            api_key,
            "https://api.x.ai/v1".into(),
            model,
        )
    }

    pub fn openrouter(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "openrouter".into(),
            "OpenRouter".into(),
            api_key,
            "https://openrouter.ai/api/v1".into(),
            model,
        )
    }

    pub fn mistral(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "mistral".into(),
            "Mistral".into(),
            api_key,
            "https://api.mistral.ai/v1".into(),
            model,
        )
    }

    pub fn deepseek(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "deepseek".into(),
            "DeepSeek".into(),
            api_key,
            "https://api.deepseek.com/v1".into(),
            model,
        )
    }

    pub fn together(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "together".into(),
            "Together AI".into(),
            api_key,
            "https://api.together.xyz/v1".into(),
            model,
        )
    }

    pub fn fireworks(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "fireworks".into(),
            "Fireworks".into(),
            api_key,
            "https://api.fireworks.ai/inference/v1".into(),
            model,
        )
    }

    pub fn perplexity(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "perplexity".into(),
            "Perplexity".into(),
            api_key,
            "https://api.perplexity.ai".into(),
            model,
        )
    }

    pub fn cohere(api_key: String, model: String) -> Result<Self> {
        Self::new(
            "cohere".into(),
            "Cohere".into(),
            api_key,
            "https://api.cohere.ai/v2".into(),
            model,
        )
    }

    pub fn custom(name: String, api_key: String, base_url: String, model: String) -> Result<Self> {
        Self::new("custom".into(), name, api_key, base_url, model)
    }

    fn build_messages(&self, messages: &[Message]) -> Vec<Value> {
        messages
            .iter()
            .map(|msg| match msg {
                Message::User(UserContent::Text(text)) => {
                    json!({"role": "user", "content": text})
                }
                Message::Assistant(content) => {
                    let mut msg = json!({"role": "assistant"});
                    if let Some(text) = content.text() {
                        msg["content"] = json!(text);
                    }
                    let calls = content.tool_calls();
                    if !calls.is_empty() {
                        msg["tool_calls"] = json!(calls
                            .iter()
                            .map(|tc| json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.input.to_string()
                                }
                            }))
                            .collect::<Vec<_>>());
                    }
                    msg
                }
                Message::Tool(result) => {
                    json!({
                        "role": "tool",
                        "tool_call_id": result.tool_use_id,
                        "content": result.content
                    })
                }
            })
            .collect()
    }

    fn build_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters
                    }
                })
            })
            .collect()
    }
}

#[async_trait]
impl Provider for OpenAICompatProvider {
    fn id(&self) -> &str {
        &self.provider_id
    }

    fn name(&self) -> &str {
        &self.provider_name
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(models::models_for_provider(&self.provider_id))
    }

    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = json!({
            "model": request.model,
            "messages": self.build_messages(&request.messages),
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": true,
            // Ask the server for a final usage chunk so we can track tokens
            // and cost for every OpenAI-compatible provider.
            "stream_options": {"include_usage": true},
        });

        // Inject system message at the start if provided
        if let Some(system) = &request.system {
            if let Some(msgs) = body["messages"].as_array_mut() {
                msgs.insert(0, json!({"role": "system", "content": system}));
            }
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() && self.supports_tools() {
                body["tools"] = json!(self.build_tools(tools));
            }
        }

        if !request.stop_sequences.is_empty() {
            body["stop"] = json!(request.stop_sequences);
        }

        let mut req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        for (key, val) in &self.extra_headers {
            req = req.header(key, val);
        }

        let response = req.json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or(text);
            return Err(ForgeError::api(status, message));
        }

        // Parse SSE stream
        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut tool_calls_map: std::collections::HashMap<u32, (String, String, String)> =
            std::collections::HashMap::new(); // index -> (id, name, arguments)
        let mut usage = Usage::default();
        let mut stop_reason = CompletionReason::EndTurn;
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }

                let data = if let Some(d) = line.strip_prefix("data: ") {
                    d
                } else {
                    continue;
                };

                let parsed: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Extract delta from choices[0].delta
                if let Some(choice) = parsed["choices"].as_array().and_then(|c| c.first()) {
                    if let Some(delta) = choice.get("delta") {
                        // Text content
                        if let Some(content) = delta["content"].as_str() {
                            full_text.push_str(content);
                            let _ = tx.send(StreamEvent::Token(content.to_string()));
                        }

                        // Tool calls
                        if let Some(tcs) = delta["tool_calls"].as_array() {
                            for tc in tcs {
                                let index = tc["index"].as_u64().unwrap_or(0) as u32;
                                let entry = tool_calls_map.entry(index).or_insert_with(|| {
                                    (String::new(), String::new(), String::new())
                                });

                                if let Some(id) = tc["id"].as_str() {
                                    entry.0 = id.to_string();
                                }
                                if let Some(func) = tc.get("function") {
                                    if let Some(name) = func["name"].as_str() {
                                        entry.1 = name.to_string();
                                        let _ = tx.send(StreamEvent::ToolCallStart {
                                            id: entry.0.clone(),
                                            name: name.to_string(),
                                        });
                                    }
                                    if let Some(args) = func["arguments"].as_str() {
                                        entry.2.push_str(args);
                                        let _ = tx.send(StreamEvent::ToolCallDelta {
                                            id: entry.0.clone(),
                                            arguments_delta: args.to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // Finish reason
                    if let Some(fr) = choice["finish_reason"].as_str() {
                        stop_reason = match fr {
                            "stop" => CompletionReason::EndTurn,
                            "tool_calls" => CompletionReason::ToolUse,
                            "length" => CompletionReason::MaxTokens,
                            _ => CompletionReason::Unknown,
                        };
                    }
                }

                // Usage in the final event
                if let Some(u) = parsed.get("usage") {
                    usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                    usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0) as u32;
                }
            }
        }

        // Finalize tool calls
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut indices: Vec<u32> = tool_calls_map.keys().cloned().collect();
        indices.sort();
        for idx in indices {
            if let Some((id, name, args)) = tool_calls_map.remove(&idx) {
                let input: Value = serde_json::from_str(&args).unwrap_or(json!({}));
                tool_calls.push(ToolCall { id, name, input });
                let _ = tx.send(StreamEvent::ToolCallEnd { id: id_stub() });
            }
        }

        let _ = tx.send(StreamEvent::Usage(usage.clone()));
        let _ = tx.send(StreamEvent::Done(stop_reason.clone()));

        let content = if !tool_calls.is_empty() && !full_text.is_empty() {
            AssistantContent::Mixed {
                text: full_text,
                tool_calls,
            }
        } else if !tool_calls.is_empty() {
            AssistantContent::ToolUse(tool_calls)
        } else {
            AssistantContent::Text(full_text)
        };

        Ok(ChatResponse {
            content,
            usage,
            model: request.model,
            stop_reason,
        })
    }

    fn supports_tools(&self) -> bool {
        self.model_info.supports_tools
    }

    fn supports_vision(&self) -> bool {
        self.model_info.supports_vision
    }

    fn context_window(&self) -> u32 {
        self.model_info.context_window
    }

    fn input_cost_per_million(&self) -> f64 {
        self.model_info.input_cost_per_million
    }

    fn output_cost_per_million(&self) -> f64 {
        self.model_info.output_cost_per_million
    }
}

fn id_stub() -> String {
    String::new()
}
