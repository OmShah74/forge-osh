//! Commands sent by the extension/IDE over stdin to the agent.
//!
//! Wire format: NDJSON, one command per line. Unknown variants are rejected
//! by serde but the reader catches the parse error and emits a SystemMessage
//! warning rather than crashing the agent loop.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundCommand {
    UserMessage {
        text: String,
        #[serde(default)]
        context_blocks: Vec<ContextBlock>,
    },
    /// Response to an OutboundEvent::PermissionRequest.
    /// `response` is one of "allow" | "deny" | "always_allow" | "trust".
    PermissionResponse {
        id: String,
        response: String,
    },
    Cancel,
    Compact {
        #[serde(default)]
        keep_last: Option<u32>,
    },
    SwitchModel {
        provider: String,
        model: String,
    },
    LoadSession {
        name: String,
    },
    NewSession {
        #[serde(default)]
        name: Option<String>,
    },
    SpawnGoal {
        objective: String,
        #[serde(default)]
        spec_path: Option<String>,
    },
    GoalControl {
        goal_id: String,
        action: String,
    },
    InvokeSkill {
        name: String,
        #[serde(default)]
        args: Option<String>,
    },
    Configure {
        key: String,
        value: serde_json::Value,
    },
    /// Round-trip latency / liveness probe. Server responds with a
    /// SystemMessage { kind: "info", text: "pong" }.
    Ping,

    // ── Session admin ───────────────────────────────────────────────────
    /// Revert the last file-mutation tool call. No-op if file_history is
    /// empty. Maps to the TUI's `/undo`.
    Undo,
    /// Rename the current session. Maps to TUI `/rename`.
    RenameSession { name: String },
    /// Force a checkpoint save right now. Maps to TUI `/save`.
    SaveSession,

    // ── Goals ───────────────────────────────────────────────────────────
    /// Return a one-shot status snapshot for a specific goal id.
    GoalStatus { goal_id: String },

    // ── Skills ──────────────────────────────────────────────────────────
    /// `action` ∈ list | show | reload | delete. `name` is required for
    /// show + delete.
    SkillCommand {
        action: String,
        #[serde(default)]
        name: Option<String>,
    },

    // ── Permission rules ────────────────────────────────────────────────
    /// `action` ∈ list | add_allow | add_deny | remove. For add_*: `tool`
    /// + `pattern` required. For remove: `index` required.
    PermissionRules {
        action: String,
        #[serde(default)]
        tool: Option<String>,
        #[serde(default)]
        pattern: Option<String>,
        #[serde(default)]
        index: Option<usize>,
    },

    // ── MCP ─────────────────────────────────────────────────────────────
    /// `action` ∈ list | connect | disconnect | enable | disable.
    McpCommand {
        action: String,
        #[serde(default)]
        server: Option<String>,
    },

    // ── Code graph ──────────────────────────────────────────────────────
    /// Build the semantic graph in the background. `rebuild=true` forces
    /// rebuild even if an artifact already exists.
    BuildGraph {
        #[serde(default)]
        rebuild: bool,
    },

    // ── Hooks ───────────────────────────────────────────────────────────
    /// Re-read `hooks.toml` from disk. The agent loop reloads hooks at
    /// the start of every turn anyway, but this gives the IDE an
    /// explicit reload affordance for parity with the TUI.
    HooksReload,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContextBlock {
    /// "file" | "selection" | "diagnostic" | "url"
    pub kind: String,
    pub label: String,
    pub content: String,
    #[serde(default)]
    pub path: Option<String>,
    /// [start_line, start_col, end_line, end_col]
    #[serde(default)]
    pub range: Option<[u32; 4]>,
}

impl ContextBlock {
    /// Render the block as a markdown fence so it can be appended to the
    /// user's message text without losing structure.
    pub fn render(&self) -> String {
        let header = match self.path.as_deref() {
            Some(p) => match self.range {
                Some([s, _, e, _]) => format!("{} ({}:L{}-L{})", self.label, p, s + 1, e + 1),
                None => format!("{} ({})", self.label, p),
            },
            None => self.label.clone(),
        };
        format!("\n\n--- Context: {header} ---\n```\n{}\n```\n", self.content)
    }
}
