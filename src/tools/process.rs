//! Background process management (P1.4).
//!
//! Lets the agent start long-running processes (dev servers, watchers, build
//! daemons) that keep running *across* turns instead of blocking the turn until
//! they exit. `bash`/`powershell` with `background: true` hand the process off
//! to a session-scoped [`ProcessRegistry`]; the agent then polls and controls
//! them with the `process_status`, `process_logs`, and `process_stop` tools.
//!
//! ## Lifecycle
//! The registry is a process-global singleton (forge-osh runs one interactive
//! session per process), so the same handles are visible to the tool layer and
//! to the TUI statusline without threading state through every `ToolContext`.
//! Each spawned child is monitored by a dedicated task that pumps stdout/stderr
//! into a capped ring log and records the final exit status. [`shutdown_all`]
//! is called on app exit so no orphaned children survive the session.

use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

use super::Tool;
use crate::types::*;

/// Maximum bytes of combined stdout/stderr retained per background process.
/// Older lines are dropped (tail is kept) so a chatty watcher cannot grow
/// memory without bound.
const MAX_LOG_BYTES: usize = 256 * 1024;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ProcessStatus {
    Running,
    Exited(i32),
    /// Stopped on request via `process_stop`.
    Killed,
    /// Never started / failed to spawn or wait.
    Failed(String),
}

impl ProcessStatus {
    pub fn label(&self) -> String {
        match self {
            ProcessStatus::Running => "running".to_string(),
            ProcessStatus::Exited(code) => format!("exited (code {code})"),
            ProcessStatus::Killed => "stopped".to_string(),
            ProcessStatus::Failed(e) => format!("failed ({e})"),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, ProcessStatus::Running)
    }
}

// ---------------------------------------------------------------------------
// Capped log buffer (keeps the most-recent bytes — tail)
// ---------------------------------------------------------------------------

struct ProcessLog {
    /// Completed lines, newest at the back. Each entry already has ANSI codes
    /// stripped and the trailing newline removed.
    lines: VecDeque<String>,
    /// Approximate retained byte count, used to enforce `MAX_LOG_BYTES`.
    bytes: usize,
    /// Total lines ever written (including dropped ones).
    total_lines: usize,
}

impl ProcessLog {
    fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            bytes: 0,
            total_lines: 0,
        }
    }

    fn push_line(&mut self, stream: &str, raw: &[u8]) {
        let cleaned = strip_ansi_escapes::strip(raw);
        let text = String::from_utf8_lossy(&cleaned);
        let text = text.trim_end_matches(['\n', '\r']);
        let line = if stream == "stderr" {
            format!("[stderr] {text}")
        } else {
            text.to_string()
        };
        self.bytes += line.len() + 1;
        self.total_lines += 1;
        self.lines.push_back(line);
        while self.bytes > MAX_LOG_BYTES && self.lines.len() > 1 {
            if let Some(dropped) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(dropped.len() + 1);
            }
        }
    }

    /// Return the last `n` lines (most-recent), oldest-first for display.
    fn tail(&self, n: usize) -> (Vec<String>, usize) {
        let take = n.min(self.lines.len());
        let start = self.lines.len() - take;
        let out: Vec<String> = self.lines.iter().skip(start).cloned().collect();
        let omitted = self.total_lines.saturating_sub(out.len());
        (out, omitted)
    }
}

// ---------------------------------------------------------------------------
// Background process handle
// ---------------------------------------------------------------------------

pub struct BgProcess {
    pub id: String,
    pub command: String,
    pub working_dir: String,
    pub started_at: Instant,
    log: Arc<Mutex<ProcessLog>>,
    status: Arc<Mutex<ProcessStatus>>,
    /// One-shot kill signal consumed by the monitor task. `None` once a stop
    /// has already been requested.
    kill_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl BgProcess {
    pub fn status(&self) -> ProcessStatus {
        self.status.lock().clone()
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Snapshot of the last `n` log lines plus the count of omitted older lines.
    pub fn logs(&self, n: usize) -> (Vec<String>, usize) {
        self.log.lock().tail(n)
    }

    /// Request termination. Returns false if a stop was already requested or the
    /// process is no longer running.
    pub fn request_stop(&self) -> bool {
        if !self.status.lock().is_running() {
            return false;
        }
        if let Some(tx) = self.kill_tx.lock().take() {
            let _ = tx.send(());
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct ProcessRegistry {
    procs: Vec<Arc<BgProcess>>,
    counter: u64,
}

impl ProcessRegistry {
    fn new() -> Self {
        Self {
            procs: Vec::new(),
            counter: 0,
        }
    }

    fn next_id(&mut self) -> String {
        self.counter += 1;
        format!("proc_{}", self.counter)
    }

    pub fn get(&self, id: &str) -> Option<Arc<BgProcess>> {
        self.procs.iter().find(|p| p.id == id).cloned()
    }

    pub fn all(&self) -> Vec<Arc<BgProcess>> {
        self.procs.clone()
    }

    /// Currently-running processes (for the statusline).
    pub fn running(&self) -> Vec<Arc<BgProcess>> {
        self.procs
            .iter()
            .filter(|p| p.status().is_running())
            .cloned()
            .collect()
    }

    pub fn running_count(&self) -> usize {
        self.procs.iter().filter(|p| p.status().is_running()).count()
    }
}

pub type SharedProcessRegistry = Arc<Mutex<ProcessRegistry>>;

static REGISTRY: OnceLock<SharedProcessRegistry> = OnceLock::new();

/// Process-global registry handle. Lazily initialised on first use.
pub fn registry() -> SharedProcessRegistry {
    REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(ProcessRegistry::new())))
        .clone()
}

/// Kill every still-running background process. Call on app shutdown so no
/// orphaned children outlive the session.
pub fn shutdown_all() {
    let reg = registry();
    let procs = reg.lock().all();
    for p in procs {
        p.request_stop();
    }
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// Spawn `command` (via the OS shell) detached in `work_dir`, register it, and
/// return its handle. The process keeps running until it exits on its own or
/// `process_stop` is called. Output is streamed into the handle's capped log by
/// a monitor task.
pub fn spawn_background(
    command: &str,
    work_dir: std::path::PathBuf,
) -> Result<Arc<BgProcess>, String> {
    let (shell, flag) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut cmd = Command::new(shell);
    cmd.arg(flag).arg(command);
    spawn_background_cmd(command.to_string(), cmd, work_dir)
}

/// Spawn an arbitrary pre-configured [`Command`] as a tracked background
/// process. `display` is the human-readable command shown to the agent. Used by
/// the PowerShell tool to run under `powershell.exe`/`pwsh` instead of the
/// default shell.
pub fn spawn_background_cmd(
    display: String,
    mut cmd: Command,
    work_dir: std::path::PathBuf,
) -> Result<Arc<BgProcess>, String> {
    use std::process::Stdio;

    let mut child = cmd
        .current_dir(&work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn background process: {e}"))?;
    let command = display;

    let log = Arc::new(Mutex::new(ProcessLog::new()));
    let status = Arc::new(Mutex::new(ProcessStatus::Running));
    let (kill_tx, kill_rx) = oneshot::channel::<()>();

    let id = {
        let reg = registry();
        let mut guard = reg.lock();
        guard.next_id()
    };

    let proc = Arc::new(BgProcess {
        id: id.clone(),
        command: command.to_string(),
        working_dir: work_dir.to_string_lossy().to_string(),
        started_at: Instant::now(),
        log: log.clone(),
        status: status.clone(),
        kill_tx: Mutex::new(Some(kill_tx)),
    });

    registry().lock().procs.push(proc.clone());

    // Monitor task: pump stdout/stderr into the log, wait for exit (or a kill
    // request), and record the final status.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_log = log.clone();
    let stdout_task = tokio::spawn(async move {
        if let Some(out) = stdout_pipe {
            let mut reader = BufReader::new(out);
            let mut buf = Vec::with_capacity(4096);
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => stdout_log.lock().push_line("stdout", &buf),
                }
            }
        }
    });

    let stderr_log = log.clone();
    let stderr_task = tokio::spawn(async move {
        if let Some(err) = stderr_pipe {
            let mut reader = BufReader::new(err);
            let mut buf = Vec::with_capacity(4096);
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => stderr_log.lock().push_line("stderr", &buf),
                }
            }
        }
    });

    tokio::spawn(async move {
        let final_status = tokio::select! {
            wait_res = child.wait() => match wait_res {
                Ok(code) => ProcessStatus::Exited(code.code().unwrap_or(-1)),
                Err(e) => ProcessStatus::Failed(e.to_string()),
            },
            _ = kill_rx => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                ProcessStatus::Killed
            }
        };
        // Drain remaining buffered output before reporting completion.
        let _ = stdout_task.await;
        let _ = stderr_task.await;
        *status.lock() = final_status;
    });

    Ok(proc)
}

fn format_handle(proc: &BgProcess) -> String {
    format!(
        "Started background process `{}`:\n  $ {}\n  cwd: {}\n\n\
         It keeps running across turns. Control it with:\n\
         - process_status (id: \"{}\") — check if it is still running\n\
         - process_logs   (id: \"{}\") — read its latest output\n\
         - process_stop   (id: \"{}\") — terminate it",
        proc.id, proc.command, proc.working_dir, proc.id, proc.id, proc.id
    )
}

// ---------------------------------------------------------------------------
// process_status
// ---------------------------------------------------------------------------

pub struct ProcessStatusTool;

#[async_trait]
impl Tool for ProcessStatusTool {
    fn name(&self) -> &str {
        "process_status"
    }

    fn description(&self) -> &str {
        "Check background processes started with bash/powershell `background: true`. \
         Pass `id` to inspect one process, or omit it to list all of them with their \
         status (running / exited / stopped), uptime, and command."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Process id (e.g. \"proc_1\"). Omit to list all." }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let reg = registry();
        if let Some(id) = input["id"].as_str().filter(|s| !s.trim().is_empty()) {
            let id = id.trim();
            let proc = match reg.lock().get(id) {
                Some(p) => p,
                None => return ToolOutput::error(format!("No background process with id `{id}`.")),
            };
            ToolOutput::success(format!(
                "Process `{}` — {}\n  command: {}\n  cwd: {}\n  uptime: {}s",
                proc.id,
                proc.status().label(),
                proc.command,
                proc.working_dir,
                proc.uptime_secs(),
            ))
        } else {
            let procs = reg.lock().all();
            if procs.is_empty() {
                return ToolOutput::success(
                    "No background processes have been started this session.",
                );
            }
            let mut out = format!("{} background process(es):\n", procs.len());
            for p in procs {
                out.push_str(&format!(
                    "- `{}` [{}] uptime {}s — {}\n",
                    p.id,
                    p.status().label(),
                    p.uptime_secs(),
                    p.command,
                ));
            }
            ToolOutput::success(out)
        }
    }
}

// ---------------------------------------------------------------------------
// process_logs
// ---------------------------------------------------------------------------

pub struct ProcessLogsTool;

#[async_trait]
impl Tool for ProcessLogsTool {
    fn name(&self) -> &str {
        "process_logs"
    }

    fn description(&self) -> &str {
        "Read the most recent stdout/stderr output of a background process started with \
         bash/powershell `background: true`. Params: `id` (required), `lines` (max lines to \
         return from the tail, default 100). stderr lines are prefixed with [stderr]."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Process id (e.g. \"proc_1\")." },
                "lines": { "type": "integer", "description": "Max tail lines to return (default 100)." }
            },
            "required": ["id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let id = match input["id"].as_str().filter(|s| !s.trim().is_empty()) {
            Some(s) => s.trim(),
            None => return ToolOutput::error("process_logs requires an `id`."),
        };
        let lines = input["lines"].as_u64().unwrap_or(100).clamp(1, 2000) as usize;

        let proc = match registry().lock().get(id) {
            Some(p) => p,
            None => return ToolOutput::error(format!("No background process with id `{id}`.")),
        };

        let (tail, omitted) = proc.logs(lines);
        let mut header = format!("Process `{}` [{}]", proc.id, proc.status().label());
        if omitted > 0 {
            header.push_str(&format!(" — showing last {} line(s), {omitted} older omitted", tail.len()));
        }
        if tail.is_empty() {
            return ToolOutput::success(format!("{header}\n(no output yet)"));
        }
        ToolOutput::success(format!("{header}\n{}", tail.join("\n")))
    }
}

// ---------------------------------------------------------------------------
// process_stop
// ---------------------------------------------------------------------------

pub struct ProcessStopTool;

#[async_trait]
impl Tool for ProcessStopTool {
    fn name(&self) -> &str {
        "process_stop"
    }

    fn description(&self) -> &str {
        "Terminate a background process started with bash/powershell `background: true`. \
         Params: `id` (required). The process tree is killed; its final logs remain readable."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Process id (e.g. \"proc_1\")." }
            },
            "required": ["id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Stopping a process the agent itself started is part of normal
        // long-running-task management; it touches nothing in the workspace.
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        let id = match input["id"].as_str().filter(|s| !s.trim().is_empty()) {
            Some(s) => s.trim(),
            None => return ToolOutput::error("process_stop requires an `id`."),
        };
        let proc = match registry().lock().get(id) {
            Some(p) => p,
            None => return ToolOutput::error(format!("No background process with id `{id}`.")),
        };
        if proc.request_stop() {
            ToolOutput::success(format!(
                "Stopping background process `{id}`. Its logs remain available via process_logs."
            ))
        } else {
            ToolOutput::success(format!(
                "Process `{id}` is not running ({}). Nothing to stop.",
                proc.status().label()
            ))
        }
    }
}

/// Public entry used by the bash tool when `background: true` is requested.
/// Returns the agent-facing handle text.
pub fn start_and_describe(command: &str, work_dir: std::path::PathBuf) -> ToolOutput {
    match spawn_background(command, work_dir) {
        Ok(proc) => ToolOutput::success(format_handle(&proc)),
        Err(e) => ToolOutput::error(e),
    }
}

/// Public entry used by the PowerShell tool when `background: true` is requested.
/// `display` is the user-facing command; `cmd` is the configured executable.
pub fn start_and_describe_cmd(
    display: String,
    cmd: Command,
    work_dir: std::path::PathBuf,
) -> ToolOutput {
    match spawn_background_cmd(display, cmd, work_dir) {
        Ok(proc) => ToolOutput::success(format_handle(&proc)),
        Err(e) => ToolOutput::error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_tail_and_cap() {
        let mut log = ProcessLog::new();
        for i in 0..10 {
            log.push_line("stdout", format!("line {i}\n").as_bytes());
        }
        let (tail, omitted) = log.tail(3);
        assert_eq!(tail, vec!["line 7", "line 8", "line 9"]);
        assert_eq!(omitted, 7);
    }

    #[test]
    fn stderr_prefixed() {
        let mut log = ProcessLog::new();
        log.push_line("stderr", b"boom\n");
        let (tail, _) = log.tail(1);
        assert_eq!(tail, vec!["[stderr] boom"]);
    }
}
