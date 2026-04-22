use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "forge-osh",
    version,
    about = "A universal, provider-agnostic coding agent for the terminal",
    long_about = "forge-osh is a powerful agentic coding assistant that works with any LLM provider.\nIt reads and writes files, executes commands, searches code, and manages git."
)]
pub struct Cli {
    /// Initial prompt (non-interactive mode)
    #[arg(trailing_var_arg = true)]
    pub prompt: Vec<String>,

    /// Provider to use
    #[arg(short, long, env = "FORGE_PROVIDER")]
    pub provider: Option<String>,

    /// Model to use
    #[arg(short, long, env = "FORGE_MODEL")]
    pub model: Option<String>,

    /// Session name to create or resume
    #[arg(short, long)]
    pub session: Option<String>,

    /// Resume a session by ID. Omit the value (`--resume`) to resume the most
    /// recently saved session. Pass an explicit id to resume that session:
    /// `--resume e1fa0…`.
    #[arg(short, long, num_args = 0..=1, default_missing_value = "__latest__")]
    pub resume: Option<String>,

    /// Working directory
    #[arg(short, long)]
    pub dir: Option<String>,

    /// Disable all tools (chat only mode)
    #[arg(long)]
    pub no_tools: bool,

    /// Enable trust mode (skip confirmations)
    #[arg(long, env = "FORGE_TRUST")]
    pub trust: bool,

    /// Disable colors
    #[arg(long, env = "FORGE_NO_COLOR")]
    pub no_color: bool,

    /// Color theme
    #[arg(long, default_value = "dark")]
    pub theme: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Manage sessions
    Sessions {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Manage providers
    Providers {
        #[command(subcommand)]
        action: ProviderAction,
    },
    /// Manage models
    Models {
        #[command(subcommand)]
        action: ModelAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigAction {
    /// Set a config value
    Set { key: String, value: String },
    /// Get a config value
    Get { key: String },
    /// Manage API keys
    Keys {
        #[command(subcommand)]
        action: KeyAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum KeyAction {
    /// Set an API key
    Set { provider: String, key: String },
    /// List configured keys
    List,
    /// Remove an API key
    Remove { provider: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum SessionAction {
    /// List all sessions
    List,
    /// Delete a session
    Delete { name: String },
    /// Export a session to Markdown
    Export { name: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProviderAction {
    /// List all configured providers
    List,
    /// Test a provider connection
    Test { provider: String },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ModelAction {
    /// List available models
    List {
        /// Filter by provider
        provider: Option<String>,
    },
    /// Set default model for a provider
    Set { provider: String, model: String },
}
