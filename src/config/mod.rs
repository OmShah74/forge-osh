pub mod keyring;
pub mod models;

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Name used for directories and display
pub const APP_NAME: &str = "forge-osh";

/// Get the config directory: ~/.forge-osh/
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FORGE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!(".{APP_NAME}"))
}

/// Get the data directory: ~/.local/share/forge-osh/
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FORGE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local")
                .join("share")
        })
        .join(APP_NAME)
}

/// Get the log directory
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Get sessions directory
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

// ---------------------------------------------------------------------------
// Config structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub features: FeaturesConfig,
}

/// Experimental / opt-in feature flags. Mirrors Codex's `[features]` table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeaturesConfig {
    /// Enable the /goal primitive (autonomous, durable, verifiable goals).
    #[serde(default)]
    pub goals: bool,
}

/// Configuration for MCP (Model Context Protocol) servers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

/// Per-server MCP configuration.
///
/// For built-in catalog entries, only `id` + `enabled` are typically set —
/// the command/args/secret_specs come from the static catalog. For custom
/// servers (id not in the catalog), all fields must be filled in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub secret_specs: Vec<crate::mcp::catalog::SecretSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_provider")]
    pub default_provider: String,
    #[serde(default = "default_true")]
    pub auto_save_sessions: bool,
    #[serde(default = "default_max_session_history")]
    pub max_session_history: usize,
    #[serde(default)]
    pub trust_mode: bool,
    #[serde(default)]
    pub verbose: bool,
    #[serde(default)]
    pub system_prompt_extra: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            default_provider: default_provider(),
            auto_save_sessions: true,
            max_session_history: 100,
            trust_mode: false,
            verbose: false,
            system_prompt_extra: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub anthropic: ProviderConfig,
    #[serde(default)]
    pub openai: ProviderConfig,
    #[serde(default)]
    pub gemini: ProviderConfig,
    #[serde(default)]
    pub groq: ProviderConfig,
    #[serde(default)]
    pub grok: ProviderConfig,
    #[serde(default)]
    pub openrouter: ProviderConfig,
    #[serde(default)]
    pub mistral: ProviderConfig,
    #[serde(default)]
    pub deepseek: ProviderConfig,
    #[serde(default)]
    pub together: ProviderConfig,
    #[serde(default)]
    pub fireworks: ProviderConfig,
    #[serde(default)]
    pub perplexity: ProviderConfig,
    #[serde(default)]
    pub cohere: ProviderConfig,
    #[serde(default)]
    pub ollama: LocalProviderConfig,
    #[serde(default)]
    pub llamacpp: LocalProviderConfig,
    #[serde(default)]
    pub lmstudio: LocalProviderConfig,
    #[serde(default)]
    pub vllm: LocalProviderConfig,
    #[serde(default)]
    pub jan: LocalProviderConfig,
    #[serde(default)]
    pub localai: LocalProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            default_model: String::new(),
            base_url: String::new(),
            timeout_seconds: default_timeout(),
            max_retries: default_retries(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProviderConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub default_model: String,
    #[serde(default = "default_true")]
    pub auto_detect: bool,
}

impl Default for LocalProviderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            default_model: String::new(),
            auto_detect: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: u32,
    #[serde(default = "default_true")]
    pub planning_mode: bool,
    #[serde(default = "default_summarize_at")]
    pub auto_summarize_at: f32,
    #[serde(default = "default_max_output")]
    pub max_output_per_tool: usize,
    #[serde(default = "default_true")]
    pub skills_enabled: bool,
    #[serde(default = "default_true")]
    pub include_skills_in_system_prompt: bool,
    #[serde(default = "default_max_skill_listed_in_prompt")]
    pub max_skill_listed_in_prompt: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            max_tool_iterations: default_max_tool_iterations(),
            planning_mode: true,
            auto_summarize_at: default_summarize_at(),
            max_output_per_tool: default_max_output(),
            skills_enabled: true,
            include_skills_in_system_prompt: true,
            max_skill_listed_in_prompt: default_max_skill_listed_in_prompt(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_tools")]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default)]
    pub bash: BashToolConfig,
    #[serde(default)]
    pub web: WebToolConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: default_tools(),
            disabled: Vec::new(),
            bash: BashToolConfig::default(),
            web: WebToolConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashToolConfig {
    #[serde(default = "default_bash_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_bash_max_timeout")]
    pub max_timeout_seconds: u64,
    #[serde(default)]
    pub allowed_commands: Vec<String>,
    #[serde(default = "default_blocked_commands")]
    pub blocked_commands: Vec<String>,
}

impl Default for BashToolConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: default_bash_timeout(),
            max_timeout_seconds: default_bash_max_timeout(),
            allowed_commands: Vec::new(),
            blocked_commands: default_blocked_commands(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebToolConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_web_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_web_max_content")]
    pub max_content_length: usize,
}

impl Default for WebToolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_seconds: default_web_timeout(),
            max_content_length: default_web_max_content(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub show_token_count: bool,
    #[serde(default = "default_true")]
    pub show_cost: bool,
    #[serde(default = "default_true")]
    pub show_spinner: bool,
    #[serde(default = "default_true")]
    pub syntax_highlight: bool,
    #[serde(default = "default_true")]
    pub diff_before_apply: bool,
    #[serde(default)]
    pub timestamp_messages: bool,
    #[serde(default = "default_true")]
    pub compact_tool_output: bool,
    #[serde(default = "default_max_conversation_lines")]
    pub max_conversation_lines: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_token_count: true,
            show_cost: true,
            show_spinner: true,
            syntax_highlight: true,
            diff_before_apply: true,
            timestamp_messages: false,
            compact_tool_output: true,
            max_conversation_lines: default_max_conversation_lines(),
        }
    }
}

// ---------------------------------------------------------------------------
// Default value functions
// ---------------------------------------------------------------------------

fn default_theme() -> String {
    "dark".to_string()
}
fn default_provider() -> String {
    "openai".to_string()
}
fn default_true() -> bool {
    true
}
fn default_max_session_history() -> usize {
    100
}
fn default_timeout() -> u64 {
    120
}
fn default_retries() -> u32 {
    3
}
fn default_max_tokens() -> u32 {
    8192
}
fn default_temperature() -> f32 {
    0.7
}
fn default_max_tool_iterations() -> u32 {
    50
}
fn default_summarize_at() -> f32 {
    0.8
}
fn default_max_output() -> usize {
    50000
}
fn default_max_skill_listed_in_prompt() -> usize {
    12
}
fn default_bash_timeout() -> u64 {
    30
}
fn default_bash_max_timeout() -> u64 {
    300
}
fn default_web_timeout() -> u64 {
    15
}
fn default_web_max_content() -> usize {
    50000
}
fn default_max_conversation_lines() -> usize {
    1000
}

fn default_blocked_commands() -> Vec<String> {
    vec![
        "rm -rf /".to_string(),
        "sudo rm -rf /".to_string(),
        "mkfs".to_string(),
        ":(){:|:&};:".to_string(),
    ]
}

fn default_tools() -> Vec<String> {
    vec![
        "read_file",
        "write_file",
        "edit_file",
        "create_file",
        "delete_file",
        "list_directory",
        "move_file",
        "copy_file",
        "bash",
        "search_files",
        "find_files",
        "git_status",
        "git_diff",
        "git_log",
        "git_add",
        "git_commit",
        "git_branch",
        "git_checkout",
        "web_fetch",
        "web_search",
        "run_linter",
        "run_tests",
        "run_formatter",
        "todo_write",
        "task_create",
        "task_update",
        "task_get",
        "task_list",
        "ask_user",
        "enter_plan_mode",
        "exit_plan_mode",
        "invoke_skill",
        "notebook_read",
        "enter_worktree",
        "exit_worktree",
        "list_worktrees",
        "git_stash",
        "git_blame",
        "git_show",
        "git_reset",
        "git_fetch",
        "git_push",
        "git_pull",
        "powershell",
        "graph_query",
        "lsp_diagnostics",
        "lsp_definition",
        "lsp_references",
        "lsp_hover",
        "lsp_document_symbols",
        "lsp_workspace_symbols",
        "lsp_rename",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn legacy_default_tools() -> Vec<String> {
    vec![
        "read_file",
        "write_file",
        "edit_file",
        "create_file",
        "delete_file",
        "list_directory",
        "move_file",
        "copy_file",
        "bash",
        "search_files",
        "find_files",
        "git_status",
        "git_diff",
        "git_log",
        "git_add",
        "git_commit",
        "git_branch",
        "git_checkout",
        "web_fetch",
        "web_search",
        "run_linter",
        "run_tests",
        "run_formatter",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

// ---------------------------------------------------------------------------
// Config loading / saving
// ---------------------------------------------------------------------------

impl Config {
    /// Load config from the default path, creating defaults if missing
    pub fn load() -> Self {
        let path = config_dir().join("config.toml");
        Self::load_from(&path).unwrap_or_default()
    }

    /// Load without auto-creating; used by callers (e.g. /mcp persistence)
    /// that want to overwrite a single section while preserving everything else.
    pub fn load_raw() -> Result<Self> {
        let path = config_dir().join("config.toml");
        if !path.exists() {
            return Ok(Config::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let cfg: Config = toml::from_str(&content)?;
        Ok(cfg)
    }

    /// Load from a specific path
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Config::default();
            config.save_to(path)?;
            return Ok(config);
        }
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        if config.tools.enabled == legacy_default_tools() {
            config.tools.enabled = default_tools();
        }
        Ok(config)
    }

    /// Save config to the default path
    pub fn save(&self) -> Result<()> {
        let path = config_dir().join("config.toml");
        self.save_to(&path)
    }

    /// Save to a specific path
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Overlay environment variables on top of loaded config
    pub fn merge_env(&mut self) {
        if let Ok(provider) = std::env::var("FORGE_PROVIDER") {
            self.general.default_provider = provider;
        }
        if let Ok(theme) = std::env::var("FORGE_THEME") {
            self.general.theme = theme;
        }
        if std::env::var("FORGE_TRUST")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            self.general.trust_mode = true;
        }
        if std::env::var("FORGE_NO_COLOR")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            self.general.theme = "plain".to_string();
        }
    }

    /// Get the provider config for a given provider id
    pub fn provider_config(&self, provider_id: &str) -> Option<&ProviderConfig> {
        match provider_id {
            "anthropic" => Some(&self.providers.anthropic),
            "openai" => Some(&self.providers.openai),
            "gemini" => Some(&self.providers.gemini),
            "groq" => Some(&self.providers.groq),
            "grok" => Some(&self.providers.grok),
            "openrouter" => Some(&self.providers.openrouter),
            "mistral" => Some(&self.providers.mistral),
            "deepseek" => Some(&self.providers.deepseek),
            "together" => Some(&self.providers.together),
            "fireworks" => Some(&self.providers.fireworks),
            "perplexity" => Some(&self.providers.perplexity),
            "cohere" => Some(&self.providers.cohere),
            _ => None,
        }
    }

    /// Check if a tool is enabled in config
    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        if self.tools.disabled.contains(&tool_name.to_string()) {
            return false;
        }
        self.tools.enabled.is_empty() || self.tools.enabled.contains(&tool_name.to_string())
    }

    /// Initialize with sane defaults for each provider's base URL and default model
    pub fn with_provider_defaults(mut self) -> Self {
        self.providers.anthropic.base_url = non_empty_or(
            &self.providers.anthropic.base_url,
            "https://api.anthropic.com/v1",
        );
        self.providers.anthropic.default_model = non_empty_or(
            &self.providers.anthropic.default_model,
            "claude-sonnet-4-20250514",
        );

        self.providers.openai.base_url =
            non_empty_or(&self.providers.openai.base_url, "https://api.openai.com/v1");
        self.providers.openai.default_model =
            non_empty_or(&self.providers.openai.default_model, "gpt-4o");

        self.providers.gemini.base_url = non_empty_or(
            &self.providers.gemini.base_url,
            "https://generativelanguage.googleapis.com/v1beta",
        );
        self.providers.gemini.default_model =
            non_empty_or(&self.providers.gemini.default_model, "gemini-2.0-flash");

        self.providers.groq.base_url = non_empty_or(
            &self.providers.groq.base_url,
            "https://api.groq.com/openai/v1",
        );
        self.providers.groq.default_model = non_empty_or(
            &self.providers.groq.default_model,
            "llama-3.3-70b-versatile",
        );

        self.providers.grok.base_url =
            non_empty_or(&self.providers.grok.base_url, "https://api.x.ai/v1");
        self.providers.grok.default_model =
            non_empty_or(&self.providers.grok.default_model, "grok-3");

        self.providers.openrouter.base_url = non_empty_or(
            &self.providers.openrouter.base_url,
            "https://openrouter.ai/api/v1",
        );
        self.providers.openrouter.default_model = non_empty_or(
            &self.providers.openrouter.default_model,
            "anthropic/claude-sonnet-4-20250514",
        );

        self.providers.mistral.base_url = non_empty_or(
            &self.providers.mistral.base_url,
            "https://api.mistral.ai/v1",
        );
        self.providers.mistral.default_model = non_empty_or(
            &self.providers.mistral.default_model,
            "mistral-large-latest",
        );

        self.providers.deepseek.base_url = non_empty_or(
            &self.providers.deepseek.base_url,
            "https://api.deepseek.com/v1",
        );
        self.providers.deepseek.default_model =
            non_empty_or(&self.providers.deepseek.default_model, "deepseek-chat");

        self.providers.together.base_url = non_empty_or(
            &self.providers.together.base_url,
            "https://api.together.xyz/v1",
        );
        self.providers.together.default_model = non_empty_or(
            &self.providers.together.default_model,
            "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        );

        self.providers.fireworks.base_url = non_empty_or(
            &self.providers.fireworks.base_url,
            "https://api.fireworks.ai/inference/v1",
        );
        self.providers.fireworks.default_model = non_empty_or(
            &self.providers.fireworks.default_model,
            "accounts/fireworks/models/llama-v3p3-70b-instruct",
        );

        self.providers.perplexity.base_url = non_empty_or(
            &self.providers.perplexity.base_url,
            "https://api.perplexity.ai",
        );
        self.providers.perplexity.default_model =
            non_empty_or(&self.providers.perplexity.default_model, "sonar-pro");

        self.providers.cohere.base_url =
            non_empty_or(&self.providers.cohere.base_url, "https://api.cohere.ai/v2");
        self.providers.cohere.default_model =
            non_empty_or(&self.providers.cohere.default_model, "command-r-plus");

        // Local providers
        self.providers.ollama.base_url =
            non_empty_or(&self.providers.ollama.base_url, "http://localhost:11434");
        self.providers.llamacpp.base_url =
            non_empty_or(&self.providers.llamacpp.base_url, "http://localhost:8080");
        self.providers.lmstudio.base_url =
            non_empty_or(&self.providers.lmstudio.base_url, "http://localhost:1234");
        self.providers.vllm.base_url =
            non_empty_or(&self.providers.vllm.base_url, "http://localhost:8000");
        self.providers.jan.base_url =
            non_empty_or(&self.providers.jan.base_url, "http://localhost:1337");
        self.providers.localai.base_url =
            non_empty_or(&self.providers.localai.base_url, "http://localhost:8080");

        self
    }
}

fn non_empty_or(val: &str, default: &str) -> String {
    if val.is_empty() {
        default.to_string()
    } else {
        val.to_string()
    }
}

/// All known cloud provider IDs
pub fn cloud_provider_ids() -> Vec<&'static str> {
    vec![
        "anthropic",
        "openai",
        "gemini",
        "groq",
        "grok",
        "openrouter",
        "mistral",
        "deepseek",
        "together",
        "fireworks",
        "perplexity",
        "cohere",
    ]
}

/// All known local provider IDs
pub fn local_provider_ids() -> Vec<&'static str> {
    vec!["ollama", "llamacpp", "lmstudio", "vllm", "jan", "localai"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.general.theme, "dark");
        assert_eq!(config.general.default_provider, "openai");
        assert!(config.general.auto_save_sessions);
        assert!(!config.general.trust_mode);
    }

    #[test]
    fn test_config_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let config = Config::default();
        config.save_to(&path).unwrap();

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.general.theme, config.general.theme);
        assert_eq!(
            loaded.general.default_provider,
            config.general.default_provider
        );
    }

    #[test]
    fn test_with_provider_defaults() {
        let config = Config::default().with_provider_defaults();
        assert_eq!(
            config.providers.anthropic.base_url,
            "https://api.anthropic.com/v1"
        );
        assert_eq!(
            config.providers.groq.base_url,
            "https://api.groq.com/openai/v1"
        );
        assert_eq!(config.providers.ollama.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_is_tool_enabled() {
        let config = Config::default();
        assert!(config.is_tool_enabled("read_file"));
        assert!(config.is_tool_enabled("bash"));
    }
}
