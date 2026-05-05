//! Per-workspace, per-language LSP client cache. The manager is shared via
//! `SharedLspManager = Arc<LspManager>` across the agent loop, tools, and
//! TUI. It is intentionally cheap to clone: all real state lives behind an
//! async RwLock so concurrent tools can share a single warm server.

use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::client::LspClient;
use super::config::{
    detect_project_root, install_command_for_language, load_server_specs, resolve_candidate,
    server_for_language, server_for_path, ServerSpec,
};

pub type SharedLspManager = Arc<LspManager>;

pub struct LspManager {
    /// Workspace root used as the default for newly-spawned servers.
    pub workspace_root: PathBuf,
    specs: Vec<ServerSpec>,
    /// Per-language client cache. The key is `ServerSpec::language`.
    clients: RwLock<HashMap<String, Arc<LspClient>>>,
}

impl LspManager {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            specs: load_server_specs(),
            clients: RwLock::new(HashMap::new()),
        }
    }

    pub fn shared(workspace_root: PathBuf) -> SharedLspManager {
        Arc::new(Self::new(workspace_root))
    }

    /// Return a client suitable for `path`, spawning if needed. Returns
    /// `Err` if no language server is registered or installed.
    pub async fn client_for_path(&self, path: &Path) -> Result<Arc<LspClient>> {
        let spec = server_for_path(path, &self.specs).ok_or_else(|| {
            anyhow!(
                "no LSP server registered for file extension '{}'",
                path.extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_else(|| "<none>".into())
            )
        })?;
        self.client_for_spec(spec.clone()).await
    }

    pub async fn client_for_language(&self, language: &str) -> Result<Arc<LspClient>> {
        let spec = server_for_language(language, &self.specs)
            .ok_or_else(|| anyhow!("unknown language: {language}"))?;
        self.client_for_spec(spec.clone()).await
    }

    async fn client_for_spec(&self, spec: ServerSpec) -> Result<Arc<LspClient>> {
        {
            let r = self.clients.read().await;
            if let Some(c) = r.get(&spec.language) {
                return Ok(c.clone());
            }
        }

        // Hold the write lock across spawn+initialize so concurrent first-use
        // callers do not launch duplicate servers for the same language.
        let mut w = self.clients.write().await;
        if let Some(c) = w.get(&spec.language) {
            return Ok(c.clone());
        }

        if resolve_candidate(&spec).is_none() {
            let names: Vec<&str> = spec.candidates.iter().map(|c| c.program.as_str()).collect();
            return Err(anyhow!(
                "no language server installed for {} - tried: {}. Install one and retry.\nHint: {}",
                spec.language,
                names.join(", "),
                spec.install_hint
            ));
        }

        let root = detect_project_root(&self.workspace_root, &spec.root_markers);
        let client = LspClient::spawn(spec, root).await?;
        let arc = Arc::new(client);
        w.insert(arc.spec.language.clone(), arc.clone());
        Ok(arc)
    }

    /// List currently-running clients (for `/lsp status`).
    pub async fn running_clients(&self) -> Vec<RunningClientInfo> {
        let r = self.clients.read().await;
        r.values()
            .map(|c| RunningClientInfo {
                language: c.spec.language.clone(),
                root: c.root.clone(),
                initialized: c.is_initialized(),
            })
            .collect()
    }

    /// List languages supported by a freshly-loaded registry.
    pub fn list_supported() -> Vec<SupportedLanguageInfo> {
        load_server_specs()
            .iter()
            .map(SupportedLanguageInfo::from_spec)
            .collect()
    }

    /// List languages supported by this manager's registry snapshot.
    pub fn supported_languages(&self) -> Vec<SupportedLanguageInfo> {
        self.specs
            .iter()
            .map(SupportedLanguageInfo::from_spec)
            .collect()
    }

    pub fn language_for_path(&self, path: &Path) -> Option<String> {
        server_for_path(path, &self.specs).map(|s| s.language.clone())
    }

    /// Best-effort background warm-up: find languages present in the current
    /// workspace and start installed servers. Missing servers are simply
    /// reported in the summary; they are not user-facing errors.
    pub async fn warm_up_workspace(&self) -> WarmupSummary {
        let languages = self.detect_project_languages(5000);
        let mut summary = WarmupSummary::default();
        for language in languages {
            summary.merge_one(
                language.clone(),
                self.install_and_start_language(&language).await,
            );
        }
        summary
    }

    pub async fn install_and_start_language(&self, language: &str) -> LanguageInstallResult {
        let language = language.trim().to_ascii_lowercase();
        let Some(spec) = server_for_language(&language, &self.specs) else {
            return LanguageInstallResult::Skipped {
                language,
                reason: "unknown language".to_string(),
            };
        };
        let mut installed_by = None;
        if resolve_candidate(spec).is_none() {
            match self.install_builtin_server(&language).await {
                Ok(Some(display)) => installed_by = Some(display),
                Ok(None) => {
                    return LanguageInstallResult::Skipped {
                        language,
                        reason: format!(
                            "no built-in installer for this language. {}",
                            spec.install_hint
                        ),
                    }
                }
                Err(err) => {
                    return LanguageInstallResult::Failed {
                        language,
                        error: err.to_string(),
                    }
                }
            }
        }

        match self.client_for_language(&language).await {
            Ok(_) => LanguageInstallResult::Started {
                language,
                installed_by,
            },
            Err(err) => LanguageInstallResult::Failed {
                language,
                error: err.to_string(),
            },
        }
    }

    pub async fn install_and_start_detected(&self) -> WarmupSummary {
        let languages = self.detect_project_languages(5000);
        let mut summary = WarmupSummary::default();
        for language in languages {
            summary.merge_one(
                language.clone(),
                self.install_and_start_language(&language).await,
            );
        }
        summary
    }

    async fn install_builtin_server(&self, language: &str) -> Result<Option<String>> {
        let Some(installer) = install_command_for_language(language) else {
            return Ok(None);
        };
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(240),
            tokio::process::Command::new(&installer.program)
                .args(&installer.args)
                .output(),
        )
        .await
        .map_err(|_| anyhow!("installer timed out: {}", installer.display))?
        .map_err(|e| anyhow!("failed to run installer '{}': {e}", installer.display))?;

        if output.status.success() {
            Ok(Some(installer.display))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            Err(anyhow!(
                "installer failed: {}\n{}{}",
                installer.display,
                stdout,
                stderr
            ))
        }
    }

    fn detect_project_languages(&self, max_files: usize) -> Vec<String> {
        let mut languages = Vec::new();
        let mut seen = HashSet::new();
        let walker = ignore::WalkBuilder::new(&self.workspace_root)
            .hidden(false)
            .ignore(true)
            .git_ignore(true)
            .git_exclude(true)
            .parents(true)
            .build();

        for entry in walker.filter_map(|e| e.ok()).take(max_files) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(spec) = server_for_path(path, &self.specs) {
                if seen.insert(spec.language.clone()) {
                    languages.push(spec.language.clone());
                }
            }
        }
        languages
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
    pub file_names: Vec<String>,
    pub candidates: Vec<String>,
    /// Some(program) if at least one candidate is on PATH.
    pub installed: Option<String>,
    pub install_hint: String,
    pub source: String,
}

impl SupportedLanguageInfo {
    fn from_spec(s: &ServerSpec) -> Self {
        Self {
            language: s.language.clone(),
            extensions: s.extensions.clone(),
            file_names: s.file_names.clone(),
            candidates: s.candidates.iter().map(|c| c.program.clone()).collect(),
            installed: resolve_candidate(s).map(|c| c.program),
            install_hint: s.install_hint.clone(),
            source: s.source.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WarmupSummary {
    pub installed: Vec<(String, String)>,
    pub started: Vec<String>,
    pub skipped: Vec<(String, String)>,
}

impl WarmupSummary {
    fn merge_one(&mut self, fallback_language: String, result: LanguageInstallResult) {
        match result {
            LanguageInstallResult::Started {
                language,
                installed_by,
            } => {
                if let Some(display) = installed_by {
                    self.installed.push((language.clone(), display));
                }
                self.started.push(language);
            }
            LanguageInstallResult::Skipped { language, reason } => {
                self.skipped.push((language, reason));
            }
            LanguageInstallResult::Failed { language, error } => {
                self.skipped.push((language, error));
            }
        }
        if self.started.is_empty() && self.skipped.is_empty() {
            self.skipped
                .push((fallback_language, "no result".to_string()));
        }
    }
}

#[derive(Debug, Clone)]
pub enum LanguageInstallResult {
    Started {
        language: String,
        installed_by: Option<String>,
    },
    Skipped {
        language: String,
        reason: String,
    },
    Failed {
        language: String,
        error: String,
    },
}
