use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    User(UserContent),
    Assistant(AssistantContent),
    Tool(ToolResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserContent {
    Text(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssistantContent {
    Text(String),
    ToolUse(Vec<ToolCall>),
    Mixed {
        text: String,
        tool_calls: Vec<ToolCall>,
    },
}

impl AssistantContent {
    pub fn text(&self) -> Option<&str> {
        match self {
            AssistantContent::Text(t) => Some(t.as_str()),
            AssistantContent::Mixed { text, .. } => Some(text.as_str()),
            AssistantContent::ToolUse(_) => None,
        }
    }

    pub fn tool_calls(&self) -> &[ToolCall] {
        match self {
            AssistantContent::ToolUse(calls) => calls,
            AssistantContent::Mixed { tool_calls, .. } => tool_calls,
            AssistantContent::Text(_) => &[],
        }
    }
}

// ---------------------------------------------------------------------------
// Tool calls & results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Chat request / response
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub system: Option<String>,
    pub stop_sequences: Vec<String>,
}

impl Default for ChatRequest {
    fn default() -> Self {
        Self {
            model: String::new(),
            messages: Vec::new(),
            tools: None,
            max_tokens: 4096,
            temperature: 0.7,
            system: None,
            stop_sequences: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: AssistantContent,
    pub usage: Usage,
    pub model: String,
    pub stop_reason: CompletionReason,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
}

impl Usage {
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompletionReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Unknown,
}

// ---------------------------------------------------------------------------
// Streaming events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Token(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        arguments_delta: String,
    },
    ToolCallEnd {
        id: String,
    },
    Usage(Usage),
    Done(CompletionReason),
    Error(String),
}

// ---------------------------------------------------------------------------
// Model info
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub provider_id: String,
}

impl ModelInfo {
    pub fn cost_for(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input = (input_tokens as f64 / 1_000_000.0) * self.input_cost_per_million;
        let output = (output_tokens as f64 / 1_000_000.0) * self.output_cost_per_million;
        input + output
    }
}

// ---------------------------------------------------------------------------
// Permission
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PermissionLevel {
    ReadOnly,
    Mutating,
    Destructive,
    Network,
    Shell,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionResponse {
    Allow,
    Deny,
    AlwaysAllow,
    TrustMode,
}

// ---------------------------------------------------------------------------
// Tool context
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub working_dir: std::path::PathBuf,
    pub home_dir: std::path::PathBuf,
    pub session_id: String,
    pub trust_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    pub metadata: Option<serde_json::Value>,
}

impl ToolOutput {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            metadata: None,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            metadata: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_total() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        assert_eq!(usage.total_tokens(), 150);
    }

    #[test]
    fn test_model_cost() {
        let model = ModelInfo {
            id: "test".into(),
            name: "Test".into(),
            context_window: 128000,
            supports_tools: true,
            supports_vision: false,
            input_cost_per_million: 3.0,
            output_cost_per_million: 15.0,
            provider_id: "test".into(),
        };
        let cost = model.cost_for(1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn test_assistant_content_text() {
        let content = AssistantContent::Text("hello".into());
        assert_eq!(content.text(), Some("hello"));
        assert!(content.tool_calls().is_empty());
    }

    #[test]
    fn test_tool_output_success() {
        let out = ToolOutput::success("done");
        assert!(!out.is_error);
        assert_eq!(out.content, "done");
    }

    #[test]
    fn test_tool_output_error() {
        let out = ToolOutput::error("failed");
        assert!(out.is_error);
    }
}
