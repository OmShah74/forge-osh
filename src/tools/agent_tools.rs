/// Agent orchestration tools:
/// - AskUserQuestionTool — pause and ask user a clarifying question
/// - EnterPlanModeTool — switch to plan mode (LLM proposes plan, user approves)
/// - ExitPlanModeTool — exit plan mode

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::*;
use super::Tool;

// ---------------------------------------------------------------------------
// Global plan-mode flag (visible to the agent loop)
// ---------------------------------------------------------------------------

static PLAN_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn is_plan_mode_active() -> bool {
    PLAN_MODE_ACTIVE.load(Ordering::SeqCst)
}

pub fn set_plan_mode(active: bool) {
    PLAN_MODE_ACTIVE.store(active, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// AskUserQuestionTool
// ---------------------------------------------------------------------------

pub struct AskUserQuestionTool;

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str { "ask_user" }

    fn description(&self) -> &str {
        "Pause execution and ask the user a clarifying question that requires their input before \
        continuing. Use this when you need specific information from the user that you cannot \
        determine from context. Provide a clear, concise question. The user's answer will be \
        returned as the tool output. Use sparingly — prefer acting with reasonable assumptions \
        over asking too many questions."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of suggested answers (user may still type freely)"
                }
            },
            "required": ["question"]
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let question = match input["question"].as_str() {
            Some(q) => q,
            None => return ToolOutput::error("Missing 'question' parameter"),
        };
        let options = input["options"].as_array();

        // Format the question prompt for display in the TUI
        let mut prompt = format!("**Question from agent:**\n\n{}", question);

        if let Some(opts) = options {
            if !opts.is_empty() {
                prompt.push_str("\n\n**Suggested options:**");
                for (i, opt) in opts.iter().enumerate() {
                    if let Some(s) = opt.as_str() {
                        prompt.push_str(&format!("\n  {}. {}", i + 1, s));
                    }
                }
            }
        }

        prompt.push_str("\n\n*Please type your answer and press Enter.*");

        // The actual blocking wait for user input is handled by the permission system.
        // We return a special marker that the TUI intercepts to show the question
        // and collect a response. The response flows back as a PermissionResponse::Custom.
        ToolOutput::success(format!("AWAITING_USER_INPUT:{}", prompt))
    }
}

// ---------------------------------------------------------------------------
// EnterPlanModeTool
// ---------------------------------------------------------------------------

pub struct EnterPlanModeTool;

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str { "enter_plan_mode" }

    fn description(&self) -> &str {
        "Switch to plan mode. In plan mode, propose your complete plan for accomplishing the task \
        before executing any actions. Describe each step you will take. After entering plan mode, \
        present your plan and then call exit_plan_mode once the user has reviewed it. \
        This is appropriate for complex, multi-step, or potentially destructive tasks where \
        the user should review the approach before any changes are made."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Why plan mode is being entered (brief description of task complexity)"
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        set_plan_mode(true);
        let reason = input["reason"].as_str().unwrap_or("complex task");
        ToolOutput::success(format!(
            "**Plan Mode Activated** — {reason}\n\n\
            I will now present my plan before taking any actions. \
            Review the plan below and confirm before I proceed with execution.\n\n\
            *(Call exit_plan_mode after presenting the plan to allow execution.)*"
        ))
    }
}

// ---------------------------------------------------------------------------
// ExitPlanModeTool
// ---------------------------------------------------------------------------

pub struct ExitPlanModeTool;

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str { "exit_plan_mode" }

    fn description(&self) -> &str {
        "Exit plan mode and proceed with execution of the proposed plan. \
        Call this after presenting your plan and receiving user approval."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "confirmed": {
                    "type": "boolean",
                    "description": "Set to true when the user has approved the plan"
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let confirmed = input["confirmed"].as_bool().unwrap_or(true);
        set_plan_mode(false);

        if confirmed {
            ToolOutput::success(
                "Plan mode exited. Proceeding with execution.".to_string()
            )
        } else {
            ToolOutput::success(
                "Plan mode exited. Plan was not confirmed — waiting for further instructions.".to_string()
            )
        }
    }
}
