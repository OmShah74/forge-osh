//! Single-server LSP client. Owns the spawned process and exposes async
//! request/notification primitives.
//!
//! Design notes:
//! - Two background tokio tasks: a writer task that drains an mpsc queue of
//!   already-serialized JSON-RPC envelopes onto stdin, and a reader task that
//!   parses Content-Length framed replies from stdout, routes responses to
//!   pending oneshot waiters, and pushes notifications (most importantly
//!   `textDocument/publishDiagnostics`) into a shared cache.
//! - Server stderr is captured in a bounded ring so we can surface useful
//!   error context if the server dies or rejects a request.
//! - We never block tools forever: every request takes a `timeout`.

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex as PLMutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use super::config::{path_to_uri, uri_to_path, ServerSpec};
use super::protocol::{
    self, DidChangeParams, DidCloseParams, DidOpenParams, Notification, PublishDiagnosticsParams,
    Range, ReferenceContext, ReferenceParams, Request, ResponseEnvelope,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, VersionedTextDocumentIdentifier,
};

/// Maximum stderr we'll keep around for diagnostics.
const STDERR_RING_BYTES: usize = 16 * 1024;

#[derive(Debug)]
struct OpenDoc {
    version: i32,
    /// Last text we sent to the server. Used to decide whether didChange is
    /// needed when a tool re-reads from disk.
    text: String,
}

pub struct LspClient {
    pub spec: &'static ServerSpec,
    pub root: PathBuf,

    next_id: AtomicU64,

    /// Pending responses keyed by request id.
    pending: Arc<PLMutex<HashMap<u64, oneshot::Sender<ResponseEnvelope>>>>,

    /// Outgoing queue → writer task.
    out_tx: mpsc::UnboundedSender<Vec<u8>>,

    /// Open document cache (URI → state). Drives didOpen / didChange.
    open_docs: Arc<PLMutex<HashMap<String, OpenDoc>>>,

    /// Cached diagnostics keyed by URI, populated by the reader task.
    diagnostics: Arc<PLMutex<HashMap<String, Vec<protocol::Diagnostic>>>>,

    /// Captured stderr ring.
    stderr_ring: Arc<PLMutex<Vec<u8>>>,

    /// Holds the child + tasks alive. Dropping shuts them down.
    _child: Arc<PLMutex<Option<Child>>>,
    _tasks: Arc<PLMutex<Vec<tokio::task::JoinHandle<()>>>>,

    /// Set once the server has acknowledged `initialize`.
    initialized: Arc<PLMutex<bool>>,
}

impl LspClient {
    /// Spawn the server and drive the initialize handshake.
    pub async fn spawn(spec: &'static ServerSpec, root: PathBuf) -> Result<Self> {
        let cand = super::config::resolve_candidate(spec)
            .ok_or_else(|| anyhow!("no language server found on PATH for {}", spec.language))?;

        let mut cmd = Command::new(cand.program);
        cmd.args(cand.args)
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn {}", cand.program))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("server stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("server stdout missing"))?;
        let stderr = child.stderr.take();

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let pending: Arc<PLMutex<HashMap<u64, oneshot::Sender<ResponseEnvelope>>>> =
            Arc::new(PLMutex::new(HashMap::new()));
        let diagnostics: Arc<PLMutex<HashMap<String, Vec<protocol::Diagnostic>>>> =
            Arc::new(PLMutex::new(HashMap::new()));
        let stderr_ring: Arc<PLMutex<Vec<u8>>> = Arc::new(PLMutex::new(Vec::new()));

        // ── Writer task ────────────────────────────────────────────────────
        let writer_task = {
            let mut stdin = stdin;
            tokio::spawn(async move {
                while let Some(payload) = out_rx.recv().await {
                    if stdin.write_all(&payload).await.is_err() {
                        break;
                    }
                    if stdin.flush().await.is_err() {
                        break;
                    }
                }
            })
        };

        // ── Reader task ────────────────────────────────────────────────────
        let reader_task = {
            let pending = pending.clone();
            let diagnostics = diagnostics.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                loop {
                    match protocol::read_message(&mut reader).await {
                        Ok(Some(env)) => handle_envelope(env, &pending, &diagnostics),
                        Ok(None) => break, // EOF — server exited
                        Err(e) => {
                            warn!("LSP read error: {e}");
                            break;
                        }
                    }
                }
                // Wake any pending requests with a synthetic error so they
                // don't hang if the server crashed.
                let mut p = pending.lock();
                for (_, tx) in p.drain() {
                    let _ = tx.send(ResponseEnvelope {
                        jsonrpc: None,
                        id: None,
                        method: None,
                        params: None,
                        result: None,
                        error: Some(protocol::RpcError {
                            code: -32000,
                            message: "language server exited".to_string(),
                            data: None,
                        }),
                    });
                }
            })
        };

        // ── Stderr drain ───────────────────────────────────────────────────
        let stderr_task = if let Some(stderr) = stderr {
            let ring = stderr_ring.clone();
            Some(tokio::spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut reader = stderr;
                let mut buf = [0u8; 1024];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut r = ring.lock();
                            r.extend_from_slice(&buf[..n]);
                            if r.len() > STDERR_RING_BYTES {
                                let drop = r.len() - STDERR_RING_BYTES;
                                r.drain(..drop);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }))
        } else {
            None
        };

        let mut tasks = vec![writer_task, reader_task];
        if let Some(t) = stderr_task {
            tasks.push(t);
        }

        let client = Self {
            spec,
            root,
            next_id: AtomicU64::new(1),
            pending,
            out_tx,
            open_docs: Arc::new(PLMutex::new(HashMap::new())),
            diagnostics,
            stderr_ring,
            _child: Arc::new(PLMutex::new(Some(child))),
            _tasks: Arc::new(PLMutex::new(tasks)),
            initialized: Arc::new(PLMutex::new(false)),
        };

        client.initialize().await?;
        Ok(client)
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    fn enqueue<T: Serialize>(&self, msg: &T) -> Result<()> {
        let body = serde_json::to_vec(msg)?;
        let mut framed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        framed.extend_from_slice(&body);
        self.out_tx
            .send(framed)
            .map_err(|_| anyhow!("LSP server writer closed"))?;
        Ok(())
    }

    fn notify<T: Serialize>(&self, method: &str, params: T) -> Result<()> {
        let n = Notification {
            jsonrpc: "2.0",
            method,
            params,
        };
        self.enqueue(&n)
    }

    async fn request<T: Serialize>(
        &self,
        method: &str,
        params: T,
        timeout: Duration,
    ) -> Result<ResponseEnvelope> {
        let id = self.next_id();
        let req = Request {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        if let Err(e) = self.enqueue(&req) {
            self.pending.lock().remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(env)) => Ok(env),
            Ok(Err(_)) => Err(anyhow!("LSP response channel dropped")),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err(anyhow!(
                    "LSP request '{method}' timed out after {:?}",
                    timeout
                ))
            }
        }
    }

    // ── Lifecycle ──────────────────────────────────────────────────────────

    async fn initialize(&self) -> Result<()> {
        let root_uri = path_to_uri(&self.root);
        // Conservative client capabilities — request only what we use.
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "forge-osh", "version": env!("CARGO_PKG_VERSION") },
            "rootUri": root_uri,
            "rootPath": self.root.to_string_lossy(),
            "workspaceFolders": [{
                "uri": root_uri,
                "name": self.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            }],
            "capabilities": {
                "textDocument": {
                    "synchronization": { "didSave": true, "willSave": false, "dynamicRegistration": false },
                    "publishDiagnostics": { "relatedInformation": false, "versionSupport": false },
                    "definition": { "linkSupport": false },
                    "references": {},
                    "hover": { "contentFormat": ["plaintext", "markdown"] },
                    "documentSymbol": { "hierarchicalDocumentSymbolSupport": false },
                    "rename": { "prepareSupport": false }
                },
                "workspace": {
                    "workspaceFolders": true,
                    "symbol": {},
                    "configuration": false
                }
            }
        });

        let env = self
            .request("initialize", params, Duration::from_secs(30))
            .await?;
        if let Some(err) = env.error {
            return Err(anyhow!("initialize failed: {} ({})", err.message, err.code));
        }
        self.notify("initialized", json!({}))?;
        *self.initialized.lock() = true;
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        *self.initialized.lock()
    }

    pub fn stderr_tail(&self) -> String {
        let r = self.stderr_ring.lock();
        String::from_utf8_lossy(&r).to_string()
    }

    // ── Document sync ─────────────────────────────────────────────────────

    /// Ensure the document at `path` is open with current disk contents.
    /// Sends didOpen on first call, didChange whenever the on-disk text has
    /// changed since the last sync.
    pub async fn ensure_open(&self, path: &Path) -> Result<String> {
        let uri = path_to_uri(path);
        let text = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read {}", path.display()))?;

        let mut docs = self.open_docs.lock();
        match docs.get_mut(&uri) {
            None => {
                let item = TextDocumentItem {
                    uri: uri.clone(),
                    language_id: self.spec.language_id,
                    version: 1,
                    text: text.clone(),
                };
                self.notify("textDocument/didOpen", DidOpenParams { text_document: item })?;
                docs.insert(
                    uri.clone(),
                    OpenDoc {
                        version: 1,
                        text,
                    },
                );
            }
            Some(state) if state.text != text => {
                state.version += 1;
                state.text = text.clone();
                let params = DidChangeParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version: state.version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent { text }],
                };
                self.notify("textDocument/didChange", params)?;
            }
            Some(_) => {}
        }
        Ok(uri)
    }

    pub async fn close(&self, path: &Path) -> Result<()> {
        let uri = path_to_uri(path);
        let mut docs = self.open_docs.lock();
        if docs.remove(&uri).is_some() {
            drop(docs);
            self.notify(
                "textDocument/didClose",
                DidCloseParams {
                    text_document: TextDocumentIdentifier { uri },
                },
            )?;
        }
        Ok(())
    }

    // ── Read-only requests ────────────────────────────────────────────────

    pub async fn definition(
        &self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<protocol::Location>> {
        let uri = self.ensure_open(path).await?;
        let env = self
            .request(
                "textDocument/definition",
                TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: protocol::Position { line, character },
                },
                Duration::from_secs(15),
            )
            .await?;
        if let Some(err) = env.error {
            return Err(anyhow!("definition failed: {}", err.message));
        }
        Ok(parse_locations(env.result))
    }

    pub async fn references(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<protocol::Location>> {
        let uri = self.ensure_open(path).await?;
        let env = self
            .request(
                "textDocument/references",
                ReferenceParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: protocol::Position { line, character },
                    context: ReferenceContext { include_declaration },
                },
                Duration::from_secs(20),
            )
            .await?;
        if let Some(err) = env.error {
            return Err(anyhow!("references failed: {}", err.message));
        }
        Ok(parse_locations(env.result))
    }

    pub async fn hover(
        &self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<String>> {
        let uri = self.ensure_open(path).await?;
        let env = self
            .request(
                "textDocument/hover",
                TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: protocol::Position { line, character },
                },
                Duration::from_secs(15),
            )
            .await?;
        if let Some(err) = env.error {
            return Err(anyhow!("hover failed: {}", err.message));
        }
        Ok(parse_hover(env.result))
    }

    pub async fn document_symbols(
        &self,
        path: &Path,
    ) -> Result<Vec<DocumentSymbolInfo>> {
        let uri = self.ensure_open(path).await?;
        let env = self
            .request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
                Duration::from_secs(20),
            )
            .await?;
        if let Some(err) = env.error {
            return Err(anyhow!("documentSymbol failed: {}", err.message));
        }
        Ok(parse_document_symbols(env.result))
    }

    pub async fn workspace_symbols(&self, query: &str) -> Result<Vec<protocol::SymbolInformation>> {
        let env = self
            .request(
                "workspace/symbol",
                protocol::WorkspaceSymbolParams { query },
                Duration::from_secs(20),
            )
            .await?;
        if let Some(err) = env.error {
            return Err(anyhow!("workspace/symbol failed: {}", err.message));
        }
        let mut out = Vec::new();
        if let Some(Value::Array(arr)) = env.result {
            for v in arr {
                if let Ok(s) = serde_json::from_value::<protocol::SymbolInformation>(v) {
                    out.push(s);
                }
            }
        }
        Ok(out)
    }

    pub async fn rename(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<WorkspaceEditPreview> {
        let uri = self.ensure_open(path).await?;
        let env = self
            .request(
                "textDocument/rename",
                protocol::RenameParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: protocol::Position { line, character },
                    new_name: new_name.to_string(),
                },
                Duration::from_secs(30),
            )
            .await?;
        if let Some(err) = env.error {
            return Err(anyhow!("rename failed: {}", err.message));
        }
        Ok(parse_workspace_edit(env.result))
    }

    /// Snapshot diagnostics for `path`. Diagnostics are pushed asynchronously
    /// by the server, so we open the document and wait briefly for the first
    /// publishDiagnostics to arrive (or return whatever's cached).
    pub async fn diagnostics_for(
        &self,
        path: &Path,
        wait: Duration,
    ) -> Result<Vec<protocol::Diagnostic>> {
        let uri = self.ensure_open(path).await?;
        let deadline = std::time::Instant::now() + wait;
        loop {
            {
                let d = self.diagnostics.lock();
                if let Some(v) = d.get(&uri) {
                    return Ok(v.clone());
                }
            }
            if std::time::Instant::now() >= deadline {
                return Ok(Vec::new());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Snapshot all currently-cached diagnostics across the workspace.
    pub fn all_diagnostics(&self) -> HashMap<String, Vec<protocol::Diagnostic>> {
        self.diagnostics.lock().clone()
    }

    pub async fn shutdown(&self) {
        let _ = self
            .request("shutdown", json!(null), Duration::from_secs(5))
            .await;
        let _ = self.notify("exit", json!(null));
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn handle_envelope(
    env: ResponseEnvelope,
    pending: &Arc<PLMutex<HashMap<u64, oneshot::Sender<ResponseEnvelope>>>>,
    diagnostics: &Arc<PLMutex<HashMap<String, Vec<protocol::Diagnostic>>>>,
) {
    // Notification?
    if env.id.is_none() {
        if let Some(method) = &env.method {
            if method == "textDocument/publishDiagnostics" {
                if let Some(p) = env.params.clone() {
                    if let Ok(parsed) = serde_json::from_value::<PublishDiagnosticsParams>(p) {
                        diagnostics.lock().insert(parsed.uri, parsed.diagnostics);
                    }
                }
            }
            // Other notifications (window/logMessage, $/progress, etc.) are ignored.
        }
        return;
    }

    // Response with numeric id
    if let Some(Value::Number(n)) = &env.id {
        if let Some(id) = n.as_u64() {
            if let Some(tx) = pending.lock().remove(&id) {
                let _ = tx.send(env);
            }
            return;
        }
    }
    debug!("LSP message with non-numeric id ignored: {:?}", env.id);
}

fn parse_locations(result: Option<Value>) -> Vec<protocol::Location> {
    let v = match result {
        Some(v) => v,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    match v {
        Value::Array(arr) => {
            for item in arr {
                push_location(&mut out, item);
            }
        }
        Value::Object(_) => {
            push_location(&mut out, v);
        }
        _ => {}
    }
    out
}

fn push_location(out: &mut Vec<protocol::Location>, v: Value) {
    // Could be a Location or a LocationLink.
    if let Ok(loc) = serde_json::from_value::<protocol::Location>(v.clone()) {
        out.push(loc);
        return;
    }
    // LocationLink: { targetUri, targetSelectionRange | targetRange }
    let uri = v.get("targetUri").and_then(|x| x.as_str()).map(|s| s.to_string());
    let range = v
        .get("targetSelectionRange")
        .or_else(|| v.get("targetRange"))
        .cloned()
        .and_then(|r| serde_json::from_value::<Range>(r).ok());
    if let (Some(uri), Some(range)) = (uri, range) {
        out.push(protocol::Location { uri, range });
    }
}

fn parse_hover(result: Option<Value>) -> Option<String> {
    let v = result?;
    let contents = v.get("contents")?;
    extract_markup(contents)
}

fn extract_markup(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => {
            // MarkupContent or MarkedString { language, value }
            map.get("value")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        }
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(s) = extract_markup(item) {
                    parts.push(s);
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n\n"))
            }
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct DocumentSymbolInfo {
    pub name: String,
    pub kind: u32,
    pub range: Range,
    pub container: Option<String>,
}

fn parse_document_symbols(result: Option<Value>) -> Vec<DocumentSymbolInfo> {
    let v = match result {
        Some(v) => v,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Value::Array(arr) = v {
        for item in arr {
            // Could be SymbolInformation or hierarchical DocumentSymbol.
            if let Some(name) = item.get("name").and_then(|x| x.as_str()) {
                let kind = item.get("kind").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                // Prefer SymbolInformation.location.range; fall back to DocumentSymbol.range.
                let range = item
                    .get("location")
                    .and_then(|l| l.get("range"))
                    .or_else(|| item.get("range"))
                    .or_else(|| item.get("selectionRange"))
                    .cloned()
                    .and_then(|r| serde_json::from_value::<Range>(r).ok());
                let container = item
                    .get("containerName")
                    .and_then(|x| x.as_str())
                    .map(String::from);
                if let Some(range) = range {
                    out.push(DocumentSymbolInfo {
                        name: name.to_string(),
                        kind,
                        range,
                        container,
                    });
                }
            }
        }
    }
    out
}

#[derive(Debug, Default, Clone)]
pub struct WorkspaceEditPreview {
    pub edits_by_path: HashMap<PathBuf, Vec<TextEdit>>,
}

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

fn parse_workspace_edit(result: Option<Value>) -> WorkspaceEditPreview {
    let mut out = WorkspaceEditPreview::default();
    let Some(v) = result else { return out; };

    // Prefer documentChanges (newer), fall back to changes map.
    if let Some(doc_changes) = v.get("documentChanges").and_then(|x| x.as_array()) {
        for entry in doc_changes {
            // TextDocumentEdit { textDocument, edits }
            let uri = entry
                .get("textDocument")
                .and_then(|td| td.get("uri"))
                .and_then(|u| u.as_str());
            let edits_arr = entry.get("edits").and_then(|e| e.as_array());
            if let (Some(uri), Some(edits_arr)) = (uri, edits_arr) {
                let path = uri_to_path(uri);
                let bucket = out.edits_by_path.entry(path).or_default();
                for e in edits_arr {
                    if let Some(te) = parse_text_edit(e) {
                        bucket.push(te);
                    }
                }
            }
        }
        return out;
    }

    if let Some(changes) = v.get("changes").and_then(|x| x.as_object()) {
        for (uri, edits) in changes {
            let path = uri_to_path(uri);
            let bucket = out.edits_by_path.entry(path).or_default();
            if let Some(arr) = edits.as_array() {
                for e in arr {
                    if let Some(te) = parse_text_edit(e) {
                        bucket.push(te);
                    }
                }
            }
        }
    }
    out
}

fn parse_text_edit(v: &Value) -> Option<TextEdit> {
    let range = serde_json::from_value::<Range>(v.get("range")?.clone()).ok()?;
    let new_text = v.get("newText")?.as_str()?.to_string();
    Some(TextEdit { range, new_text })
}
