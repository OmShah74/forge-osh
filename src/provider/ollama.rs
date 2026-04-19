use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::error::{ForgeError, Result};
use crate::types::*;

use super::Provider;

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| ForgeError::Provider(format!("Failed to create HTTP client: {e}")))?;

        Ok(Self {
            client,
            base_url,
            model,
        })
    }

    /// Check if Ollama is running
    pub async fn detect(base_url: &str) -> bool {
        let client = Client::new();
        client
            .get(format!("{base_url}/api/tags"))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Fetch available models from Ollama
    pub async fn fetch_models(base_url: &str) -> Result<Vec<ModelInfo>> {
        let client = Client::new();
        let resp = client
            .get(format!("{base_url}/api/tags"))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;

        let data: Value = resp.json().await.map_err(|e| ForgeError::Provider(e.to_string()))?;

        let models = data["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|m| {
                        let name = m["name"].as_str().unwrap_or("unknown").to_string();
                        let _size = m["size"].as_u64().unwrap_or(0);
                        // Rough context window estimation based on model
                        let ctx = if name.contains("llama") {
                            131_072
                        } else {
                            8_192
                        };
                        ModelInfo {
                            id: name.clone(),
                            name: name.clone(),
                            context_window: ctx,
                            supports_tools: true,
                            supports_vision: name.contains("vision")
                                || name.contains("llava"),
                            input_cost_per_million: 0.0,
                            output_cost_per_million: 0.0,
                            provider_id: "ollama".to_string(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    fn build_messages(&self, messages: &[Message], system: Option<&str>) -> Vec<Value> {
        let mut result = Vec::new();
        if let Some(sys) = system {
            result.push(json!({"role": "system", "content": sys}));
        }
        for msg in messages {
            match msg {
                Message::User(UserContent::Text(text)) => {
                    result.push(json!({"role": "user", "content": text}));
                }
                Message::Assistant(content) => {
                    let mut m = json!({"role": "assistant"});
                    if let Some(text) = content.text() {
                        m["content"] = json!(text);
                    }
                    let calls = content.tool_calls();
                    if !calls.is_empty() {
                        m["tool_calls"] = json!(calls
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
                    result.push(m);
                }
                Message::Tool(r) => {
                    result.push(json!({
                        "role": "tool",
                        "tool_call_id": r.tool_use_id,
                        "content": r.content
                    }));
                }
            }
        }
        result
    }

    fn build_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect()
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }

    fn name(&self) -> &str {
        "Ollama"
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Self::fetch_models(&self.base_url).await
    }

    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<ChatResponse> {
        // Use OpenAI-compatible endpoint for tool support
        let has_tools = request.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
        let url = if has_tools {
            format!("{}/v1/chat/completions", self.base_url)
        } else {
            format!("{}/api/chat", self.base_url)
        };

        if has_tools {
            // OpenAI-compatible path
            let mut body = json!({
                "model": request.model,
                "messages": self.build_messages(&request.messages, request.system.as_deref()),
                "stream": true,
                "stream_options": {"include_usage": true},
            });

            if let Some(tools) = &request.tools {
                if !tools.is_empty() {
                    body["tools"] = json!(self.build_tools(tools));
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
                return Err(ForgeError::api(status, text));
            }

            // Parse SSE (same as OpenAI compat)
            let mut stream = response.bytes_stream();
            let mut full_text = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let usage = Usage::default();
            let mut stop_reason = CompletionReason::EndTurn;
            let mut buffer = String::new();
            let mut tc_map: std::collections::HashMap<u32, (String, String, String)> =
                std::collections::HashMap::new();

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if line.is_empty() || line == "data: [DONE]" {
                        continue;
                    }
                    let data = match line.strip_prefix("data: ") {
                        Some(d) => d,
                        None => continue,
                    };
                    let parsed: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    if let Some(choice) = parsed["choices"].as_array().and_then(|c| c.first()) {
                        if let Some(delta) = choice.get("delta") {
                            if let Some(c) = delta["content"].as_str() {
                                full_text.push_str(c);
                                let _ = tx.send(StreamEvent::Token(c.to_string()));
                            }
                            if let Some(tcs) = delta["tool_calls"].as_array() {
                                for tc in tcs {
                                    let idx = tc["index"].as_u64().unwrap_or(0) as u32;
                                    let entry = tc_map.entry(idx).or_default();
                                    if let Some(id) = tc["id"].as_str() {
                                        entry.0 = id.to_string();
                                    }
                                    if let Some(f) = tc.get("function") {
                                        if let Some(n) = f["name"].as_str() {
                                            entry.1 = n.to_string();
                                            let _ = tx.send(StreamEvent::ToolCallStart {
                                                id: entry.0.clone(),
                                                name: n.to_string(),
                                            });
                                        }
                                        if let Some(a) = f["arguments"].as_str() {
                                            entry.2.push_str(a);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(fr) = choice["finish_reason"].as_str() {
                            stop_reason = match fr {
                                "stop" => CompletionReason::EndTurn,
                                "tool_calls" => CompletionReason::ToolUse,
                                "length" => CompletionReason::MaxTokens,
                                _ => CompletionReason::Unknown,
                            };
                        }
                    }
                }
            }

            let mut indices: Vec<u32> = tc_map.keys().cloned().collect();
            indices.sort();
            for idx in indices {
                if let Some((id, name, args)) = tc_map.remove(&idx) {
                    let input: Value = serde_json::from_str(&args).unwrap_or(json!({}));
                    tool_calls.push(ToolCall { id, name, input });
                }
            }

            let _ = tx.send(StreamEvent::Usage(usage.clone()));
            let _ = tx.send(StreamEvent::Done(stop_reason.clone()));

            let content = if !tool_calls.is_empty() && !full_text.is_empty() {
                AssistantContent::Mixed { text: full_text, tool_calls }
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
        } else {
            // Native Ollama API (newline-delimited JSON)
            let mut body = json!({
                "model": request.model,
                "messages": self.build_messages(&request.messages, request.system.as_deref()),
                "stream": true,
            });

            if let Some(opts) = body.as_object_mut() {
                opts.insert(
                    "options".to_string(),
                    json!({"temperature": request.temperature}),
                );
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
                return Err(ForgeError::api(status, text));
            }

            let mut stream = response.bytes_stream();
            let mut full_text = String::new();
            let mut usage = Usage::default();
            let stop_reason = CompletionReason::EndTurn;
            let mut buffer = String::new();

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    let parsed: Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    if let Some(msg) = parsed.get("message") {
                        if let Some(content) = msg["content"].as_str() {
                            full_text.push_str(content);
                            let _ = tx.send(StreamEvent::Token(content.to_string()));
                        }
                    }

                    if parsed["done"].as_bool() == Some(true) {
                        if let Some(pt) = parsed["prompt_eval_count"].as_u64() {
                            usage.input_tokens = pt as u32;
                        }
                        if let Some(et) = parsed["eval_count"].as_u64() {
                            usage.output_tokens = et as u32;
                        }
                    }
                }
            }

            let _ = tx.send(StreamEvent::Usage(usage.clone()));
            let _ = tx.send(StreamEvent::Done(stop_reason.clone()));

            Ok(ChatResponse {
                content: AssistantContent::Text(full_text),
                usage,
                model: request.model,
                stop_reason,
            })
        }
    }

    fn supports_tools(&self) -> bool {
        true // Depends on model, assume yes for recent models
    }

    fn supports_vision(&self) -> bool {
        self.model.contains("vision") || self.model.contains("llava")
    }

    fn context_window(&self) -> u32 {
        131_072
    }

    fn input_cost_per_million(&self) -> f64 {
        0.0
    }

    fn output_cost_per_million(&self) -> f64 {
        0.0
    }
}
