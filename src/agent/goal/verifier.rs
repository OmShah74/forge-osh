//! Verifier execution.
//!
//! After the model emits `CLAIM_DONE:`, the supervisor calls
//! [`run_all`] to empirically check that the goal's stopping condition is
//! satisfied. Each verifier runs in a separate child process (or via local
//! filesystem inspection) with a strict per-verifier wall clock. The
//! report is persisted to `~/.forge-osh/goals/<id>/verifier_runs/<ts>.json`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use super::persistence as persist;
use super::{GoalId, GoalSpec, Verifier};

const PER_VERIFIER_WALL: Duration = Duration::from_secs(5 * 60);
const STDOUT_EXCERPT_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierResult {
    pub name: String,
    pub passed: bool,
    pub summary: String,
    pub exit_code: Option<i32>,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierReport {
    pub at: chrono::DateTime<Utc>,
    pub results: Vec<VerifierResult>,
}

impl VerifierReport {
    pub fn all_pass(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(|r| r.passed)
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    pub fn failures(&self) -> Vec<&VerifierResult> {
        self.results.iter().filter(|r| !r.passed).collect()
    }

    pub fn pass_count(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    pub fn fail_count(&self) -> usize {
        self.results.iter().filter(|r| !r.passed).count()
    }
}

/// Run every verifier in the spec sequentially. The goal's workdir is the
/// CWD for shell-typed verifiers. File-typed verifiers resolve paths
/// against the workdir as well.
pub async fn run_all(spec: &GoalSpec) -> VerifierReport {
    let mut results = Vec::with_capacity(spec.verifiers.len());
    for v in &spec.verifiers {
        let r = run_one(v, &spec.workdir).await;
        results.push(r);
    }
    VerifierReport {
        at: Utc::now(),
        results,
    }
}

/// Persist the report. Called separately from `run_all` so callers can
/// emit events between gather and write.
pub fn persist_report(id: &GoalId, report: &VerifierReport) -> std::io::Result<()> {
    let dir = persist::verifier_runs_dir(id);
    std::fs::create_dir_all(&dir)?;
    let ts = report.at.format("%Y-%m-%dT%H-%M-%S%.3fZ").to_string();
    let path = dir.join(format!("{ts}.json"));
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    persist::write_atomic(&path, &bytes)
}

// ---------------------------------------------------------------------------
// Per-verifier dispatch
// ---------------------------------------------------------------------------

async fn run_one(v: &Verifier, workdir: &Path) -> VerifierResult {
    let started = Instant::now();
    let (name, outcome) = match v {
        Verifier::Shell {
            cmd,
            expect_exit,
            expect_stdout_contains,
        } => (
            format!("shell `{}`", truncate(cmd, 60)),
            run_shell(cmd, *expect_exit, expect_stdout_contains.as_deref(), workdir).await,
        ),
        Verifier::FileExists { path } => (
            format!("exists `{}`", path.display()),
            check_file_exists(path, workdir).await,
        ),
        Verifier::FileContains { path, needle } => (
            format!("contains `{}` ⊂ `{}`", truncate(needle, 30), path.display()),
            check_file_contains(path, needle, workdir).await,
        ),
        Verifier::NoUncommittedFiles { except } => (
            "git tree clean".to_string(),
            check_git_clean(except, workdir).await,
        ),
        Verifier::Custom { name, cmd } => (
            format!("{name}: `{}`", truncate(cmd, 60)),
            run_shell(cmd, 0, None, workdir).await,
        ),
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    VerifierResult {
        name,
        passed: outcome.passed,
        summary: outcome.summary,
        exit_code: outcome.exit_code,
        stdout_excerpt: outcome.stdout_excerpt,
        stderr_excerpt: outcome.stderr_excerpt,
        duration_ms,
    }
}

struct Outcome {
    passed: bool,
    summary: String,
    exit_code: Option<i32>,
    stdout_excerpt: String,
    stderr_excerpt: String,
}

impl Outcome {
    fn pass(summary: impl Into<String>) -> Self {
        Self {
            passed: true,
            summary: summary.into(),
            exit_code: None,
            stdout_excerpt: String::new(),
            stderr_excerpt: String::new(),
        }
    }
    fn fail(summary: impl Into<String>) -> Self {
        Self {
            passed: false,
            summary: summary.into(),
            exit_code: None,
            stdout_excerpt: String::new(),
            stderr_excerpt: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Shell verifier
// ---------------------------------------------------------------------------

async fn run_shell(
    cmd: &str,
    expect_exit: i32,
    expect_stdout_contains: Option<&str>,
    workdir: &Path,
) -> Outcome {
    let mut command = build_shell_command(cmd);
    command
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return Outcome::fail(format!("spawn failed: {e}")),
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let wait_fut = async move {
        let status = child.wait().await;
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        if let Some(mut s) = stdout {
            let _ = s.read_to_end(&mut stdout_buf).await;
        }
        if let Some(mut s) = stderr {
            let _ = s.read_to_end(&mut stderr_buf).await;
        }
        (status, stdout_buf, stderr_buf)
    };

    let res = timeout(PER_VERIFIER_WALL, wait_fut).await;
    match res {
        Err(_) => Outcome {
            passed: false,
            summary: format!(
                "timed out after {}s",
                PER_VERIFIER_WALL.as_secs()
            ),
            exit_code: None,
            stdout_excerpt: String::new(),
            stderr_excerpt: String::new(),
        },
        Ok((status_res, stdout_buf, stderr_buf)) => {
            let exit_code = status_res.as_ref().ok().and_then(|s| s.code());
            let stdout = String::from_utf8_lossy(&stdout_buf).into_owned();
            let stderr = String::from_utf8_lossy(&stderr_buf).into_owned();
            let exit_ok = exit_code == Some(expect_exit);
            let mut passed = exit_ok;
            let mut summary = if exit_ok {
                format!("exit {expect_exit}")
            } else {
                format!(
                    "exit {} (expected {})",
                    exit_code.map(|i| i.to_string()).unwrap_or_else(|| "?".into()),
                    expect_exit
                )
            };
            if let Some(needle) = expect_stdout_contains {
                if !stdout.contains(needle) {
                    passed = false;
                    summary.push_str(&format!(
                        ", stdout missing expected substring '{}'",
                        truncate(needle, 30)
                    ));
                } else {
                    summary.push_str(", stdout matched");
                }
            }
            Outcome {
                passed,
                summary,
                exit_code,
                stdout_excerpt: excerpt(&stdout),
                stderr_excerpt: excerpt(&stderr),
            }
        }
    }
}

fn build_shell_command(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

fn excerpt(s: &str) -> String {
    if s.len() <= STDOUT_EXCERPT_BYTES {
        return s.to_string();
    }
    let head = &s[..STDOUT_EXCERPT_BYTES.min(s.len())];
    format!("{head}\n…(truncated)")
}

// ---------------------------------------------------------------------------
// File verifiers
// ---------------------------------------------------------------------------

async fn check_file_exists(path: &Path, workdir: &Path) -> Outcome {
    let full = resolve_under(workdir, path);
    match tokio::fs::metadata(&full).await {
        Ok(_) => Outcome::pass(format!("found {}", full.display())),
        Err(e) => Outcome::fail(format!("missing {}: {e}", full.display())),
    }
}

async fn check_file_contains(path: &Path, needle: &str, workdir: &Path) -> Outcome {
    let full = resolve_under(workdir, path);
    match tokio::fs::read(&full).await {
        Err(e) => Outcome::fail(format!("read failed: {e}")),
        Ok(bytes) => {
            let body = String::from_utf8_lossy(&bytes);
            if body.contains(needle) {
                Outcome::pass(format!(
                    "{} contains needle ({} bytes scanned)",
                    full.display(),
                    bytes.len()
                ))
            } else {
                Outcome {
                    passed: false,
                    summary: format!("{} does not contain needle", full.display()),
                    exit_code: None,
                    stdout_excerpt: excerpt(&body),
                    stderr_excerpt: String::new(),
                }
            }
        }
    }
}

fn resolve_under(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

// ---------------------------------------------------------------------------
// git tree-clean verifier
// ---------------------------------------------------------------------------

async fn check_git_clean(except: &[String], workdir: &Path) -> Outcome {
    let mut cmd = Command::new("git");
    cmd.arg("status")
        .arg("--porcelain")
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Outcome::fail(format!("git spawn failed: {e}")),
    };
    let stdout = child.stdout.take();
    let wait_fut = async move {
        let status = child.wait().await;
        let mut buf = Vec::new();
        if let Some(mut s) = stdout {
            let _ = s.read_to_end(&mut buf).await;
        }
        (status, buf)
    };
    let res = timeout(PER_VERIFIER_WALL, wait_fut).await;
    let (status_res, stdout_bytes) = match res {
        Err(_) => {
            return Outcome::fail("git status timed out");
        }
        Ok(x) => x,
    };
    if let Ok(s) = &status_res {
        if !s.success() {
            return Outcome::fail(format!(
                "git exited {}",
                s.code().map(|i| i.to_string()).unwrap_or_else(|| "?".into())
            ));
        }
    } else {
        return Outcome::fail("git wait error");
    }

    let body = String::from_utf8_lossy(&stdout_bytes);
    let except_pats: Vec<glob::Pattern> = except
        .iter()
        .filter_map(|s| glob::Pattern::new(s).ok())
        .collect();
    let mut dirty: Vec<String> = Vec::new();
    for line in body.lines() {
        if line.len() < 3 {
            continue;
        }
        // porcelain format: `XY <path>`
        let path = line[3..].trim();
        let matched_except = except_pats.iter().any(|p| p.matches(path));
        if !matched_except {
            dirty.push(path.to_string());
        }
    }
    if dirty.is_empty() {
        Outcome::pass("git tree is clean")
    } else {
        Outcome {
            passed: false,
            summary: format!("{} uncommitted path(s)", dirty.len()),
            exit_code: None,
            stdout_excerpt: dirty.join("\n"),
            stderr_excerpt: String::new(),
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
