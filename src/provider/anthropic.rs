use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::config::models::anthropic_models;
use crate::error::{ForgeError, Result};
use crate::session::tokens::TokenCounter;
use crate::types::*;

use super::Provider;

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    model_info: ModelInfo,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Result<Self> {
        let models = anthropic_models();
        let model_info = models
            .iter()
            .find(|m| m.id == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo {
                id: model.clone(),
                name: model.clone(),
                context_window: 200_000,
                supports_tools: true,
                supports_vision: true,
                input_cost_per_million: 3.0,
                output_cost_per_million: 15.0,
                provider_id: "anthropic".to_string(),
            });

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ForgeError::Provider(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            client,
            api_key,
            base_url,
            model,
            model_info,
        })
    }

    /// Build the messages array. When `cache_last` is true, attach an
    /// `ephemeral` cache_control marker to the LAST content block of the
    /// final message so the entire conversation prefix up to that point
    /// gets cached. Anthropic allows up to 4 cache breakpoints per request.
    fn build_messages(&self, messages: &[Message], cache_last: bool) -> Vec<Value> {
        let last_idx = messages.len().saturating_sub(1);
        messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let mark_this = cache_last && i == last_idx;
                match msg {
                    Message::User(UserContent::Text(text)) => {
                        if mark_this {
                            // Wrap as a content-block array so we can attach cache_control.
                            json!({
                                "role": "user",
                                "content": [{
                                    "type": "text",
                                    "text": text,
                                    "cache_control": {"type": "ephemeral"}
                                }]
                            })
                        } else {
                            json!({"role": "user", "content": text})
                        }
                    }
                    Message::Assistant(content) => {
                        let mut blocks = Vec::new();
                        if let Some(text) = content.text() {
                            if !text.is_empty() {
                                blocks.push(json!({
                                    "type": "text",
                                    "text": text
                                }));
                            }
                        }
                        for tc in content.tool_calls() {
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": tc.input
                            }));
                        }
                        if blocks.is_empty() {
                            blocks.push(json!({"type": "text", "text": ""}));
                        }
                        if mark_this {
                            if let Some(last) = blocks.last_mut() {
                                if let Some(obj) = last.as_object_mut() {
                                    obj.insert(
                                        "cache_control".to_string(),
                                        json!({"type": "ephemeral"}),
                                    );
                                }
                            }
                        }
                        json!({
                            "role": "assistant",
                            "content": blocks
                        })
                    }
                    Message::Tool(result) => {
                        let mut block = json!({
                            "type": "tool_result",
                            "tool_use_id": result.tool_use_id,
                            "content": result.content,
                            "is_error": result.is_error
                        });
                        if mark_this {
                            if let Some(obj) = block.as_object_mut() {
                                obj.insert(
                                    "cache_control".to_string(),
                                    json!({"type": "ephemeral"}),
                                );
                            }
                        }
                        json!({
                            "role": "user",
                            "content": [block]
                        })
                    }
                }
            })
            .collect()
    }

    /// Build tool definitions. When `cache_last` is true, attach an ephemeral
    /// cache_control marker to the LAST tool — this caches the entire tools
    /// array (plus everything before it: tools are sent after system).
    fn build_tools(&self, tools: &[ToolDefinition], cache_last: bool) -> Vec<Value> {
        let last_idx = tools.len().saturating_sub(1);
        tools
            .iter()
            .enumerate()
            .map(|(i, tool)| {
                let mut v = json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters
                });
                if cache_last && i == last_idx {
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert(
                            "cache_control".to_string(),
                            json!({"type": "ephemeral"}),
                        );
                    }
                }
                v
            })
            .collect()
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn name(&self) -> &str {
        "Anthropic"
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(anthropic_models())
    }

    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/messages", self.base_url);

        // Prompt caching strategy (Anthropic allows up to 4 ephemeral breakpoints):
        //   1) system prompt — single ephemeral block (very stable across turns)
        //   2) tools array — marker on last tool (stable across turns)
        //   3) conversation prefix — marker on last message (lets the
        //      growing conversation prefix get cached after each turn)
        // The 5-minute TTL (default) is renewed every request that re-reads
        // the same prefix, so an active conversation keeps its prefix warm.
        let cache_system = request
            .system
            .as_ref()
            .map(|s| TokenCounter::count_text(s) >= 1024)
            .unwrap_or(false);
        let cache_tools = request
            .tools
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false);
        // Cache the conversation prefix only when there's enough context to
        // make it worthwhile. <1024 tokens of history is below Anthropic's
        // minimum cacheable size for Sonnet/Haiku — adding a breakpoint there
        // is a wasted ephemeral marker.
        let cache_messages = TokenCounter::count_messages(&request.messages) >= 2048;

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": true,
            "messages": self.build_messages(&request.messages, cache_messages),
        });

        if let Some(system) = &request.system {
            if cache_system {
                body["system"] = json!([{
                    "type": "text",
                    "text": system,
                    "cache_control": {"type": "ephemeral"}
                }]);
            } else {
                body["system"] = json!(system);
            }
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                body["tools"] = json!(self.build_tools(tools, cache_tools));
            }
        }

        // Extended thinking (Claude 3.7+ / 4+). The thinking block is
        // advisory — if the model does not support it the server simply
        // ignores the field. Anthropic requires `temperature = 1.0` when
        // thinking is enabled, so we override.
        match request.thinking {
            ThinkingConfig::Disabled => {}
            ThinkingConfig::Enabled => {
                // Anthropic needs a budget; default to half of max_tokens,
                // floored at 1024 (the API minimum).
                let budget = (request.max_tokens / 2).max(1024);
                body["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": budget,
                });
                body["temperature"] = json!(1.0);
            }
            ThinkingConfig::Budget { tokens } => {
                body["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": tokens.max(1024),
                });
                body["temperature"] = json!(1.0);
            }
        }

        if !request.stop_sequences.is_empty() {
            body["stop_sequences"] = json!(request.stop_sequences);
        }

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

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
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_input = String::new();
        let mut usage = Usage::default();
        let mut stop_reason = CompletionReason::EndTurn;
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE events from buffer
            while let Some(event_end) = buffer.find("\n\n") {
                let event_text = buffer[..event_end].to_string();
                buffer = buffer[event_end + 2..].to_string();

                // Parse event type and data
                let mut event_type = String::new();
                let mut data = String::new();

                for line in event_text.lines() {
                    if let Some(et) = line.strip_prefix("event: ") {
                        event_type = et.to_string();
                    } else if let Some(d) = line.strip_prefix("data: ") {
                        data = d.to_string();
                    }
                }

                if data.is_empty() || data == "[DONE]" {
                    continue;
                }

                let parsed: Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                match event_type.as_str() {
                    "message_start" => {
                        if let Some(u) = parsed.get("message").and_then(|m| m.get("usage")) {
                            // Anthropic separates uncached/created/read input tokens.
                            // input_tokens here is the count of NEW (uncached) input
                            // tokens — it excludes cache reads and cache creation.
                            usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0) as u32;
                            let cw = u["cache_creation_input_tokens"].as_u64().unwrap_or(0) as u32;
                            let cr = u["cache_read_input_tokens"].as_u64().unwrap_or(0) as u32;
                            if cw > 0 {
                                usage.cache_write_tokens = Some(cw);
                            }
                            if cr > 0 {
                                usage.cache_read_tokens = Some(cr);
                            }
                        }
                    }
                    "content_block_start" => {
                        if let Some(cb) = parsed.get("content_block") {
                            if cb["type"] == "tool_use" {
                                current_tool_id = cb["id"].as_str().unwrap_or("").to_string();
                                current_tool_name = cb["name"].as_str().unwrap_or("").to_string();
                                current_tool_input.clear();
                                let _ = tx.send(StreamEvent::ToolCallStart {
                                    id: current_tool_id.clone(),
                                    name: current_tool_name.clone(),
                                });
                            }
                        }
                    }
                    "content_block_delta" => {
                        if let Some(delta) = parsed.get("delta") {
                            if delta["type"] == "text_delta" {
                                if let Some(text) = delta["text"].as_str() {
                                    full_text.push_str(text);
                                    let _ = tx.send(StreamEvent::Token(text.to_string()));
                                }
                            } else if delta["type"] == "input_json_delta" {
                                if let Some(json_delta) = delta["partial_json"].as_str() {
                                    current_tool_input.push_str(json_delta);
                                    let _ = tx.send(StreamEvent::ToolCallDelta {
                                        id: current_tool_id.clone(),
                                        arguments_delta: json_delta.to_string(),
                                    });
                                }
                            }
                        }
                    }
                    "content_block_stop" => {
                        if !current_tool_name.is_empty() {
                            let input: Value =
                                serde_json::from_str(&current_tool_input).unwrap_or(json!({}));
                            tool_calls.push(ToolCall {
                                id: current_tool_id.clone(),
                                name: current_tool_name.clone(),
                                input,
                            });
                            let _ = tx.send(StreamEvent::ToolCallEnd {
                                id: current_tool_id.clone(),
                            });
                            current_tool_name.clear();
                            current_tool_id.clear();
                            current_tool_input.clear();
                        }
                    }
                    "message_delta" => {
                        if let Some(delta) = parsed.get("delta") {
                            if let Some(sr) = delta["stop_reason"].as_str() {
                                stop_reason = match sr {
                                    "end_turn" => CompletionReason::EndTurn,
                                    "tool_use" => CompletionReason::ToolUse,
                                    "max_tokens" => CompletionReason::MaxTokens,
                                    "stop_sequence" => CompletionReason::StopSequence,
                                    _ => CompletionReason::Unknown,
                                };
                            }
                        }
                        if let Some(u) = parsed.get("usage") {
                            usage.output_tokens = u["output_tokens"].as_u64().unwrap_or(0) as u32;
                        }
                    }
                    "message_stop" => {
                        // Message complete
                    }
                    _ => {}
                }
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
