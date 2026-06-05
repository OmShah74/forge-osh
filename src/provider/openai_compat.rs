use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::config::models;
use crate::error::{ForgeError, Result};
use crate::session::tokens::TokenCounter;
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

    fn model_info_for(&self, model: &str) -> ModelInfo {
        models::models_for_provider(&self.provider_id)
            .into_iter()
            .find(|m| m.id == model)
            .unwrap_or_else(|| ModelInfo {
                id: model.to_string(),
                name: model.to_string(),
                context_window: self.model_info.context_window,
                supports_tools: self.model_info.supports_tools,
                supports_vision: self.model_info.supports_vision,
                input_cost_per_million: self.model_info.input_cost_per_million,
                output_cost_per_million: self.model_info.output_cost_per_million,
                provider_id: self.provider_id.clone(),
            })
    }

    fn supports_tools_for(&self, model: &str) -> bool {
        self.model_info_for(model).supports_tools
    }

    /// OpenAI's GPT-5 family and the o-series (o1/o3/o4/o5) deprecated
    /// `max_tokens` — they only accept `max_completion_tokens`. Sending
    /// `max_tokens` returns a 400 like the one the user hit:
    ///   "Unsupported parameter: 'max_tokens' is not supported with this
    ///    model. Use 'max_completion_tokens' instead."
    /// We only switch params for the OpenAI provider — third-party OpenAI-
    /// compatible servers (Groq, Together, etc.) still expect `max_tokens`
    /// even when they host a model with a similar name.
    fn uses_max_completion_tokens(&self, model: &str) -> bool {
        if self.provider_id != "openai" {
            return false;
        }
        let m = model.to_ascii_lowercase();
        m.starts_with("gpt-5")
            || m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
            || m.starts_with("o5")
    }

    /// Reasoning models (o-series and gpt-5) reject custom `temperature`
    /// values — only the server default is allowed. We omit the field
    /// entirely for these models.
    fn omits_temperature(&self, model: &str) -> bool {
        if self.provider_id != "openai" {
            return false;
        }
        let m = model.to_ascii_lowercase();
        m.starts_with("gpt-5")
            || m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
            || m.starts_with("o5")
    }

    /// Identify the underlying provider this request will route to.
    /// For OpenRouter we parse the model id prefix; for direct providers we
    /// return their own id. Drives the rest of the cache strategy.
    fn underlying_provider(&self, model: &str) -> &'static str {
        if self.provider_id != "openrouter" {
            return match self.provider_id.as_str() {
                "openai" => "openai",
                "deepseek" => "deepseek",
                "mistral" => "mistral",
                "groq" => "groq",
                "grok" => "grok",
                "together" => "together",
                "fireworks" => "fireworks",
                "perplexity" => "perplexity",
                "cohere" => "cohere",
                _ => "custom",
            };
        }
        let m = model.to_ascii_lowercase();
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
        } else if m.starts_with("mistralai/") || m.starts_with("mistral/") {
            "mistral"
        } else if m.starts_with("meta-llama/") || m.starts_with("meta/") {
            "meta"
        } else if m.starts_with("qwen/") || m.starts_with("alibaba/") {
            "qwen"
        } else if m.starts_with("x-ai/") || m.starts_with("xai/") {
            "grok"
        } else if m.starts_with("perplexity/") {
            "perplexity"
        } else if m.starts_with("cohere/") {
            "cohere"
        } else {
            "openrouter-other"
        }
    }

    /// Inject Anthropic-style `cache_control` markers when serving Claude
    /// via an OpenAI-format wire. Required for OpenRouter → anthropic/*;
    /// harmful elsewhere (some strict compat servers 400 on unknown fields).
    fn inject_anthropic_cache_markers(&self, model: &str) -> bool {
        self.underlying_provider(model) == "anthropic"
    }

    /// `prompt_cache_key` is honored by OpenAI and DeepSeek (and forwarded
    /// to them by OpenRouter). Omitted elsewhere to avoid 400s.
    fn supports_prompt_cache_key(&self, model: &str) -> bool {
        matches!(self.underlying_provider(model), "openai" | "deepseek")
    }

    /// `prompt_cache_retention = "24h"` is supported by gpt-5.x and gpt-4.1
    /// on the OpenAI backend, whether reached directly or via OpenRouter.
    fn supports_extended_cache(&self, model: &str) -> bool {
        if self.underlying_provider(model) != "openai" {
            return false;
        }
        let m = model.to_ascii_lowercase();
        let m = m.strip_prefix("openai/").unwrap_or(&m);
        m.starts_with("gpt-5") || m.starts_with("gpt-4.1")
    }

    /// Stable SHA-256-derived key so successive turns from the same chat
    /// land on the same OpenAI/DeepSeek cache shard.
    fn compute_prompt_cache_key(&self, request: &ChatRequest) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(request.model.as_bytes());
        hasher.update(b"\x00");
        if let Some(s) = &request.system {
            hasher.update(s.as_bytes());
        }
        hasher.update(b"\x00");
        if let Some(tools) = &request.tools {
            let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            names.sort();
            for n in names {
                hasher.update(n.as_bytes());
                hasher.update(b",");
            }
        }
        let digest = hasher.finalize();
        let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
        format!("forge-osh-{hex}")
    }

    /// Build the messages array. When `cache_last_msg` is true the last
    /// message's content is emitted as a one-element array with a
    /// `cache_control: {type:"ephemeral"}` marker on it — the OpenAI-format
    /// dialect Anthropic understands. This is only ever set when the
    /// underlying provider is Anthropic (i.e. OpenRouter → anthropic/*).
    fn build_messages(&self, messages: &[Message], cache_last_msg: bool) -> Vec<Value> {
        let last_idx = messages.len().saturating_sub(1);
        messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let mark_this = cache_last_msg && i == last_idx;
                match msg {
                    Message::User(UserContent::Text(text)) => {
                        if mark_this {
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
                    Message::User(UserContent::Multimodal(parts)) => {
                        // OpenAI-compatible content-parts array: text parts +
                        // image_url parts (data URIs), in interleaved order.
                        let mut content: Vec<serde_json::Value> = Vec::with_capacity(parts.len());
                        for part in parts {
                            match part {
                                UserPart::Text(t) => {
                                    if !t.is_empty() {
                                        content.push(json!({"type": "text", "text": t}));
                                    }
                                }
                                UserPart::Image(img) => {
                                    content.push(json!({
                                        "type": "image_url",
                                        "image_url": { "url": img.data_url() }
                                    }));
                                }
                            }
                        }
                        json!({"role": "user", "content": content})
                    }
                    Message::Assistant(content) => {
                        let mut msg = json!({"role": "assistant"});
                        if let Some(text) = content.text() {
                            if mark_this {
                                msg["content"] = json!([{
                                    "type": "text",
                                    "text": text,
                                    "cache_control": {"type": "ephemeral"}
                                }]);
                            } else {
                                msg["content"] = json!(text);
                            }
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
                        if mark_this {
                            // OpenAI tool-message format keeps `content` as a
                            // string; OpenRouter accepts the array form with
                            // cache_control for Anthropic routes.
                            json!({
                                "role": "tool",
                                "tool_call_id": result.tool_use_id,
                                "content": [{
                                    "type": "text",
                                    "text": result.content,
                                    "cache_control": {"type": "ephemeral"}
                                }]
                            })
                        } else {
                            json!({
                                "role": "tool",
                                "tool_call_id": result.tool_use_id,
                                "content": result.content
                            })
                        }
                    }
                }
            })
            .collect()
    }

    /// Build tool definitions. When `cache_last_tool` is true, attach an
    /// ephemeral cache_control marker to the LAST tool (Anthropic dialect
    /// over OpenAI format — caches the full tools array prefix).
    fn build_tools(&self, tools: &[ToolDefinition], cache_last_tool: bool) -> Vec<Value> {
        let last_idx = tools.len().saturating_sub(1);
        tools
            .iter()
            .enumerate()
            .map(|(i, tool)| {
                let mut v = json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters
                    }
                });
                if cache_last_tool && i == last_idx {
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

        // Prompt-cache strategy.
        // - OpenRouter → anthropic/* needs Anthropic cache_control markers
        //   injected in the OpenAI message format (up to 4 ephemeral
        //   breakpoints; we use 3: system, last tool, last message).
        // - OpenAI / DeepSeek (direct OR via OpenRouter) get a stable
        //   prompt_cache_key for shard routing.
        // - Gemini/GLM/etc. auto-cache server-side — no client action.
        let anthro_markers = self.inject_anthropic_cache_markers(&request.model);
        let big_system = request
            .system
            .as_ref()
            .map(|s| TokenCounter::count_text(s) >= 1024)
            .unwrap_or(false);
        let has_tools = request
            .tools
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false);
        let big_history = TokenCounter::count_messages(&request.messages) >= 2048;
        let mark_msg = anthro_markers && big_history;
        let mark_tools = anthro_markers && has_tools;
        let mark_system = anthro_markers && big_system;

        let mut body = json!({
            "model": request.model,
            "messages": self.build_messages(&request.messages, mark_msg),
            "stream": true,
            // Ask the server for a final usage chunk so we can track tokens
            // and cost for every OpenAI-compatible provider.
            "stream_options": {"include_usage": true},
        });

        if self.uses_max_completion_tokens(&request.model) {
            body["max_completion_tokens"] = json!(request.max_tokens);
        } else {
            body["max_tokens"] = json!(request.max_tokens);
        }

        if !self.omits_temperature(&request.model) {
            body["temperature"] = json!(request.temperature);
        }

        // Inject system message at the start if provided.
        // For Anthropic routes we send the content-array form with an
        // ephemeral cache_control marker so the system prompt is cached.
        if let Some(system) = &request.system {
            let system_value = if mark_system {
                json!([{
                    "type": "text",
                    "text": system,
                    "cache_control": {"type": "ephemeral"}
                }])
            } else {
                json!(system)
            };
            if let Some(msgs) = body["messages"].as_array_mut() {
                msgs.insert(0, json!({"role": "system", "content": system_value}));
            }
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() && self.supports_tools_for(&request.model) {
                body["tools"] = json!(self.build_tools(tools, mark_tools));
            }
        }

        if !request.stop_sequences.is_empty() {
            body["stop"] = json!(request.stop_sequences);
        }

        // prompt_cache_key (OpenAI / DeepSeek, direct or via OpenRouter)
        if self.supports_prompt_cache_key(&request.model) {
            body["prompt_cache_key"] = json!(self.compute_prompt_cache_key(&request));
            if self.supports_extended_cache(&request.model) {
                body["prompt_cache_retention"] = json!("24h");
            }
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

                // Usage in the final event (stream_options.include_usage=true).
                //
                // Field shape across servers we care about:
                // - OpenAI / DeepSeek direct: prompt_tokens (total),
                //   prompt_tokens_details.cached_tokens (subset, ⊆ prompt).
                // - OpenRouter → anthropic/*: also emits
                //   prompt_tokens_details.cached_tokens (read) AND
                //   cache_creation_input_tokens or prompt_tokens_details.
                //   cache_creation_tokens (write). We try both keys.
                // - Gemini direct lives in a different code path.
                // - OpenRouter normalizes everything; missing fields default to 0.
                //
                // We always normalize Usage so that:
                //   input_tokens     = uncached new tokens
                //   cache_read_tokens  = subset already in the cache
                //   cache_write_tokens = subset freshly populating the cache
                if let Some(u) = parsed.get("usage") {
                    let total_prompt = u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                    usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0) as u32;
                    let details = u.get("prompt_tokens_details");
                    let cached = details
                        .and_then(|d| d.get("cached_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    let cache_write = u
                        .get("cache_creation_input_tokens")
                        .or_else(|| details.and_then(|d| d.get("cache_creation_tokens")))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    let mut uncached = total_prompt;
                    if cached > 0 && cached <= uncached {
                        uncached -= cached;
                        usage.cache_read_tokens = Some(cached);
                    }
                    if cache_write > 0 && cache_write <= uncached {
                        uncached -= cache_write;
                        usage.cache_write_tokens = Some(cache_write);
                    }
                    usage.input_tokens = uncached;
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
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    name,
                    input,
                });
                let _ = tx.send(StreamEvent::ToolCallEnd { id });
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
