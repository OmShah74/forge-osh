pub mod agent_tools;
pub mod code;
pub mod executor;
pub mod fs;
pub mod git;
pub mod search;
pub mod shell;
pub mod tasks;
pub mod web;
pub mod worktree;

use async_trait::async_trait;
use std::collections::HashMap;

use crate::types::{PermissionLevel, ToolContext, ToolDefinition, ToolOutput};

/// Trait every tool must implement
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn permission_level(&self) -> PermissionLevel;
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolOutput;
}

/// Registry of all available tools
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register all built-in tools
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // ── File system tools ──────────────────────────────────────────────
        registry.register(Box::new(fs::ReadFileTool));
        registry.register(Box::new(fs::WriteFileTool));
        registry.register(Box::new(fs::EditFileTool));
        registry.register(Box::new(fs::CreateFileTool));
        registry.register(Box::new(fs::DeleteFileTool));
        registry.register(Box::new(fs::ListDirectoryTool));
        registry.register(Box::new(fs::MoveFileTool));
        registry.register(Box::new(fs::CopyFileTool));

        // ── Shell ──────────────────────────────────────────────────────────
        registry.register(Box::new(shell::BashTool::default()));

        // ── Git ────────────────────────────────────────────────────────────
        registry.register(Box::new(git::GitStatusTool));
        registry.register(Box::new(git::GitDiffTool));
        registry.register(Box::new(git::GitLogTool));
        registry.register(Box::new(git::GitAddTool));
        registry.register(Box::new(git::GitCommitTool));
        registry.register(Box::new(git::GitBranchTool));
        registry.register(Box::new(git::GitCheckoutTool));
        registry.register(Box::new(git::GitStashTool));
        registry.register(Box::new(git::GitBlameTool));
        registry.register(Box::new(git::GitShowTool));
        registry.register(Box::new(git::GitResetTool));
        registry.register(Box::new(git::GitFetchTool));
        registry.register(Box::new(git::GitPushTool));
        registry.register(Box::new(git::GitPullTool));

        // ── Search ─────────────────────────────────────────────────────────
        registry.register(Box::new(search::SearchFilesTool));
        registry.register(Box::new(search::FindFilesTool));

        // ── Web ────────────────────────────────────────────────────────────
        registry.register(Box::new(web::WebFetchTool));
        registry.register(Box::new(web::WebSearchTool));

        // ── Code quality ───────────────────────────────────────────────────
        registry.register(Box::new(code::RunLinterTool));
        registry.register(Box::new(code::RunTestsTool));
        registry.register(Box::new(code::RunFormatterTool));

        // ── Task management ────────────────────────────────────────────────
        registry.register(Box::new(tasks::TodoWriteTool));
        registry.register(Box::new(tasks::TaskCreateTool));
        registry.register(Box::new(tasks::TaskUpdateTool));
        registry.register(Box::new(tasks::TaskGetTool));
        registry.register(Box::new(tasks::TaskListTool));

        // ── Agent orchestration ────────────────────────────────────────────
        registry.register(Box::new(agent_tools::AskUserQuestionTool));
        registry.register(Box::new(agent_tools::EnterPlanModeTool));
        registry.register(Box::new(agent_tools::ExitPlanModeTool));

        // ── Git worktrees ──────────────────────────────────────────────────
        registry.register(Box::new(worktree::EnterWorktreeTool));
        registry.register(Box::new(worktree::ExitWorktreeTool));
        registry.register(Box::new(worktree::ListWorktreesTool));

        registry
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn all_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self.tools
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect();
        // Sort for deterministic ordering
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }
}
