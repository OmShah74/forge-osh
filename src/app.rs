use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::cli::*;
use crate::config::{self, keyring::KeyStore, Config};
use crate::graph::{new_shared_graph, SharedGraph};
use crate::lsp::{LspManager, SharedLspManager};
use crate::mcp::McpManager;
use crate::provider::router::ProviderRouter;
use crate::session::{checkpoint::Checkpoint, Session};
use crate::skills::{shared_registry, SharedSkillRegistry};
use crate::tools::ToolRegistry;

pub struct App {
    pub config: Arc<Config>,
    pub provider_router: Arc<RwLock<ProviderRouter>>,
    pub tools: Arc<ToolRegistry>,
    pub session: Arc<Mutex<Session>>,
    pub key_store: KeyStore,
    pub key_store_shared: Arc<Mutex<KeyStore>>,
    /// Shared semantic code graph (None until /forge-graph has been built)
    pub shared_graph: SharedGraph,
    /// Shared LSP manager — language servers are spawned lazily on first use.
    pub lsp: SharedLspManager,
    pub skills: SharedSkillRegistry,
    /// MCP manager — owns lifecycle of every configured MCP server.
    pub mcp: Arc<McpManager>,
}

impl App {
    pub async fn new(cli: &Cli) -> anyhow::Result<Self> {
        // Load and merge config
        let mut config = Config::load().with_provider_defaults();
        config.merge_env();

        // Apply CLI overrides
        if let Some(theme) = &cli.theme {
            config.general.theme = theme.clone();
        }
        if cli.trust {
            config.general.trust_mode = true;
        }
        if cli.verbose {
            config.general.verbose = true;
        }
        if cli.no_color {
            config.general.theme = "plain".to_string();
        }

        let config = Arc::new(config);

        // Initialize shared graph (loaded from artifact if it exists)
        let shared_graph = new_shared_graph();

        // Initialize key store
        let key_store = KeyStore::new(&config::config_dir());

        // Initialize provider router
        let mut router = ProviderRouter::from_config(&config, &key_store)?;

        // Detect local providers
        router.detect_local_providers(&config).await;

        // Apply CLI provider/model overrides
        if let Some(provider) = &cli.provider {
            if let Some(model) = &cli.model {
                let _ = router.set_active(provider, model);
            } else {
                // Use provider's default model
                if let Some(pc) = config.provider_config(provider) {
                    let _ = router.set_active(provider, &pc.default_model);
                }
            }
        } else if let Some(model) = &cli.model {
            let active_provider = router.active_provider_id().to_string();
            let _ = router.set_active(&active_provider, model);
        }

        let provider_router = Arc::new(RwLock::new(router));

        // Initialize session working directory first (LSP manager needs it).
        let working_dir = cli.dir.clone().unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });

        // Initialize shared LSP manager. Servers still spawn lazily on direct
        // tool use, but we also warm installed project servers in the
        // background so LSP is ready without a manual command.
        let lsp = LspManager::shared(std::path::PathBuf::from(&working_dir));
        {
            let lsp = lsp.clone();
            tokio::spawn(async move {
                let _ = lsp.warm_up_workspace().await;
            });
        }

        // Initialize tools (always register graph_query and lsp_* — they
        // self-disable when no graph / no language server is available).
        let tools = Arc::new(if cli.no_tools {
            ToolRegistry::new()
        } else {
            let registry = ToolRegistry::with_config(&config);
            registry.register_enabled(
                &config,
                Box::new(crate::graph::tools::GraphQueryTool::new_with_lsp(
                    shared_graph.clone(),
                    lsp.clone(),
                )),
            );
            registry.register_enabled(
                &config,
                Box::new(crate::lsp::tools::LspDiagnosticsTool::new(lsp.clone())),
            );
            registry.register_enabled(
                &config,
                Box::new(crate::lsp::tools::LspDefinitionTool::new(lsp.clone())),
            );
            registry.register_enabled(
                &config,
                Box::new(crate::lsp::tools::LspReferencesTool::new(lsp.clone())),
            );
            registry.register_enabled(
                &config,
                Box::new(crate::lsp::tools::LspHoverTool::new(lsp.clone())),
            );
            registry.register_enabled(
                &config,
                Box::new(crate::lsp::tools::LspDocumentSymbolsTool::new(lsp.clone())),
            );
            registry.register_enabled(
                &config,
                Box::new(crate::lsp::tools::LspWorkspaceSymbolsTool::new(lsp.clone())),
            );
            registry.register_enabled(
                &config,
                Box::new(crate::lsp::tools::LspRenameTool::new(lsp.clone())),
            );
            registry
        });

        let mut session = if let Some(resume_arg) = &cli.resume {
            // --resume (no value)        → "__latest__" sentinel → load most recent
            // --resume <id> / <partial>  → load that session (exact id or id prefix,
            //                              falling back to name match)
            let sessions = Checkpoint::list()?;
            let target: Option<String> = if resume_arg == "__latest__" {
                sessions.first().map(|s| s.id.clone())
            } else {
                // exact id → prefix id → name
                sessions
                    .iter()
                    .find(|s| s.id == *resume_arg)
                    .map(|s| s.id.clone())
                    .or_else(|| {
                        sessions
                            .iter()
                            .find(|s| s.id.starts_with(resume_arg.as_str()))
                            .map(|s| s.id.clone())
                    })
                    .or_else(|| {
                        sessions
                            .iter()
                            .find(|s| s.name == *resume_arg)
                            .map(|s| s.id.clone())
                    })
            };

            match target {
                Some(id) => {
                    let mut loaded = Checkpoint::load(&id)?;
                    // Honour any user-supplied working directory override
                    // (--dir) so resumed sessions don't cling to a stale cwd.
                    if cli.dir.is_some() {
                        loaded.working_dir = working_dir.clone();
                    }
                    loaded
                }
                None => {
                    eprintln!("No matching session for '{resume_arg}', starting fresh.");
                    create_new_session(&provider_router, &cli.session, &working_dir).await
                }
            }
        } else if let Some(name) = &cli.session {
            // Try to load named session, or create new
            let sessions = Checkpoint::list()?;
            if let Some(existing) = sessions.iter().find(|s| s.name == *name) {
                Checkpoint::load(&existing.id)?
            } else {
                create_new_session(&provider_router, &Some(name.clone()), &working_dir).await
            }
        } else {
            create_new_session(&provider_router, &None, &working_dir).await
        };

        // Keep the runtime router and persisted session metadata aligned.
        // Resuming/loading a session should route calls to the model shown in
        // the UI; explicit CLI provider/model overrides intentionally win.
        {
            let mut router = provider_router.write().await;
            if cli.provider.is_none() && cli.model.is_none() {
                if router
                    .set_active(&session.provider_id, &session.model_id)
                    .is_err()
                {
                    session.provider_id = router.active_provider_id().to_string();
                    session.model_id = router.active_model_id().to_string();
                }
            } else {
                session.provider_id = router.active_provider_id().to_string();
                session.model_id = router.active_model_id().to_string();
            }
        }

        let session = Arc::new(Mutex::new(session));
        let skills = shared_registry(std::path::Path::new(&working_dir));

        // Try to load graph artifact for the working directory
        {
            let root = std::path::PathBuf::from(&working_dir);
            if let Some(loaded) = crate::graph::CodeGraph::try_load(&root) {
                if let Ok(mut g) = shared_graph.write() {
                    *g = Some(loaded);
                    tracing::info!("forge-graph loaded from artifact");
                }
            }
        }

        // ── MCP manager: load servers from config and connect enabled ones ─
        let key_store_shared = Arc::new(Mutex::new(key_store.clone()));
        let mcp = Arc::new(McpManager::new(key_store_shared.clone(), tools.clone()));
        mcp.load_from_config(&config.mcp.servers).await;
        // Spawn enabled servers in the background so startup is not blocked
        // by a slow npx download. Connections that succeed will register
        // their tools into the shared registry — visible to the agent on
        // the next request.
        {
            let mcp_bg = mcp.clone();
            tokio::spawn(async move {
                mcp_bg.connect_all_enabled().await;
            });
        }

        Ok(Self {
            config,
            provider_router,
            tools,
            session,
            key_store,
            key_store_shared,
            shared_graph,
            lsp,
            skills,
            mcp,
        })
    }

    /// Run interactive TUI mode
    pub async fn run_tui(&self) -> anyhow::Result<()> {
        crate::tui::run_tui(
            self.config.clone(),
            self.provider_router.clone(),
            self.tools.clone(),
            self.session.clone(),
            self.key_store_shared.clone(),
            self.shared_graph.clone(),
            self.lsp.clone(),
            self.skills.clone(),
            self.mcp.clone(),
        )
        .await
    }

    /// Run non-interactive mode with a single prompt
    pub async fn run_once(&self, prompt: String) -> anyhow::Result<()> {
        use crate::agent::{AgentEvent, AgentLoop, PermissionRequest};
        use crate::types::PermissionResponse;
        use tokio::sync::mpsc;

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (perm_tx, _perm_rx) = mpsc::unbounded_channel::<PermissionRequest>();
        let (_, perm_resp_rx) = mpsc::unbounded_channel::<PermissionResponse>();

        use crate::agent::permissions::PermissionStore;
        use crate::session::FileStateCache;
        use crate::types::{PermissionMode, ThinkingConfig};
        use tokio_util::sync::CancellationToken;

        let agent = AgentLoop {
            provider_router: self.provider_router.clone(),
            tools: self.tools.clone(),
            session: self.session.clone(),
            config: self.config.clone(),
            event_tx,
            permission_tx: perm_tx,
            permission_rx: Arc::new(Mutex::new(perm_resp_rx)),
            graph: self.shared_graph.clone(),
            lsp: self.lsp.clone(),
            file_cache: Arc::new(FileStateCache::new()),
            permission_store: Arc::new(parking_lot::RwLock::new(PermissionStore::load())),
            cancel: Arc::new(parking_lot::RwLock::new(CancellationToken::new())),
            permission_mode: Arc::new(parking_lot::RwLock::new(
                if self.config.general.trust_mode {
                    PermissionMode::Bypass
                } else {
                    PermissionMode::Default
                },
            )),
            thinking: Arc::new(parking_lot::RwLock::new(ThinkingConfig::Disabled)),
            skill_registry: self.skills.clone(),
        };

        // Spawn agent
        let handle = tokio::spawn(async move { agent.run(prompt).await });

        // Print streaming output
        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::Token(t) => print!("{t}"),
                AgentEvent::ToolStart { name, .. } => {
                    eprintln!("[Tool: {name}]");
                }
                AgentEvent::ToolEnd {
                    name,
                    output,
                    is_error,
                    ..
                } => {
                    if is_error {
                        eprintln!("[{name} ERROR]: {output}");
                    }
                }
                AgentEvent::Error(e) => eprintln!("Error: {e}"),
                AgentEvent::Done => {
                    println!();
                    break;
                }
                _ => {}
            }
        }

        let _ = handle.await;
        Ok(())
    }

    /// Run a CLI subcommand
    pub async fn run_subcommand(&mut self, cmd: Commands) -> anyhow::Result<()> {
        match cmd {
            Commands::Config { action } => self.handle_config(action).await,
            Commands::Sessions { action } => self.handle_sessions(action).await,
            Commands::Providers { action } => self.handle_providers(action).await,
            Commands::Models { action } => self.handle_models(action).await,
        }
    }

    async fn handle_config(&mut self, action: Option<ConfigAction>) -> anyhow::Result<()> {
        match action {
            None => {
                // Open config in editor
                let path = config::config_dir().join("config.toml");
                println!("Config file: {}", path.display());
                if let Ok(editor) = std::env::var("EDITOR") {
                    let _ = std::process::Command::new(editor).arg(&path).status();
                }
            }
            Some(ConfigAction::Set { key, value }) => {
                let mut config = Config::load().with_provider_defaults();
                set_config_value(&mut config, &key, &value)?;
                config.save()?;
                println!(
                    "{key} = {}",
                    get_config_value(&config, &key).unwrap_or(value)
                );
            }
            Some(ConfigAction::Get { key }) => {
                let config = Config::load().with_provider_defaults();
                if let Some(value) = get_config_value(&config, &key) {
                    println!("{key} = {value}");
                } else {
                    anyhow::bail!("Unknown config key: {key}");
                }
            }
            Some(ConfigAction::Keys { action }) => match action {
                KeyAction::Set { provider, key } => {
                    self.key_store.set(&provider, &key)?;
                    println!("API key set for: {provider}");
                }
                KeyAction::List => {
                    let providers = self.key_store.list_providers();
                    if providers.is_empty() {
                        println!("No API keys configured.");
                        println!("Set one with: forge-osh config keys set <provider> <key>");
                    } else {
                        println!("Configured API keys:");
                        for p in &providers {
                            println!("  - {p}");
                        }
                    }
                    // Also show env vars
                    for pid in config::cloud_provider_ids() {
                        let env = crate::config::keyring::provider_env_var(pid);
                        if std::env::var(&env).is_ok() {
                            println!("  - {pid} (via {env})");
                        }
                    }
                }
                KeyAction::Remove { provider } => {
                    self.key_store.delete(&provider)?;
                    println!("API key removed for: {provider}");
                }
            },
        }
        Ok(())
    }

    async fn handle_sessions(&self, action: SessionAction) -> anyhow::Result<()> {
        match action {
            SessionAction::List => {
                let sessions = Checkpoint::list()?;
                if sessions.is_empty() {
                    println!("No saved sessions.");
                } else {
                    println!(
                        "{:<36} {:<15} {:<8} {:<20} Model",
                        "ID", "Name", "Msgs", "Updated"
                    );
                    println!("{}", "-".repeat(90));
                    for s in &sessions {
                        println!(
                            "{:<36} {:<15} {:<8} {:<20} {}",
                            s.id, s.name, s.message_count, s.updated_at, s.model
                        );
                    }
                }
            }
            SessionAction::Delete { name } => {
                Checkpoint::delete(&name)?;
                println!("Deleted session: {name}");
            }
            SessionAction::Export { name } => {
                let session = Checkpoint::load(&name)?;
                let md = Checkpoint::export_markdown(&session);
                println!("{md}");
            }
        }
        Ok(())
    }

    async fn handle_providers(&self, action: ProviderAction) -> anyhow::Result<()> {
        match action {
            ProviderAction::List => {
                let router = self.provider_router.read().await;
                let providers = router.available_providers();
                if providers.is_empty() {
                    println!("No providers configured.");
                    println!("Set an API key: forge-osh config keys set anthropic <your-key>");
                } else {
                    println!("{:<15} {:<20} Status", "ID", "Name");
                    println!("{}", "-".repeat(50));
                    for (id, name) in &providers {
                        let active = if *id == router.active_provider_id() {
                            " (active)"
                        } else {
                            ""
                        };
                        println!("{:<15} {:<20} Connected{}", id, name, active);
                    }
                }
            }
            ProviderAction::Test { provider } => {
                println!("Testing connection to {provider}...");
                let router = self.provider_router.read().await;
                if router.has_provider(&provider) {
                    println!("{provider}: Connected");
                } else {
                    println!("{provider}: Not configured or no API key");
                }
            }
        }
        Ok(())
    }

    async fn handle_models(&mut self, action: ModelAction) -> anyhow::Result<()> {
        match action {
            ModelAction::List { provider } => {
                let models = if let Some(pid) = provider {
                    crate::config::models::models_for_provider(&pid)
                } else {
                    crate::config::models::builtin_model_catalog()
                };

                println!(
                    "{:<15} {:<40} {:<10} {:<8} Cost (in/out per 1M)",
                    "Provider", "Model", "Context", "Tools"
                );
                println!("{}", "-".repeat(100));
                for m in &models {
                    let ctx = if m.context_window >= 1_000_000 {
                        format!("{}M", m.context_window / 1_000_000)
                    } else {
                        format!("{}K", m.context_window / 1_000)
                    };
                    let tools = if m.supports_tools { "Yes" } else { "No" };
                    let cost = if m.input_cost_per_million == 0.0 {
                        "Free".to_string()
                    } else {
                        format!(
                            "${:.2} / ${:.2}",
                            m.input_cost_per_million, m.output_cost_per_million
                        )
                    };
                    println!(
                        "{:<15} {:<40} {:<10} {:<8} {}",
                        m.provider_id, m.name, ctx, tools, cost
                    );
                }
            }
            ModelAction::Set { provider, model } => {
                let mut config = Config::load().with_provider_defaults();
                set_provider_default_model(&mut config, &provider, &model)?;
                config.general.default_provider = provider.clone();
                config.save()?;

                {
                    let mut router = self.provider_router.write().await;
                    if router.has_provider(&provider) {
                        let _ = router.set_active(&provider, &model);
                    }
                }

                println!("Default model for {provider} set to {model}");
            }
        }
        Ok(())
    }
}

fn set_config_value(config: &mut Config, key: &str, value: &str) -> anyhow::Result<()> {
    match key {
        "theme" | "general.theme" => config.general.theme = value.to_string(),
        "default_provider" | "general.default_provider" => {
            config.general.default_provider = value.to_string();
        }
        "auto_save_sessions" | "general.auto_save_sessions" => {
            config.general.auto_save_sessions = parse_bool(value)?;
        }
        "max_session_history" | "general.max_session_history" => {
            config.general.max_session_history = value.parse()?;
        }
        "trust" | "trust_mode" | "general.trust_mode" => {
            config.general.trust_mode = parse_bool(value)?;
        }
        "verbose" | "general.verbose" => config.general.verbose = parse_bool(value)?,
        "system_prompt_extra" | "general.system_prompt_extra" => {
            config.general.system_prompt_extra = value.to_string();
        }
        "agent.max_tokens" => config.agent.max_tokens = value.parse()?,
        "agent.temperature" => config.agent.temperature = value.parse()?,
        "agent.max_tool_iterations" => config.agent.max_tool_iterations = value.parse()?,
        "agent.planning_mode" => config.agent.planning_mode = parse_bool(value)?,
        "agent.auto_summarize_at" => config.agent.auto_summarize_at = value.parse()?,
        "agent.max_output_per_tool" => config.agent.max_output_per_tool = value.parse()?,
        "agent.skills_enabled" => config.agent.skills_enabled = parse_bool(value)?,
        "agent.include_skills_in_system_prompt" => {
            config.agent.include_skills_in_system_prompt = parse_bool(value)?;
        }
        "agent.max_skill_listed_in_prompt" => {
            config.agent.max_skill_listed_in_prompt = value.parse()?;
        }
        "tools.enabled" => config.tools.enabled = split_list(value),
        "tools.disabled" => config.tools.disabled = split_list(value),
        "tools.bash.timeout_seconds" => config.tools.bash.timeout_seconds = value.parse()?,
        "tools.bash.max_timeout_seconds" => {
            config.tools.bash.max_timeout_seconds = value.parse()?
        }
        "tools.bash.allowed_commands" => config.tools.bash.allowed_commands = split_list(value),
        "tools.bash.blocked_commands" => config.tools.bash.blocked_commands = split_list(value),
        "tools.web.enabled" => config.tools.web.enabled = parse_bool(value)?,
        "tools.web.timeout_seconds" => config.tools.web.timeout_seconds = value.parse()?,
        "tools.web.max_content_length" => config.tools.web.max_content_length = value.parse()?,
        "ui.show_token_count" => config.ui.show_token_count = parse_bool(value)?,
        "ui.show_cost" => config.ui.show_cost = parse_bool(value)?,
        "ui.show_spinner" => config.ui.show_spinner = parse_bool(value)?,
        "ui.syntax_highlight" => config.ui.syntax_highlight = parse_bool(value)?,
        "ui.diff_before_apply" => config.ui.diff_before_apply = parse_bool(value)?,
        "ui.timestamp_messages" => config.ui.timestamp_messages = parse_bool(value)?,
        "ui.compact_tool_output" => config.ui.compact_tool_output = parse_bool(value)?,
        "ui.max_conversation_lines" => config.ui.max_conversation_lines = value.parse()?,
        _ if key.starts_with("providers.") => set_provider_config_value(config, key, value)?,
        _ => anyhow::bail!("Unknown config key: {key}"),
    }
    Ok(())
}

fn get_config_value(config: &Config, key: &str) -> Option<String> {
    Some(match key {
        "theme" | "general.theme" => config.general.theme.clone(),
        "default_provider" | "general.default_provider" => config.general.default_provider.clone(),
        "auto_save_sessions" | "general.auto_save_sessions" => {
            config.general.auto_save_sessions.to_string()
        }
        "max_session_history" | "general.max_session_history" => {
            config.general.max_session_history.to_string()
        }
        "trust" | "trust_mode" | "general.trust_mode" => config.general.trust_mode.to_string(),
        "verbose" | "general.verbose" => config.general.verbose.to_string(),
        "system_prompt_extra" | "general.system_prompt_extra" => {
            config.general.system_prompt_extra.clone()
        }
        "agent.max_tokens" => config.agent.max_tokens.to_string(),
        "agent.temperature" => config.agent.temperature.to_string(),
        "agent.max_tool_iterations" => config.agent.max_tool_iterations.to_string(),
        "agent.planning_mode" => config.agent.planning_mode.to_string(),
        "agent.auto_summarize_at" => config.agent.auto_summarize_at.to_string(),
        "agent.max_output_per_tool" => config.agent.max_output_per_tool.to_string(),
        "agent.skills_enabled" => config.agent.skills_enabled.to_string(),
        "agent.include_skills_in_system_prompt" => {
            config.agent.include_skills_in_system_prompt.to_string()
        }
        "agent.max_skill_listed_in_prompt" => config.agent.max_skill_listed_in_prompt.to_string(),
        "tools.enabled" => config.tools.enabled.join(","),
        "tools.disabled" => config.tools.disabled.join(","),
        "tools.bash.timeout_seconds" => config.tools.bash.timeout_seconds.to_string(),
        "tools.bash.max_timeout_seconds" => config.tools.bash.max_timeout_seconds.to_string(),
        "tools.bash.allowed_commands" => config.tools.bash.allowed_commands.join(","),
        "tools.bash.blocked_commands" => config.tools.bash.blocked_commands.join(","),
        "tools.web.enabled" => config.tools.web.enabled.to_string(),
        "tools.web.timeout_seconds" => config.tools.web.timeout_seconds.to_string(),
        "tools.web.max_content_length" => config.tools.web.max_content_length.to_string(),
        "ui.show_token_count" => config.ui.show_token_count.to_string(),
        "ui.show_cost" => config.ui.show_cost.to_string(),
        "ui.show_spinner" => config.ui.show_spinner.to_string(),
        "ui.syntax_highlight" => config.ui.syntax_highlight.to_string(),
        "ui.diff_before_apply" => config.ui.diff_before_apply.to_string(),
        "ui.timestamp_messages" => config.ui.timestamp_messages.to_string(),
        "ui.compact_tool_output" => config.ui.compact_tool_output.to_string(),
        "ui.max_conversation_lines" => config.ui.max_conversation_lines.to_string(),
        _ if key.starts_with("providers.") => get_provider_config_value(config, key)?,
        _ => return None,
    })
}

fn set_provider_config_value(config: &mut Config, key: &str, value: &str) -> anyhow::Result<()> {
    let mut parts = key.split('.');
    let _providers = parts.next();
    let provider = parts.next().unwrap_or_default();
    let field = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        anyhow::bail!("Unknown config key: {key}");
    }

    if let Some(pc) = provider_config_mut(config, provider) {
        match field {
            "enabled" => pc.enabled = parse_bool(value)?,
            "api_key" => pc.api_key = value.to_string(),
            "default_model" => pc.default_model = value.to_string(),
            "base_url" => pc.base_url = value.to_string(),
            "timeout_seconds" => pc.timeout_seconds = value.parse()?,
            "max_retries" => pc.max_retries = value.parse()?,
            _ => anyhow::bail!("Unknown provider config field: {field}"),
        }
        return Ok(());
    }

    if let Some(pc) = local_provider_config_mut(config, provider) {
        match field {
            "enabled" => pc.enabled = parse_bool(value)?,
            "base_url" => pc.base_url = value.to_string(),
            "default_model" => pc.default_model = value.to_string(),
            "auto_detect" => pc.auto_detect = parse_bool(value)?,
            _ => anyhow::bail!("Unknown local provider config field: {field}"),
        }
        return Ok(());
    }

    anyhow::bail!("Unknown provider: {provider}");
}

fn get_provider_config_value(config: &Config, key: &str) -> Option<String> {
    let mut parts = key.split('.');
    let _providers = parts.next();
    let provider = parts.next().unwrap_or_default();
    let field = parts.next().unwrap_or_default();
    if parts.next().is_some() {
        return None;
    }

    if let Some(pc) = provider_config(config, provider) {
        return Some(match field {
            "enabled" => pc.enabled.to_string(),
            "api_key" => pc.api_key.clone(),
            "default_model" => pc.default_model.clone(),
            "base_url" => pc.base_url.clone(),
            "timeout_seconds" => pc.timeout_seconds.to_string(),
            "max_retries" => pc.max_retries.to_string(),
            _ => return None,
        });
    }

    if let Some(pc) = local_provider_config(config, provider) {
        return Some(match field {
            "enabled" => pc.enabled.to_string(),
            "base_url" => pc.base_url.clone(),
            "default_model" => pc.default_model.clone(),
            "auto_detect" => pc.auto_detect.to_string(),
            _ => return None,
        });
    }

    None
}

fn set_provider_default_model(
    config: &mut Config,
    provider: &str,
    model: &str,
) -> anyhow::Result<()> {
    if let Some(pc) = provider_config_mut(config, provider) {
        pc.default_model = model.to_string();
        return Ok(());
    }
    if let Some(pc) = local_provider_config_mut(config, provider) {
        pc.default_model = model.to_string();
        return Ok(());
    }
    anyhow::bail!("Unknown provider: {provider}");
}

fn provider_config<'a>(config: &'a Config, provider: &str) -> Option<&'a config::ProviderConfig> {
    match provider {
        "anthropic" => Some(&config.providers.anthropic),
        "openai" => Some(&config.providers.openai),
        "gemini" => Some(&config.providers.gemini),
        "groq" => Some(&config.providers.groq),
        "grok" => Some(&config.providers.grok),
        "openrouter" => Some(&config.providers.openrouter),
        "mistral" => Some(&config.providers.mistral),
        "deepseek" => Some(&config.providers.deepseek),
        "together" => Some(&config.providers.together),
        "fireworks" => Some(&config.providers.fireworks),
        "perplexity" => Some(&config.providers.perplexity),
        "cohere" => Some(&config.providers.cohere),
        _ => None,
    }
}

fn provider_config_mut<'a>(
    config: &'a mut Config,
    provider: &str,
) -> Option<&'a mut config::ProviderConfig> {
    match provider {
        "anthropic" => Some(&mut config.providers.anthropic),
        "openai" => Some(&mut config.providers.openai),
        "gemini" => Some(&mut config.providers.gemini),
        "groq" => Some(&mut config.providers.groq),
        "grok" => Some(&mut config.providers.grok),
        "openrouter" => Some(&mut config.providers.openrouter),
        "mistral" => Some(&mut config.providers.mistral),
        "deepseek" => Some(&mut config.providers.deepseek),
        "together" => Some(&mut config.providers.together),
        "fireworks" => Some(&mut config.providers.fireworks),
        "perplexity" => Some(&mut config.providers.perplexity),
        "cohere" => Some(&mut config.providers.cohere),
        _ => None,
    }
}

fn local_provider_config<'a>(
    config: &'a Config,
    provider: &str,
) -> Option<&'a config::LocalProviderConfig> {
    match provider {
        "ollama" => Some(&config.providers.ollama),
        "llamacpp" => Some(&config.providers.llamacpp),
        "lmstudio" => Some(&config.providers.lmstudio),
        "vllm" => Some(&config.providers.vllm),
        "jan" => Some(&config.providers.jan),
        "localai" => Some(&config.providers.localai),
        _ => None,
    }
}

fn local_provider_config_mut<'a>(
    config: &'a mut Config,
    provider: &str,
) -> Option<&'a mut config::LocalProviderConfig> {
    match provider {
        "ollama" => Some(&mut config.providers.ollama),
        "llamacpp" => Some(&mut config.providers.llamacpp),
        "lmstudio" => Some(&mut config.providers.lmstudio),
        "vllm" => Some(&mut config.providers.vllm),
        "jan" => Some(&mut config.providers.jan),
        "localai" => Some(&mut config.providers.localai),
        _ => None,
    }
}

fn parse_bool(value: &str) -> anyhow::Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => anyhow::bail!("Expected boolean value, got '{value}'"),
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

async fn create_new_session(
    router: &Arc<RwLock<ProviderRouter>>,
    name: &Option<String>,
    working_dir: &str,
) -> Session {
    let router = router.read().await;
    let provider_id = router.active_provider_id().to_string();
    let model_id = router.active_model_id().to_string();
    let session_name = name
        .clone()
        .unwrap_or_else(|| chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
    Session::new(session_name, provider_id, model_id, working_dir.to_string())
}
