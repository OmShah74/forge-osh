use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::cli::*;
use crate::config::{self, keyring::KeyStore, Config};
use crate::graph::{new_shared_graph, SharedGraph};
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
    /// Shared semantic code graph (None until /forge-graph has been built)
    pub shared_graph: SharedGraph,
    pub skills: SharedSkillRegistry,
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

        // Initialize tools (always register graph_query — it self-disables when no graph)
        let tools = Arc::new(if cli.no_tools {
            ToolRegistry::new()
        } else {
            let mut registry = ToolRegistry::with_builtins();
            registry.register(Box::new(crate::graph::tools::GraphQueryTool::new(
                shared_graph.clone(),
            )));
            registry
        });

        // Initialize session
        let working_dir = cli.dir.clone().unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });

        let session = if let Some(resume_arg) = &cli.resume {
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

        Ok(Self {
            config,
            provider_router,
            tools,
            session,
            key_store,
            shared_graph,
            skills,
        })
    }

    /// Run interactive TUI mode
    pub async fn run_tui(&self) -> anyhow::Result<()> {
        crate::tui::run_tui(
            self.config.clone(),
            self.provider_router.clone(),
            self.tools.clone(),
            self.session.clone(),
            Arc::new(Mutex::new(self.key_store.clone())),
            self.shared_graph.clone(),
            self.skills.clone(),
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
                println!("Set {key} = {value}");
                // Would update config.toml here
            }
            Some(ConfigAction::Get { key }) => {
                println!("Config key: {key}");
                // Would read from config.toml
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

    async fn handle_models(&self, action: ModelAction) -> anyhow::Result<()> {
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
                println!("Set default model for {provider}: {model}");
                // Would update config here
            }
        }
        Ok(())
    }
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
