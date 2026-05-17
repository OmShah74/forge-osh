//! MCP transport: child-process stdio with line-delimited JSON-RPC.
//!
//! Servers are spawned as separate processes. We write each JSON-RPC frame
//! followed by a newline to stdin and read newline-delimited frames from
//! stdout. Stderr is forwarded to a log channel for diagnostics.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;

use super::protocol::{
    InboundFrame, JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    JSONRPC_VERSION,
};

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("transport closed")]
    Closed,
    #[error("server returned error {code}: {message}")]
    Rpc { code: i32, message: String },
    #[error("timed out waiting for response")]
    Timeout,
    #[error("io error: {0}")]
    Io(String),
    #[error("encode error: {0}")]
    Encode(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("spawn failed: {0}")]
    Spawn(String),
}

impl From<JsonRpcError> for TransportError {
    fn from(e: JsonRpcError) -> Self {
        TransportError::Rpc {
            code: e.code,
            message: e.message,
        }
    }
}

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<JsonRpcResponse>>>>;

/// A live stdio MCP transport. Drop this to kill the child.
pub struct StdioTransport {
    next_id: AtomicI64,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: PendingMap,
    /// Notification stream — one per server. Currently we just drain it to
    /// keep memory bounded; future work can route specific notifications.
    _notif_rx: Arc<Mutex<mpsc::UnboundedReceiver<JsonRpcNotification>>>,
    pub stderr_log: Arc<Mutex<Vec<String>>>,
    child: Arc<Mutex<Child>>,
    request_timeout: Duration,
}

impl StdioTransport {
    pub async fn spawn(
        program: &str,
        args: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&std::path::Path>,
        request_timeout: Duration,
    ) -> Result<Self, TransportError> {
        // On Windows, Rust's Command::new() uses CreateProcessW which does
        // not honour PATHEXT — so a bare program name like "npx" or "uvx"
        // (which are actually `npx.cmd` / `uvx.exe` shims) fails to spawn
        // with ERROR_FILE_NOT_FOUND. We route any non-absolute, non-.exe
        // command through `cmd /C` so the standard cmd.exe resolution
        // (PATHEXT, PATH walk, .bat/.cmd handling) takes effect.
        let mut cmd;
        #[cfg(windows)]
        {
            let needs_shim = !std::path::Path::new(program).is_absolute()
                && !program.to_ascii_lowercase().ends_with(".exe");
            if needs_shim {
                cmd = Command::new("cmd");
                cmd.arg("/C");
                cmd.arg(program);
                cmd.args(args);
            } else {
                cmd = Command::new(program);
                cmd.args(args);
            }
        }
        #[cfg(not(windows))]
        {
            cmd = Command::new(program);
            cmd.args(args);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        if let Some(d) = cwd {
            cmd.current_dir(d);
        }
        // On Windows, prevent flashing console windows.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| TransportError::Spawn(format!("{program}: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TransportError::Spawn("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TransportError::Spawn("no stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| TransportError::Spawn("no stderr".into()))?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (notif_tx, notif_rx) = mpsc::unbounded_channel::<JsonRpcNotification>();
        let stderr_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        // Reader task: parse newline-delimited frames.
        {
            let pending = pending.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            match InboundFrame::parse(trimmed) {
                                Ok(InboundFrame::Response(resp)) => {
                                    if let Some(id) = parse_id(&resp.id) {
                                        let mut map = pending.lock().await;
                                        if let Some(tx) = map.remove(&id) {
                                            let _ = tx.send(resp);
                                        }
                                    }
                                }
                                Ok(InboundFrame::Notification(n)) => {
                                    let _ = notif_tx.send(n);
                                }
                                Ok(InboundFrame::Request(_)) => {
                                    // We don't currently handle server→client
                                    // requests. Silently ignore — most servers
                                    // don't issue them.
                                }
                                Err(e) => {
                                    tracing::warn!("mcp: bad frame: {e}");
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("mcp: stdout read err: {e}");
                            break;
                        }
                    }
                }
                // Wake any pending callers.
                let mut map = pending.lock().await;
                map.clear();
            });
        }

        // Stderr capture task.
        {
            let log = stderr_log.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let mut l = log.lock().await;
                            if l.len() >= 200 {
                                l.remove(0);
                            }
                            l.push(line.trim_end().to_string());
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        Ok(Self {
            next_id: AtomicI64::new(1),
            stdin: Arc::new(Mutex::new(stdin)),
            pending,
            _notif_rx: Arc::new(Mutex::new(notif_rx)),
            stderr_log,
            child: Arc::new(Mutex::new(child)),
            request_timeout,
        })
    }

    pub async fn request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, TransportError> {
        self.request_inner(method, params, Some(self.request_timeout))
            .await
    }

    /// Like `request`, but never times out. Used for `tools/call` so that
    /// slow upstreams (arXiv, large LLM gateways, long Docker pulls, etc.)
    /// don't get cut off by an arbitrary client-side ceiling. If the child
    /// dies the oneshot is dropped and we still return `Closed`; user can
    /// always cancel with Ctrl+C.
    pub async fn request_no_timeout(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, TransportError> {
        self.request_inner(method, params, None).await
    }

    async fn request_inner(
        &self,
        method: &str,
        params: Option<Value>,
        request_timeout: Option<Duration>,
    ) -> Result<Value, TransportError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Value::from(id),
            method: method.to_string(),
            params,
        };
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        let mut frame =
            serde_json::to_string(&req).map_err(|e| TransportError::Encode(e.to_string()))?;
        frame.push('\n');
        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(frame.as_bytes())
                .await
                .map_err(|e| TransportError::Io(e.to_string()))?;
            stdin
                .flush()
                .await
                .map_err(|e| TransportError::Io(e.to_string()))?;
        }

        match request_timeout {
            Some(dur) => match timeout(dur, rx).await {
                Ok(Ok(resp)) => {
                    if let Some(err) = resp.error {
                        return Err(err.into());
                    }
                    Ok(resp.result.unwrap_or(Value::Null))
                }
                Ok(Err(_)) => Err(TransportError::Closed),
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    Err(TransportError::Timeout)
                }
            },
            None => match rx.await {
                Ok(resp) => {
                    if let Some(err) = resp.error {
                        return Err(err.into());
                    }
                    Ok(resp.result.unwrap_or(Value::Null))
                }
                Err(_) => Err(TransportError::Closed),
            },
        }
    }

    pub async fn notify(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), TransportError> {
        let n = JsonRpcNotification {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.to_string(),
            params,
        };
        let mut frame =
            serde_json::to_string(&n).map_err(|e| TransportError::Encode(e.to_string()))?;
        frame.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| TransportError::Io(e.to_string()))?;
        Ok(())
    }

    pub async fn stderr_snapshot(&self) -> Vec<String> {
        self.stderr_log.lock().await.clone()
    }

    pub async fn shutdown(&self) {
        // Best-effort: close stdin to let the child exit cleanly, then kill.
        {
            let mut stdin = self.stdin.lock().await;
            let _ = stdin.shutdown().await;
        }
        let mut child = self.child.lock().await;
        let _ = child.start_kill();
    }
}

fn parse_id(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}
