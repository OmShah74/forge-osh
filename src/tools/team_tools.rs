//! Live team coordination tools: `team_post` / `team_read`.
//!
//! These let agents on the same team exchange findings through the shared
//! [`crate::agent::team_bus::TeamBlackboard`] during execution — peer-to-peer,
//! without routing through the orchestrator. They are no-ops (with a clear
//! message) when the current `ToolContext` has no blackboard, i.e. in the
//! ordinary single-agent loop.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::Tool;
use crate::agent::team_spawner::{SpawnStrategy, TeamSpawner};
use crate::types::*;

// ---------------------------------------------------------------------------
// team_post
// ---------------------------------------------------------------------------

pub struct TeamPostTool;

#[async_trait]
impl Tool for TeamPostTool {
    fn name(&self) -> &str {
        "team_post"
    }

    fn description(&self) -> &str {
        "Post a short finding or coordination note to the shared team blackboard so peer agents \
         can read it LIVE, without waiting for the orchestrator. Use this in team/swarm work to \
         announce a discovery, claim ownership of a file before editing it, flag a conflict, or \
         hand off context. Params: `message` (required), `topic` (optional category to group \
         messages, e.g. 'files', 'api', 'blocker')."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "The note to share with teammates." },
                "topic": { "type": "string", "description": "Optional category for filtering (e.g. 'files')." }
            },
            "required": ["message"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Posts session-local coordination state only; never touches the user's
        // workspace, so it must never trigger a permission prompt.
        PermissionLevel::ReadOnly
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let message = match input["message"].as_str() {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => return ToolOutput::error("team_post needs a non-empty 'message'."),
        };
        let topic = input["topic"].as_str().unwrap_or("").trim().to_string();

        match &ctx.team_blackboard {
            Some(bb) => {
                let count = {
                    let mut guard = bb.lock();
                    guard.post(ctx.session_id.clone(), topic.clone(), message);
                    guard.len()
                };
                let topic_note = if topic.is_empty() {
                    String::new()
                } else {
                    format!(" [topic: {topic}]")
                };
                ToolOutput::success(format!(
                    "Posted to the team blackboard{topic_note} ({count} message(s) total). \
                     Teammates can see it with team_read."
                ))
            }
            None => ToolOutput::success(
                "No shared team blackboard in this context — team_post only has an effect inside \
                 a /team or spawn_team worker. (Nothing was shared.)",
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// team_read
// ---------------------------------------------------------------------------

pub struct TeamReadTool;

#[async_trait]
impl Tool for TeamReadTool {
    fn name(&self) -> &str {
        "team_read"
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "Read the latest messages teammates posted to the shared team blackboard (live peer \
         coordination). Call this before starting work and periodically while working to build on \
         peers' findings and avoid duplicating or clobbering their changes. Params: `topic` \
         (optional filter), `limit` (max messages, default 20)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string", "description": "Only show messages with this topic." },
                "limit": { "type": "integer", "description": "Max messages to return (default 20)." }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let topic = input["topic"].as_str().map(|s| s.trim()).filter(|s| !s.is_empty());
        let limit = input["limit"].as_u64().unwrap_or(20).clamp(1, 200) as usize;

        match &ctx.team_blackboard {
            Some(bb) => {
                // Exclude the caller's own posts so they only see teammates.
                let entries = bb
                    .lock()
                    .read(topic, limit, Some(ctx.session_id.as_str()));
                if entries.is_empty() {
                    ToolOutput::success(
                        "No teammate messages on the blackboard yet. Post your own findings with \
                         team_post so peers can build on them.",
                    )
                } else {
                    let body: Vec<String> = entries.iter().map(|e| format!("- {}", e.render())).collect();
                    ToolOutput::success(format!(
                        "Team blackboard ({} message(s)):\n{}",
                        entries.len(),
                        body.join("\n")
                    ))
                }
            }
            None => ToolOutput::success(
                "No shared team blackboard in this context (not running inside a team).",
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// spawn_team — model-callable dynamic orchestration
// ---------------------------------------------------------------------------

/// Lets an agent (notably a `/goal` worker) fan a task out across sub-agents
/// and choose the execution architecture. Holds the runtime deps via an
/// `Arc<TeamSpawner>`, injected post-boot when the provider router is ready.
pub struct SpawnTeamTool {
    spawner: Arc<TeamSpawner>,
}

impl SpawnTeamTool {
    pub fn new(spawner: Arc<TeamSpawner>) -> Self {
        Self { spawner }
    }
}

#[async_trait]
impl Tool for SpawnTeamTool {
    fn name(&self) -> &str {
        "spawn_team"
    }

    fn description(&self) -> &str {
        "Decompose the current objective and run it across autonomous sub-agents, then get their \
         merged results back. YOU choose the architecture via `strategy`:\n\
         - `swarm`: independent, parallelizable subtasks (e.g. investigate N modules, edit N \
           disjoint files). Sub-agents run at once and coordinate peer-to-peer via the team \
           blackboard. No central review.\n\
         - `orchestrator`: parallel subtasks that must be integrated coherently (e.g. a feature \
           touching shared code). Sub-agents run in parallel, then a review agent reconciles them.\n\
         - `sequential`: tightly-ordered steps where each depends on the previous.\n\
         Use this ONLY for genuinely separable work; for small or tightly-coupled tasks just do \
         them yourself in this loop. Provide `subtasks` as a list of self-contained instructions \
         (preferred), or a single `goal` string to auto-split. Each sub-agent has its own context \
         window and runs in the same working directory; this call blocks until they finish."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "strategy": {
                    "type": "string",
                    "enum": ["swarm", "orchestrator", "sequential"],
                    "description": "Execution architecture (default: orchestrator)."
                },
                "subtasks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Self-contained subtask instructions, one per sub-agent (2-8)."
                },
                "goal": {
                    "type": "string",
                    "description": "Overall goal; used as the team goal and to auto-split if `subtasks` is omitted."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Spawns coordinator-authorized sub-agents (same trust model as
        // /team and @worker). ReadOnly so /goal can orchestrate autonomously
        // without a permission prompt; the sub-agents enforce their own gates.
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let strategy = input["strategy"]
            .as_str()
            .and_then(SpawnStrategy::parse)
            .unwrap_or(SpawnStrategy::Orchestrator);

        // Build the team goal: explicit subtasks (joined with ';' so the board
        // planner makes one worker per subtask) or a single goal string.
        let subtasks: Vec<String> = input["subtasks"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let goal = if subtasks.len() >= 2 {
            subtasks.join("; ")
        } else if let Some(g) = input["goal"].as_str().filter(|s| !s.trim().is_empty()) {
            g.trim().to_string()
        } else if let Some(single) = subtasks.first() {
            single.clone()
        } else {
            return ToolOutput::error(
                "spawn_team needs either `subtasks` (a list of instructions) or a `goal` string.",
            );
        };

        let workdir = ctx.working_dir.to_string_lossy().to_string();
        let report = self.spawner.run_team(goal, strategy, workdir).await;
        ToolOutput::success(report)
    }
}
