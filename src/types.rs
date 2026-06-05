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
    /// Plain text (the overwhelmingly common case).
    Text(String),
    /// An ordered, interleaved sequence of text and image parts. Order is
    /// significant: it preserves exactly where each pasted image sat in the
    /// user's input string so the model receives images in the right place
    /// relative to the surrounding words (crucial for multi-image prompts).
    Multimodal(Vec<UserPart>),
}

/// One ordered piece of a multimodal user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserPart {
    Text(String),
    Image(ImageRef),
}

/// A base64-encoded image attached to a user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRef {
    /// MIME type, e.g. `image/png` or `image/jpeg`.
    pub media_type: String,
    /// Base64-encoded image bytes (no `data:` prefix).
    pub data: String,
}

impl ImageRef {
    /// `data:<media_type>;base64,<data>` form (used by OpenAI-compatible APIs).
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.data)
    }
}

impl UserContent {
    /// Flatten to plain text for token counting, display fallbacks, and any
    /// provider/path that does not handle images. Images become a compact
    /// `[image]` placeholder so positions are still legible.
    pub fn to_text(&self) -> String {
        match self {
            UserContent::Text(t) => t.clone(),
            UserContent::Multimodal(parts) => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        UserPart::Text(t) => out.push_str(t),
                        UserPart::Image(_) => out.push_str("[image]"),
                    }
                }
                out
            }
        }
    }

    /// Borrowed view of every image part, in order.
    pub fn images(&self) -> Vec<&ImageRef> {
        match self {
            UserContent::Text(_) => Vec::new(),
            UserContent::Multimodal(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    UserPart::Image(img) => Some(img),
                    UserPart::Text(_) => None,
                })
                .collect(),
        }
    }

    pub fn has_images(&self) -> bool {
        matches!(self, UserContent::Multimodal(parts) if parts.iter().any(|p| matches!(p, UserPart::Image(_))))
    }
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
    fn default() -> Self {
        ThinkingConfig::Disabled
    }
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
    /// Provider streamed a chunk of "extended thinking" / reasoning content
    /// distinct from the visible answer. Providers without thinking support
    /// simply never emit this.
    ThinkingDelta(String),
    /// All thinking blocks for this assistant turn have been received.
    ThinkingDone,
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, arguments_delta: String },
    ToolCallEnd { id: String },
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
    fn default() -> Self {
        PermissionMode::Default
    }
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
            "accept-edits" | "accept_edits" | "acceptedits" | "accept" => {
                Some(PermissionMode::AcceptEdits)
            }
            "bypass" | "trust" | "yolo" => Some(PermissionMode::Bypass),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool context
// ---------------------------------------------------------------------------

/// One incremental chunk of stdout/stderr produced by a long-running tool
/// (currently `bash` and `powershell`).  Wired through the AgentLoop's
/// chunk channel and surfaced on the JSON-RPC bridge as `tool_output_delta`,
/// letting IDE webviews show live tail output instead of waiting for the
/// final `tool_call_end` buffered excerpt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutputChunk {
    pub tool_call_id: String,
    /// `"stdout"` or `"stderr"`.
    pub stream: String,
    pub text: String,
}

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
    /// When true, mutating file tools must pass through an explicit diff
    /// review prompt before they touch disk. Bypass/trust mode still means
    /// "I know what I am doing" and skips the review gate.
    pub diff_review: bool,
    /// Optional shared file-state cache. Tools that mutate files should call
    /// `check_unchanged` through this before writing; ReadOnly file tools
    /// should `record_read` after a successful read. Absent in tests that
    /// synthesise minimal contexts — tools must degrade to "no cache" rather
    /// than panicking.
    #[doc(hidden)]
    pub file_cache: Option<std::sync::Arc<crate::session::FileStateCache>>,
    /// Skill-scoped constraints and overrides currently active for this turn chain.
    #[doc(hidden)]
    pub active_skill_scope: Option<crate::skills::ActiveSkillScope>,
    /// Shared skill registry. Absent in minimal test contexts — tools that
    /// need skill lookup must fall back to loading from disk.
    #[doc(hidden)]
    pub skill_registry: Option<crate::skills::SharedSkillRegistry>,
    /// Channel for streaming intermediate tool output (e.g. live stdout from
    /// a long-running `bash`/`powershell` command). Set by the AgentLoop when
    /// a chunk-forwarder is wired up (the JSON-RPC bridge always does this;
    /// the TUI leaves it `None` and tools fall back to buffered output).
    pub output_chunk_tx: Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>,
    /// The id of the tool call currently being executed. Set by the executor
    /// immediately before `Tool::execute` so streaming tools can tag deltas
    /// with the matching id from `ToolCallStart`.
    pub tool_call_id: Option<String>,
    /// Live shared team blackboard. Present only for workers running as part of
    /// a team / swarm (set by the coordinator or the `spawn_team` runner);
    /// `None` for the normal single-agent loop. The `team_post` / `team_read`
    /// tools use it for peer-to-peer coordination without the orchestrator.
    #[doc(hidden)]
    pub team_blackboard: Option<crate::agent::team_bus::SharedBlackboard>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("working_dir", &self.working_dir)
            .field("home_dir", &self.home_dir)
            .field("session_id", &self.session_id)
            .field("trust_mode", &self.trust_mode)
            .field("permission_mode", &self.permission_mode)
            .field("diff_review", &self.diff_review)
            .field("file_cache", &self.file_cache.as_ref().map(|c| c.len()))
            .field(
                "active_skill_scope",
                &self
                    .active_skill_scope
                    .as_ref()
                    .map(|s| s.skill_name.as_str()),
            )
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
            diff_review: true,
            file_cache: None,
            active_skill_scope: None,
            skill_registry: None,
            output_chunk_tx: None,
            tool_call_id: None,
            team_blackboard: None,
        }
    }

    /// Convenience: send one stdout/stderr chunk through `output_chunk_tx`
    /// tagged with the current `tool_call_id`. No-op if either is unset
    /// (TUI surface, unit tests). Errors on the channel are dropped — the
    /// chunk is best-effort and never blocks tool execution.
    pub fn emit_output_chunk(&self, stream: &str, text: impl Into<String>) {
        if let (Some(tx), Some(id)) = (&self.output_chunk_tx, &self.tool_call_id) {
            let _ = tx.send(ToolOutputChunk {
                tool_call_id: id.clone(),
                stream: stream.to_string(),
                text: text.into(),
            });
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
