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

/// Extended thinking configuration. Providers that do not support thinking
/// (OpenAI, Ollama) simply ignore this field; Anthropic translates
/// `Budget(n)` into its `thinking = { type: enabled, budget_tokens: n }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingConfig {
    /// Extended thinking off.
    Disabled,
    /// Let the model/provider decide the budget.
    Enabled,
    /// Ask the model to reserve at most `tokens` for extended thinking.
    Budget { tokens: u32 },
}

impl Default for ThinkingConfig {
    fn default() -> Self { ThinkingConfig::Disabled }
}

impl ThinkingConfig {
    pub fn is_enabled(&self) -> bool {
        !matches!(self, ThinkingConfig::Disabled)
    }

    /// The explicit budget in tokens, if any.
    pub fn budget(&self) -> Option<u32> {
        match self {
            ThinkingConfig::Budget { tokens } => Some(*tokens),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub system: Option<String>,
    pub stop_sequences: Vec<String>,
    pub thinking: ThinkingConfig,
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
            thinking: ThinkingConfig::Disabled,
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

/// How the permission system should behave for this session.
///
/// - `Default`: standard behaviour — ReadOnly tools auto-allow, others prompt
///   unless a stored PermissionStore rule says otherwise.
/// - `Plan`: the agent may only use ReadOnly tools; any Mutating/Destructive/
///   Shell/Network call is denied automatically. Used by `enter_plan_mode`.
/// - `AcceptEdits`: ReadOnly and Mutating tools auto-allow; Destructive / Shell
///   / Network still prompt. Matches Claude Code's "accept edits" mode.
/// - `Bypass`: every tool is auto-allowed. Dangerous — equivalent to the old
///   `trust_mode = true` flag, preserved for backwards compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    Default,
    Plan,
    AcceptEdits,
    Bypass,
}

impl Default for PermissionMode {
    fn default() -> Self { PermissionMode::Default }
}

impl PermissionMode {
    pub fn as_label(&self) -> &'static str {
        match self {
            PermissionMode::Default => "default",
            PermissionMode::Plan => "plan",
            PermissionMode::AcceptEdits => "accept-edits",
            PermissionMode::Bypass => "bypass",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "default" | "normal" => Some(PermissionMode::Default),
            "plan" => Some(PermissionMode::Plan),
            "accept-edits" | "accept_edits" | "acceptedits" | "accept" => Some(PermissionMode::AcceptEdits),
            "bypass" | "trust" | "yolo" => Some(PermissionMode::Bypass),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool context
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ToolContext {
    pub working_dir: std::path::PathBuf,
    pub home_dir: std::path::PathBuf,
    pub session_id: String,
    /// Kept for backwards compatibility with call sites that only care
    /// about "is everything blessed?" (equivalent to `mode == Bypass`).
    pub trust_mode: bool,
    /// Fine-grained permission mode. `Bypass` implies `trust_mode == true`.
    pub permission_mode: PermissionMode,
    /// Optional shared file-state cache. Tools that mutate files should call
    /// `check_unchanged` through this before writing; ReadOnly file tools
    /// should `record_read` after a successful read. Absent in tests that
    /// synthesise minimal contexts — tools must degrade to "no cache" rather
    /// than panicking.
    #[doc(hidden)]
    pub file_cache: Option<std::sync::Arc<crate::session::FileStateCache>>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("working_dir", &self.working_dir)
            .field("home_dir", &self.home_dir)
            .field("session_id", &self.session_id)
            .field("trust_mode", &self.trust_mode)
            .field("permission_mode", &self.permission_mode)
            .field("file_cache", &self.file_cache.as_ref().map(|c| c.len()))
            .finish()
    }
}

impl ToolContext {
    pub fn new(working_dir: std::path::PathBuf, session_id: String, mode: PermissionMode) -> Self {
        Self {
            working_dir,
            home_dir: dirs::home_dir().unwrap_or_default(),
            session_id,
            trust_mode: mode == PermissionMode::Bypass,
            permission_mode: mode,
            file_cache: None,
        }
    }
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
