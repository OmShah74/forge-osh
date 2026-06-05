/**
 * Wire protocol mirror of `src/jsonrpc/{outbound,inbound}.rs`.
 *
 * Keep this file in lockstep with the Rust enums. The handshake refuses to
 * attach if the binary's `JSONRPC_VERSION` doesn't match `EXPECTED_VERSION`
 * below, so any breaking schema change MUST bump both.
 */

export const EXPECTED_VERSION = 2;

// ---------------------------------------------------------------------------
// Outbound events (agent → extension)
// ---------------------------------------------------------------------------

export type Usage = {
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
  cost_usd: number;
};

export type PermissionLevelStr =
  | "read_only"
  | "mutating"
  | "destructive"
  | "shell"
  | "network";

export type SystemKind = "info" | "warn" | "error";

export type ForgeEvent =
  | {
      type: "ready";
      jsonrpc_version: number;
      forge_version: string;
      provider: string;
      model: string;
    }
  | { type: "assistant_text_delta"; text: string }
  | { type: "assistant_text_end" }
  | { type: "thinking_start" }
  | { type: "thinking_delta"; text: string }
  | { type: "thinking_end" }
  | {
      type: "tool_call_start";
      id: string;
      name: string;
      input: unknown;
    }
  | {
      type: "tool_call_end";
      id: string;
      output_excerpt: string;
      is_error: boolean;
    }
  | {
      // Live incremental stdout/stderr from a long-running tool
      // (currently bash/powershell). Emitted between tool_call_start and
      // tool_call_end so the webview can tail output as it arrives instead
      // of waiting for the buffered output_excerpt. Added in JSONRPC v2.
      type: "tool_output_delta";
      id: string;
      stream: "stdout" | "stderr";
      text: string;
    }
  | {
      type: "permission_request";
      id: string;
      tool: string;
      summary: string;
      level: PermissionLevelStr;
      input: unknown;
      diff_preview?: string;
    }
  | {
      type: "diff_preview";
      tool_call_id: string;
      path: string;
      unified_diff: string;
    }
  | ({ type: "usage" } & Usage)
  | { type: "compaction"; stage: "start" | "complete" | "failed"; summary?: string }
  | { type: "goal_event"; goal_id: string; payload: unknown }
  | { type: "session_loaded"; id: string; message_count: number }
  | { type: "system_message"; text: string; kind: SystemKind }
  | { type: "done"; reason: string }
  | { type: "error"; message: string };

// ---------------------------------------------------------------------------
// Inbound commands (extension → agent)
// ---------------------------------------------------------------------------

export type ContextBlock = {
  kind: "file" | "selection" | "diagnostic" | "url";
  label: string;
  content: string;
  path?: string;
  range?: [number, number, number, number];
};

export type PermissionResponseStr = "allow" | "deny" | "always_allow" | "trust";

export type GoalAction =
  | "pause"
  | "resume"
  | "clear"
  | "verify_now"
  | "force_complete";

export type SkillAction = "list" | "show" | "reload" | "delete";

export type PermissionRuleAction = "list" | "add_allow" | "add_deny" | "remove";

export type McpAction = "list" | "connect" | "disconnect" | "enable" | "disable";

export type ConfigureKey = "permission_mode" | "thinking" | "effort_level";

export type ForgeCommand =
  | { type: "user_message"; text: string; context_blocks?: ContextBlock[] }
  | { type: "permission_response"; id: string; response: PermissionResponseStr }
  | { type: "cancel" }
  | { type: "compact"; keep_last?: number }
  | { type: "switch_model"; provider: string; model: string }
  | { type: "load_session"; name: string }
  | { type: "new_session"; name?: string }
  | { type: "spawn_goal"; objective: string; spec_path?: string }
  | { type: "goal_control"; goal_id: string; action: GoalAction }
  | { type: "invoke_skill"; name: string; args?: string }
  | { type: "configure"; key: ConfigureKey; value: unknown }
  | { type: "ping" }
  | { type: "undo" }
  | { type: "rename_session"; name: string }
  | { type: "save_session" }
  | { type: "goal_status"; goal_id: string }
  | { type: "skill_command"; action: SkillAction; name?: string }
  | {
      type: "permission_rules";
      action: PermissionRuleAction;
      tool?: string;
      pattern?: string;
      index?: number;
    }
  | { type: "mcp_command"; action: McpAction; server?: string }
  | { type: "build_graph"; rebuild?: boolean }
  | { type: "hooks_reload" };

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Compute a cache-hit percentage for status-bar display. */
export function cacheHitPercent(u: Usage): number {
  const reads = u.cache_read ?? 0;
  const fresh = u.input ?? 0;
  const total = reads + fresh;
  return total > 0 ? Math.round((reads / total) * 100) : 0;
}
