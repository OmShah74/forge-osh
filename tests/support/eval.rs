#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use forge_agent::agent::permissions::PermissionStore;
use forge_agent::agent::{AgentEvent, AgentLoop, PermissionRequest};
use forge_agent::config::Config;
use forge_agent::error::{ForgeError, Result};
use forge_agent::graph::new_shared_graph;
use forge_agent::lsp::LspManager;
use forge_agent::provider::router::ProviderRouter;
use forge_agent::provider::Provider;
use forge_agent::session::{FileStateCache, Session};
use forge_agent::skills;
use forge_agent::tools::ToolRegistry;
use forge_agent::types::{
    ChatRequest, ChatResponse, CompletionReason, Message, ModelInfo, PermissionMode,
    PermissionResponse, StreamEvent, ThinkingConfig, Usage,
};
use tempfile::TempDir;
use tokio::sync::{mpsc, Mutex as TokioMutex, RwLock};
use tokio_util::sync::CancellationToken;

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("eval env lock poisoned")
}

#[derive(Debug, Clone)]
pub struct ScriptedProvider {
    state: Arc<Mutex<ScriptedProviderState>>,
    id: String,
    name: String,
    model: String,
    context_window: u32,
    supports_tools: bool,
}

#[derive(Debug)]
struct ScriptedProviderState {
    responses: VecDeque<ChatResponse>,
    requests: Vec<ChatRequest>,
    streamed_tokens: Vec<String>,
}

impl ScriptedProvider {
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ScriptedProviderState {
                responses: responses.into(),
                requests: Vec::new(),
                streamed_tokens: Vec::new(),
            })),
            id: "eval".to_string(),
            name: "Eval Mock Provider".to_string(),
            model: "eval-model".to_string(),
            context_window: 128_000,
            supports_tools: true,
        }
    }

    pub fn with_context_window(mut self, context_window: u32) -> Self {
        self.context_window = context_window;
        self
    }

    pub fn without_tools(mut self) -> Self {
        self.supports_tools = false;
        self
    }

    pub fn requests(&self) -> Vec<ChatRequest> {
        self.state.lock().expect("provider lock").requests.clone()
    }

    pub fn streamed_tokens(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("provider lock")
            .streamed_tokens
            .clone()
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(vec![ModelInfo {
            id: self.model.clone(),
            name: self.model.clone(),
            context_window: self.context_window,
            supports_tools: self.supports_tools,
            supports_vision: false,
            input_cost_per_million: 0.0,
            output_cost_per_million: 0.0,
            provider_id: self.id.clone(),
        }])
    }

    async fn chat(
        &self,
        request: ChatRequest,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<ChatResponse> {
        let response = {
            let mut state = self.state.lock().expect("provider lock");
            state.requests.push(request);
            state
                .responses
                .pop_front()
                .ok_or_else(|| ForgeError::provider("scripted provider exhausted"))?
        };

        if let Some(text) = response.content.text() {
            if !text.is_empty() {
                self.state
                    .lock()
                    .expect("provider lock")
                    .streamed_tokens
                    .push(text.to_string());
                let _ = tx.send(StreamEvent::Token(text.to_string()));
            }
        }
        let _ = tx.send(StreamEvent::Usage(response.usage.clone()));
        let _ = tx.send(StreamEvent::Done(response.stop_reason.clone()));
        Ok(response)
    }

    fn supports_tools(&self) -> bool {
        self.supports_tools
    }

    fn supports_vision(&self) -> bool {
        false
    }

    fn context_window(&self) -> u32 {
        self.context_window
    }

    fn input_cost_per_million(&self) -> f64 {
        0.0
    }

    fn output_cost_per_million(&self) -> f64 {
        0.0
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

pub fn text_response(text: impl Into<String>) -> ChatResponse {
    ChatResponse {
        content: forge_agent::types::AssistantContent::Text(text.into()),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        },
        model: "eval-model".to_string(),
        stop_reason: CompletionReason::EndTurn,
    }
}

pub fn tool_use_response(tool_call: forge_agent::types::ToolCall) -> ChatResponse {
    ChatResponse {
        content: forge_agent::types::AssistantContent::ToolUse(vec![tool_call]),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        },
        model: "eval-model".to_string(),
        stop_reason: CompletionReason::ToolUse,
    }
}

pub struct EvalHarness {
    pub _env_guard: std::sync::MutexGuard<'static, ()>,
    pub tempdir: TempDir,
    pub provider: ScriptedProvider,
    pub session: Arc<TokioMutex<Session>>,
    pub agent: Arc<AgentLoop>,
    pub events: mpsc::UnboundedReceiver<AgentEvent>,
    pub permissions: mpsc::UnboundedReceiver<PermissionRequest>,
}

impl EvalHarness {
    pub fn new(responses: Vec<ChatResponse>) -> Self {
        let env_guard = env_lock();
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("FORGE_CONFIG_DIR", tempdir.path().join("config"));
        std::env::set_var("FORGE_DATA_DIR", tempdir.path().join("data"));

        let mut config = Config::default();
        config.general.auto_save_sessions = false;
        config.general.trust_mode = false;
        config.agent.max_tool_iterations = 6;
        config.agent.max_tokens = 1024;
        config.ui.diff_before_apply = true;

        let provider = ScriptedProvider::new(responses);
        let router = ProviderRouter::from_provider("eval", Box::new(provider.clone()));
        let working_dir = tempdir.path().to_string_lossy().to_string();
        let session = Arc::new(TokioMutex::new(Session::new(
            "eval".to_string(),
            "eval".to_string(),
            "eval-model".to_string(),
            working_dir.clone(),
        )));
        let tools = Arc::new(ToolRegistry::with_builtins());
        let (event_tx, event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (perm_tx, perm_req_rx) = mpsc::unbounded_channel::<PermissionRequest>();
        let (_perm_resp_tx, perm_resp_rx) = mpsc::unbounded_channel::<PermissionResponse>();

        let agent = Arc::new(AgentLoop {
            provider_router: Arc::new(RwLock::new(router)),
            tools,
            session: session.clone(),
            config: Arc::new(config),
            event_tx,
            permission_tx: perm_tx,
            permission_rx: Arc::new(TokioMutex::new(perm_resp_rx)),
            graph: new_shared_graph(),
            lsp: LspManager::shared(tempdir.path().to_path_buf()),
            file_cache: Arc::new(FileStateCache::new()),
            permission_store: Arc::new(parking_lot::RwLock::new(PermissionStore::default())),
            cancel: Arc::new(parking_lot::RwLock::new(CancellationToken::new())),
            permission_mode: Arc::new(parking_lot::RwLock::new(PermissionMode::Default)),
            thinking: Arc::new(parking_lot::RwLock::new(ThinkingConfig::Disabled)),
            skill_registry: skills::shared_registry(tempdir.path()),
            output_chunk_tx: None,
        });

        Self {
            _env_guard: env_guard,
            tempdir,
            provider,
            session,
            agent,
            events: event_rx,
            permissions: perm_req_rx,
        }
    }

    pub fn workspace_path(&self, relative: &str) -> std::path::PathBuf {
        self.tempdir.path().join(relative)
    }

    pub async fn history(&self) -> Vec<Message> {
        self.session.lock().await.history.messages().to_vec()
    }
}
