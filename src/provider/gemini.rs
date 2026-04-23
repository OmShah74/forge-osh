use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::config::models::gemini_models;
use crate::error::{ForgeError, Result};
use crate::types::*;

use super::Provider;

pub struct GeminiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    model_info: ModelInfo,
}

impl GeminiProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Result<Self> {
        let models = gemini_models();
        let model_info = models
            .iter()
            .find(|m| m.id == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo {
                id: model.clone(),
                name: model.clone(),
                context_window: 1_048_576,
                supports_tools: true,
                supports_vision: true,
                input_cost_per_million: 0.0,
                output_cost_per_million: 0.0,
                provider_id: "gemini".to_string(),
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

    fn build_contents(&self, messages: &[Message]) -> Vec<Value> {
        messages
            .iter()
            .filter_map(|msg| match msg {
                Message::User(UserContent::Text(text)) => Some(json!({
                    "role": "user",
                    "parts": [{"text": text}]
                })),
                Message::Assistant(content) => {
                    let mut parts = Vec::new();
                    if let Some(text) = content.text() {
                        if !text.is_empty() {
                            parts.push(json!({"text": text}));
                        }
                    }
                    for tc in content.tool_calls() {
                        parts.push(json!({
                            "functionCall": {
                                "name": tc.name,
                                "args": tc.input
                            }
                        }));
                    }
                    if parts.is_empty() {
                        None
                    } else {
                        Some(json!({"role": "model", "parts": parts}))
                    }
                }
                Message::Tool(result) => Some(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": result.tool_use_id,
                            "response": {
                                "result": result.content
                            }
                        }
                    }]
                })),
            })
            .collect()
    }

    fn build_tools(&self, tools: &[ToolDefinition]) -> Value {
        let declarations: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                })
            })
            .collect();
        json!([{"functionDeclarations": declarations}])
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn id(&self) -> &str {
        "gemini"
    }

    fn name(&self) -> &str {
        "Google Gemini"
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(gemini_models())
    }

    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<ChatResponse> {
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, request.model, self.api_key
        );

        let mut body = json!({
            "contents": self.build_contents(&request.messages),
            "generationConfig": {
                "maxOutputTokens": request.max_tokens,
                "temperature": request.temperature,
            }
        });

        if let Some(system) = &request.system {
            body["systemInstruction"] = json!({
                "parts": [{"text": system}]
            });
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                body["tools"] = self.build_tools(tools);
            }
        }

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
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

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason = CompletionReason::EndTurn;
        let mut buffer = String::new();
        let mut tc_counter = 0u32;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
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

                if let Some(candidates) = parsed["candidates"].as_array() {
                    for candidate in candidates {
                        if let Some(parts) = candidate["content"]["parts"].as_array() {
                            for part in parts {
                                if let Some(text) = part["text"].as_str() {
                                    full_text.push_str(text);
                                    let _ = tx.send(StreamEvent::Token(text.to_string()));
                                }
                                if let Some(fc) = part.get("functionCall") {
                                    let name = fc["name"].as_str().unwrap_or("").to_string();
                                    let args = fc.get("args").cloned().unwrap_or(json!({}));
                                    let id = format!("tc_{tc_counter}");
                                    tc_counter += 1;
                                    let _ = tx.send(StreamEvent::ToolCallStart {
                                        id: id.clone(),
                                        name: name.clone(),
                                    });
                                    tool_calls.push(ToolCall {
                                        id: id.clone(),
                                        name,
                                        input: args,
                                    });
                                    let _ = tx.send(StreamEvent::ToolCallEnd { id });
                                }
                            }
                        }

                        if let Some(fr) = candidate["finishReason"].as_str() {
                            stop_reason = match fr {
                                "STOP" => CompletionReason::EndTurn,
                                "MAX_TOKENS" => CompletionReason::MaxTokens,
                                "SAFETY" => CompletionReason::EndTurn,
                                _ => CompletionReason::Unknown,
                            };
                        }
                    }
                }

                if let Some(um) = parsed.get("usageMetadata") {
                    usage.input_tokens = um["promptTokenCount"].as_u64().unwrap_or(0) as u32;
                    usage.output_tokens = um["candidatesTokenCount"].as_u64().unwrap_or(0) as u32;
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
