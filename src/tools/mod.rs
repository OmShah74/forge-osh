pub mod agent_tools;
pub mod code;
pub mod executor;
pub mod fs;
pub mod git;
pub mod notebook;
pub mod powershell;
pub mod search;
pub mod shell;
pub mod skills;
pub mod tasks;
pub mod team_tools;
pub mod validate;
pub mod web;
pub mod worktree;

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::Config;
use crate::types::{PermissionLevel, ToolContext, ToolDefinition, ToolOutput};

/// Trait every tool must implement
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn permission_level(&self) -> PermissionLevel;

    /// Permission level considering the specific input (e.g. bash can be ReadOnly for `ls`).
    /// Default delegates to `permission_level()`. Override to enable input-aware classification.
    fn effective_permission_level(&self, _input: &serde_json::Value) -> PermissionLevel {
        self.permission_level()
    }

    /// Whether this tool is safe to run concurrently with other
    /// concurrency-safe tools in the same turn. ReadOnly tools that do no
    /// shared-filesystem mutation (`read_file`, `search_files`, `find_files`,
    /// `list_directory`, `git_status`, `git_diff`, `web_fetch`, etc.) should
    /// override this to return `true`. Default: `false` (run serially).
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolOutput;
}

/// Registry of all available tools.
///
/// Internally backed by a `parking_lot::RwLock` so tools can be added/removed
/// dynamically (used by the MCP manager when an MCP server connects/disconnects)
/// without rebuilding the whole agent. Each entry is an `Arc<dyn Tool>` so
/// callers receive clones, never borrowed references — which keeps the
/// existing `Arc<ToolRegistry>` shared-ownership model intact.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// Register all built-in tools
    pub fn with_builtins() -> Self {
        Self::with_config(&Config::default())
    }

    /// Register all built-in tools after applying config enable/disable lists
    /// and tool-specific settings.
    pub fn with_config(config: &Config) -> Self {
        let registry = Self::new();

        // ── File system tools ──────────────────────────────────────────────
        registry.register_enabled(config, Box::new(fs::ReadFileTool));
        registry.register_enabled(config, Box::new(fs::WriteFileTool));
        registry.register_enabled(config, Box::new(fs::EditFileTool));
        registry.register_enabled(config, Box::new(fs::CreateFileTool));
        registry.register_enabled(config, Box::new(fs::DeleteFileTool));
        registry.register_enabled(config, Box::new(fs::ListDirectoryTool));
        registry.register_enabled(config, Box::new(fs::MoveFileTool));
        registry.register_enabled(config, Box::new(fs::CopyFileTool));

        // ── Shell ──────────────────────────────────────────────────────────
        registry.register_enabled(
            config,
            Box::new(shell::BashTool::from_config(&config.tools.bash)),
        );
        registry.register_enabled(config, Box::new(powershell::PowerShellTool::default()));

        // ── Git ────────────────────────────────────────────────────────────
        registry.register_enabled(config, Box::new(git::GitStatusTool));
        registry.register_enabled(config, Box::new(git::GitDiffTool));
        registry.register_enabled(config, Box::new(git::GitLogTool));
        registry.register_enabled(config, Box::new(git::GitAddTool));
        registry.register_enabled(config, Box::new(git::GitCommitTool));
        registry.register_enabled(config, Box::new(git::GitBranchTool));
        registry.register_enabled(config, Box::new(git::GitCheckoutTool));
        registry.register_enabled(config, Box::new(git::GitStashTool));
        registry.register_enabled(config, Box::new(git::GitBlameTool));
        registry.register_enabled(config, Box::new(git::GitShowTool));
        registry.register_enabled(config, Box::new(git::GitResetTool));
        registry.register_enabled(config, Box::new(git::GitFetchTool));
        registry.register_enabled(config, Box::new(git::GitPushTool));
        registry.register_enabled(config, Box::new(git::GitPullTool));

        // ── Search ─────────────────────────────────────────────────────────
        registry.register_enabled(config, Box::new(search::SearchFilesTool));
        registry.register_enabled(config, Box::new(search::FindFilesTool));

        // ── Web ────────────────────────────────────────────────────────────
        if config.tools.web.enabled {
            registry.register_enabled(
                config,
                Box::new(web::WebFetchTool::from_config(&config.tools.web)),
            );
            registry.register_enabled(
                config,
                Box::new(web::WebSearchTool::from_config(&config.tools.web)),
            );
        }

        // ── Code quality ───────────────────────────────────────────────────
        registry.register_enabled(config, Box::new(code::RunLinterTool));
        registry.register_enabled(config, Box::new(code::RunTestsTool));
        registry.register_enabled(config, Box::new(code::RunFormatterTool));

        // ── Task management ────────────────────────────────────────────────
        registry.register_enabled(config, Box::new(tasks::UpdatePlanTool));
        registry.register_enabled(config, Box::new(tasks::TodoWriteTool));
        registry.register_enabled(config, Box::new(tasks::TaskCreateTool));
        registry.register_enabled(config, Box::new(tasks::TaskUpdateTool));
        registry.register_enabled(config, Box::new(tasks::TaskGetTool));
        registry.register_enabled(config, Box::new(tasks::TaskListTool));

        // Live team coordination (shared blackboard).
        registry.register_enabled(config, Box::new(team_tools::TeamPostTool));
        registry.register_enabled(config, Box::new(team_tools::TeamReadTool));

        // ── Agent orchestration ────────────────────────────────────────────
        registry.register_enabled(config, Box::new(agent_tools::AskUserQuestionTool));
        registry.register_enabled(config, Box::new(agent_tools::EnterPlanModeTool));
        registry.register_enabled(config, Box::new(agent_tools::ExitPlanModeTool));
        registry.register_enabled(config, Box::new(skills::InvokeSkillTool));

        // ── Notebooks ──────────────────────────────────────────────────────
        registry.register_enabled(config, Box::new(notebook::NotebookReadTool));

        // ── Git worktrees ──────────────────────────────────────────────────
        registry.register_enabled(config, Box::new(worktree::EnterWorktreeTool));
        registry.register_enabled(config, Box::new(worktree::ExitWorktreeTool));
        registry.register_enabled(config, Box::new(worktree::ListWorktreesTool));

        registry
    }

    pub fn register(&self, tool: Box<dyn Tool>) {
        let arc: Arc<dyn Tool> = Arc::from(tool);
        self.tools.write().insert(arc.name().to_string(), arc);
    }

    pub fn register_enabled(&self, config: &Config, tool: Box<dyn Tool>) {
        if config.is_tool_enabled(tool.name()) {
            self.register(tool);
        }
    }

    /// Remove a tool by name. Used by the MCP manager when a server disconnects.
    pub fn unregister(&self, name: &str) -> bool {
        self.tools.write().remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().get(name).cloned()
    }

    pub fn all_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .read()
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
        let mut names: Vec<String> = self.tools.read().keys().cloned().collect();
        names.sort();
        names
    }
}
