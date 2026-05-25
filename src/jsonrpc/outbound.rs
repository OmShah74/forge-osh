//! Events emitted by the agent (Rust) over stdout to the extension/IDE.
//!
//! Wire format: one JSON object per line (NDJSON). Every variant carries a
//! `type` discriminator (snake_case). New variants are additive — clients
//! must ignore unknown ones — and breaking changes bump `JSONRPC_VERSION`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundEvent {
    /// Sent exactly once after process spawn + provider router is ready.
    /// Clients gate on this before they send their first command.
    Ready {
        jsonrpc_version: u32,
        forge_version: String,
        provider: String,
        model: String,
    },
    AssistantTextDelta {
        text: String,
    },
    AssistantTextEnd,
    ThinkingStart,
    ThinkingDelta {
        text: String,
    },
    ThinkingEnd,
    ToolCallStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolCallEnd {
        id: String,
        output_excerpt: String,
        is_error: bool,
    },
    PermissionRequest {
        id: String,
        tool: String,
        summary: String,
        level: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff_preview: Option<String>,
    },
    DiffPreview {
        tool_call_id: String,
        path: String,
        unified_diff: String,
    },
    Usage {
        input: u32,
        output: u32,
        cache_read: u32,
        cache_write: u32,
        cost_usd: f64,
    },
    Compaction {
        stage: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    GoalEvent {
        goal_id: String,
        payload: serde_json::Value,
    },
    SessionLoaded {
        id: String,
        message_count: u32,
    },
    SystemMessage {
        text: String,
        kind: String, // "info" | "warn" | "error"
    },
    /// Indicates the agent has finished the current turn. `reason` is one of
    /// `end_turn`, `max_iterations`, `cancelled`, `error`.
    Done {
        reason: String,
    },
    Error {
        message: String,
    },
}
