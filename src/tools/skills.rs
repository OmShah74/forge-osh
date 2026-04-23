use async_trait::async_trait;
use serde_json::json;

use crate::skills::{apply_skill, SkillLoader};
use crate::tools::Tool;
use crate::types::{PermissionLevel, ToolContext, ToolOutput};

pub struct InvokeSkillTool;

#[async_trait]
impl Tool for InvokeSkillTool {
    fn name(&self) -> &str {
        "invoke_skill"
    }

    fn description(&self) -> &str {
        "Execute a registered skill by name. Returns the skill's materialized prompt \
         which describes the workflow to follow. When a skill is active, tool usage \
         may be narrowed to the skill's allowlist."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "skill": { "type": "string", "description": "Skill name to invoke." },
                "args": { "type": ["string", "null"], "description": "Optional skill arguments." }
            },
            "required": ["skill"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    // Mutates session.active_skill_scope via loop.rs::apply_special_tool_effects.
    // Must never run concurrently with other tools in the same turn.
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolOutput {
        let Some(skill_name) = input.get("skill").and_then(|v| v.as_str()) else {
            return ToolOutput::error("invoke_skill requires a string 'skill' field");
        };
        let args = input.get("args").and_then(|v| v.as_str());

        let applied = if let Some(shared) = &ctx.skill_registry {
            let guard = shared.read();
            apply_skill(&guard, skill_name, args, &ctx.session_id)
        } else {
            let registry = SkillLoader::load(&ctx.working_dir);
            apply_skill(&registry, skill_name, args, &ctx.session_id)
        };

        let applied = match applied {
            Ok(applied) => applied,
            Err(err) => return ToolOutput::error(format!("Skill invocation failed: {err}")),
        };

        // Return the materialized prompt AS the tool_result content. The LLM
        // will receive it directly and act on it — no separate user turn needs
        // to be injected. `apply_special_tool_effects` in the loop uses the
        // metadata to set up scope/hooks without touching history.
        ToolOutput {
            content: format!(
                "Skill '{}' activated ({} mode).\n\n{}",
                applied.skill_name,
                applied.mode.as_str(),
                applied.materialized_prompt
            ),
            is_error: false,
            metadata: Some(json!({
                "skill_invocation": {
                    "success": true,
                    "mode": applied.mode.as_str(),
                    "skill_name": applied.skill_name,
                    "applied_allowed_tools": if applied.allowed_tools.is_empty() { serde_json::Value::Null } else { json!(applied.allowed_tools) },
                    "model_override": applied.model_override,
                    "materialized_prompt": applied.materialized_prompt,
                    "source": applied.source.label(),
                    "canonical_path": applied.canonical_path.map(|p| p.to_string_lossy().to_string()),
                    "hooks": {
                        "PreToolUse": applied.hooks.pre_tool_use,
                        "PostToolUse": applied.hooks.post_tool_use,
                        "Stop": applied.hooks.stop,
                    }
                }
            })),
        }
    }
}
