//! Per-workspace, per-language LSP client cache. The manager is shared via
//! `SharedLspManager = Arc<LspManager>` across the agent loop, tools, and
//! TUI. It is intentionally cheap to clone: all real state lives behind an
//! async RwLock so concurrent tools can share a single warm server.
//!
//! Lazy lifecycle: nothing runs until a tool first asks for it. After that
//! the server lives until process exit (or `/lsp shutdown`).

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::client::LspClient;
use super::config::{
    builtin_servers, detect_project_root, resolve_candidate, server_for_language, server_for_path,
    ServerSpec,
};

pub type SharedLspManager = Arc<LspManager>;

pub struct LspManager {
    /// Workspace root used as the default for newly-spawned servers.
    pub workspace_root: PathBuf,
    /// Per-language client cache. The key is `ServerSpec::language`.
    clients: RwLock<HashMap<&'static str, Arc<LspClient>>>,
}

impl LspManager {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            clients: RwLock::new(HashMap::new()),
        }
    }

    pub fn shared(workspace_root: PathBuf) -> SharedLspManager {
        Arc::new(Self::new(workspace_root))
    }

    /// Return a client suitable for `path`, spawning if needed. Returns
    /// `Err` if no language server is registered or installed.
    pub async fn client_for_path(&self, path: &Path) -> Result<Arc<LspClient>> {
        let spec = server_for_path(path).ok_or_else(|| {
            anyhow!(
                "no LSP server registered for file extension '{}'",
                path.extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_else(|| "<none>".into())
            )
        })?;
        self.client_for_spec(spec).await
    }

    pub async fn client_for_language(&self, language: &str) -> Result<Arc<LspClient>> {
        let spec = server_for_language(language)
            .ok_or_else(|| anyhow!("unknown language: {language}"))?;
        self.client_for_spec(spec).await
    }

    async fn client_for_spec(&self, spec: &'static ServerSpec) -> Result<Arc<LspClient>> {
        // Fast path: already running.
        {
            let r = self.clients.read().await;
            if let Some(c) = r.get(spec.language) {
                return Ok(c.clone());
            }
        }

        // Spawn. We hold the write lock across spawn+initialize so two
        // concurrent first-use callers don't both spawn the server.
        let mut w = self.clients.write().await;
        if let Some(c) = w.get(spec.language) {
            return Ok(c.clone());
        }

        if resolve_candidate(spec).is_none() {
            let names: Vec<&str> = spec.candidates.iter().map(|c| c.program).collect();
            return Err(anyhow!(
                "no language server installed for {} — tried: {}. Install one and retry.",
                spec.language,
                names.join(", ")
            ));
        }

        let root = detect_project_root(&self.workspace_root, spec.root_markers);
        let client = LspClient::spawn(spec, root).await?;
        let arc = Arc::new(client);
        w.insert(spec.language, arc.clone());
        Ok(arc)
    }

    /// List currently-running clients (for `/lsp status`).
    pub async fn running_clients(&self) -> Vec<RunningClientInfo> {
        let r = self.clients.read().await;
        r.values()
            .map(|c| RunningClientInfo {
                language: c.spec.language.to_string(),
                root: c.root.clone(),
                initialized: c.is_initialized(),
            })
            .collect()
    }

    /// List languages we support and the install status of their servers.
    pub fn list_supported() -> Vec<SupportedLanguageInfo> {
        builtin_servers()
            .iter()
            .map(|s| SupportedLanguageInfo {
                language: s.language.to_string(),
                extensions: s.extensions.iter().map(|e| e.to_string()).collect(),
                candidates: s.candidates.iter().map(|c| c.program.to_string()).collect(),
                installed: resolve_candidate(s).map(|c| c.program.to_string()),
            })
            .collect()
    }

    /// Shut down all running servers. Best-effort.
    pub async fn shutdown_all(&self) {
        let mut w = self.clients.write().await;
        for (_, c) in w.drain() {
            c.shutdown().await;
        }
    }

    /// Shut down one language server.
    pub async fn shutdown_language(&self, language: &str) -> Result<()> {
        let mut w = self.clients.write().await;
        match w.remove(language) {
            Some(c) => {
                c.shutdown().await;
                Ok(())
            }
            None => Err(anyhow!("no running server for {language}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunningClientInfo {
    pub language: String,
    pub root: PathBuf,
    pub initialized: bool,
}

#[derive(Debug, Clone)]
pub struct SupportedLanguageInfo {
    pub language: String,
    pub extensions: Vec<String>,
    pub candidates: Vec<String>,
    /// Some(program) if at least one candidate is on PATH.
    pub installed: Option<String>,
}
