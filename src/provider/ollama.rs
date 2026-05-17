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

        let data: Value = resp
            .json()
            .await
            .map_err(|e| ForgeError::Provider(e.to_string()))?;

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
                        // Only models with a tool-aware chat template can use
                        // `tools`. Sending tools to a model without one (e.g.
                        // base llama2/phi/gemma:2b) makes Ollama spin without
                        // ever emitting tokens. Allowlist the known-good
                        // families instead of defaulting to true.
                        let lower = name.to_lowercase();
                        let supports_tools = ["llama3.1", "llama3.2", "llama3.3", "llama4",
                                              "qwen2.5", "qwen3", "mistral", "mixtral",
                                              "command-r", "firefunction", "hermes",
                                              "granite", "smollm2", "deepseek", "gpt-oss"]
                            .iter()
                            .any(|tag| lower.contains(tag));
                        ModelInfo {
                            id: name.clone(),
                            name: name.clone(),
                            context_window: ctx,
                            supports_tools,
                            supports_vision: name.contains("vision") || name.contains("llava"),
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

    /// Pick a `num_ctx` that fits the prompt. Ollama defaults to 2048 tokens,
    /// which is far smaller than forge-osh's system prompt + tool definitions —
    /// without overriding this, prompts are silently truncated and the model
    /// often produces nothing or runs forever.
    fn estimate_num_ctx(&self, request: &ChatRequest) -> u32 {
        let mut chars = request.system.as_deref().map(|s| s.len()).unwrap_or(0);
        for m in &request.messages {
            chars += match m {
                Message::User(UserContent::Text(t)) => t.len(),
                Message::Assistant(c) => {
                    let text_len = c.text().map(|t| t.len()).unwrap_or(0);
                    let tools_len: usize = c
                        .tool_calls()
                        .iter()
                        .map(|tc| tc.name.len() + tc.input.to_string().len() + 32)
                        .sum();
                    text_len + tools_len
                }
                Message::Tool(r) => r.content.len() + 32,
            };
        }
        if let Some(tools) = &request.tools {
            for t in tools {
                chars += t.name.len() + t.description.len() + t.parameters.to_string().len() + 64;
            }
        }
        // ~4 chars/token, plus headroom for the response.
        let prompt_tokens = (chars as u32) / 4;
        let needed = prompt_tokens + request.max_tokens + 1024;
        // Clamp into common Ollama-supported sizes. We bias toward larger
        // windows because most Ollama-served models support at least 8k.
        const STEPS: &[u32] = &[8_192, 16_384, 32_768, 65_536, 131_072];
        for &s in STEPS {
            if needed <= s {
                return s;
            }
        }
        *STEPS.last().unwrap()
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
        // Always use Ollama's NATIVE /api/chat endpoint. The OpenAI-compat shim
        // (/v1/chat/completions) buffers the entire response when `tools` is
        // present and frequently appears to hang on local models — the native
        // endpoint streams NDJSON reliably and has supported `tools` since
        // Ollama 0.3.
        let url = format!("{}/api/chat", self.base_url);

        let has_tools = request
            .tools
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false);

        // Estimate the context size we need so Ollama doesn't silently truncate
        // against its 2048-token default. We use a coarse 4-chars-per-token
        // heuristic against the rendered system prompt + messages + tools, then
        // add room for the response, and clamp into a sane range. Most modern
        // local models support 8k–128k; we pick the smallest that fits.
        let num_ctx = self.estimate_num_ctx(&request);

        let messages = self.build_messages(&request.messages, request.system.as_deref());
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
            // keep_alive avoids per-request model unload/reload churn that can
            // make the first byte take 30–60s and feel like a hang.
            "keep_alive": "10m",
            "options": {
                "temperature": request.temperature,
                "num_ctx": num_ctx,
                "num_predict": request.max_tokens as i64,
            },
        });

        if has_tools {
            if let Some(tools) = &request.tools {
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

        let mut stream = response.bytes_stream();
        let mut full_text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason = CompletionReason::EndTurn;
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
                        if !content.is_empty() {
                            full_text.push_str(content);
                            let _ = tx.send(StreamEvent::Token(content.to_string()));
                        }
                    }

                    // Native Ollama tool_calls: array of
                    // { "function": { "name": "...", "arguments": { ... } } }
                    // arguments is a JSON object directly, not a string, and
                    // there is no provider-side `id`. We synthesize one.
                    if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                        for tc in tcs {
                            let f = match tc.get("function") {
                                Some(f) => f,
                                None => continue,
                            };
                            let name = match f.get("name").and_then(|n| n.as_str()) {
                                Some(n) => n.to_string(),
                                None => continue,
                            };
                            let input = f.get("arguments").cloned().unwrap_or_else(|| json!({}));
                            let id = format!("call_{}", tool_calls.len());

                            let _ = tx.send(StreamEvent::ToolCallStart {
                                id: id.clone(),
                                name: name.clone(),
                            });
                            tool_calls.push(ToolCall { id, name, input });
                        }
                    }
                }

                if parsed["done"].as_bool() == Some(true) {
                    if let Some(pt) = parsed["prompt_eval_count"].as_u64() {
                        usage.input_tokens = pt as u32;
                    }
                    if let Some(et) = parsed["eval_count"].as_u64() {
                        usage.output_tokens = et as u32;
                    }
                    if let Some(reason) = parsed["done_reason"].as_str() {
                        stop_reason = match reason {
                            "stop" => CompletionReason::EndTurn,
                            "length" => CompletionReason::MaxTokens,
                            _ => CompletionReason::EndTurn,
                        };
                    }
                }
            }
        }

        if !tool_calls.is_empty() {
            stop_reason = CompletionReason::ToolUse;
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
