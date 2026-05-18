//! `/goal` primitive — durable, verifiable, autonomous objectives.
//!
//! See `future_plan/05_goal_integration.md` for the full design. Phase 1
//! (this module) provides the type surface, persistence layer and a
//! multi-goal supervisor with placeholder workers. Phase 2 fills in the
//! autonomous LLM loop, Phase 3 adds verifier execution, Phase 4 adds the
//! polished TUI UX.
//!
//! Per the user's directive:
//!   - multi-goal from day 1 (HashMap<GoalId, GoalHandle>, no single-active)
//!   - NO cost limit (Budget has no max_usd; cost is observed, never enforced)
//!   - per-goal cost is tracked in GoalMetrics for display only

pub mod persistence;
pub mod policy;
pub mod prompt;
pub mod resumer;
pub mod supervisor;
pub mod verifier;
pub mod worker;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// A goal identifier. Short, URL-safe, sortable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GoalId(pub String);

impl GoalId {
    /// Generate a fresh id: 8 hex chars of randomness prefixed with a
    /// 6-char base-36 timestamp. Sortable by creation order, short enough
    /// to type, collision-resistant enough for an interactive CLI.
    pub fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let ts36 = base36(ts);
        use rand::RngCore;
        let mut rnd = [0u8; 4];
        rand::thread_rng().fill_bytes(&mut rnd);
        let hex: String = rnd.iter().map(|b| format!("{:02x}", b)).collect();
        Self(format!("{ts36}-{hex}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for GoalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn base36(mut n: u64) -> String {
    const ALPH: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(ALPH[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

// ---------------------------------------------------------------------------
// GoalSpec
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSpec {
    pub id: GoalId,
    pub objective: String,
    #[serde(default)]
    pub stopping_condition: String,
    #[serde(default)]
    pub verifiers: Vec<Verifier>,
    #[serde(default)]
    pub budget: Budget,
    #[serde(default)]
    pub policy: Policy,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub seed_files: Vec<PathBuf>,
    pub workdir: PathBuf,
}

impl GoalSpec {
    /// Construct a minimum-viable spec from a free-text objective. The
    /// stopping condition defaults to the objective itself (the model
    /// decides "done" until verifiers are added).
    pub fn from_objective(objective: impl Into<String>, workdir: PathBuf) -> Self {
        let objective = objective.into();
        Self {
            id: GoalId::new(),
            stopping_condition: objective.clone(),
            objective,
            verifiers: Vec::new(),
            budget: Budget::default(),
            policy: Policy::default(),
            created_at: Utc::now(),
            seed_files: Vec::new(),
            workdir,
        }
    }
}

// ---------------------------------------------------------------------------
// Verifiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Verifier {
    Shell {
        cmd: String,
        #[serde(default)]
        expect_exit: i32,
        #[serde(default)]
        expect_stdout_contains: Option<String>,
    },
    FileExists {
        path: PathBuf,
    },
    FileContains {
        path: PathBuf,
        needle: String,
    },
    NoUncommittedFiles {
        #[serde(default)]
        except: Vec<String>,
    },
    Custom {
        name: String,
        cmd: String,
    },
}

impl Verifier {
    pub fn short_label(&self) -> String {
        match self {
            Verifier::Shell { cmd, .. } => format!("shell: {}", truncate(cmd, 60)),
            Verifier::FileExists { path } => format!("exists: {}", path.display()),
            Verifier::FileContains { path, .. } => format!("contains: {}", path.display()),
            Verifier::NoUncommittedFiles { .. } => "clean git tree".into(),
            Verifier::Custom { name, .. } => format!("custom: {name}"),
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

// ---------------------------------------------------------------------------
// Budget — no max_usd per user directive
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default, with = "humantime_serde_opt")]
    pub max_wall: Option<Duration>,
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_turns: Some(200),
            max_wall: Some(Duration::from_secs(4 * 60 * 60)),
            max_input_tokens: None,
            max_output_tokens: None,
        }
    }
}

mod humantime_serde_opt {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(v: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(d) => s.serialize_some(&d.as_secs()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let raw: Option<u64> = Option::deserialize(d)?;
        Ok(raw.map(Duration::from_secs))
    }
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoApprove {
    /// Read-only ops only; mutating tools always denied unattended.
    ReadOnly,
    /// Mutating tools allowed within `write_globs`; shell within `shell_allowlist`.
    AllowedTools,
    /// Everything auto-approved. Use with caution.
    All,
}

impl Default for AutoApprove {
    fn default() -> Self {
        AutoApprove::AllowedTools
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default = "default_true")]
    pub network: bool,
    #[serde(default)]
    pub auto_approve: AutoApprove,
    #[serde(default)]
    pub write_globs: Vec<String>,
    #[serde(default = "default_deny_globs")]
    pub deny_globs: Vec<String>,
    #[serde(default = "default_shell_allowlist")]
    pub shell_allowlist: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            network: true,
            auto_approve: AutoApprove::default(),
            write_globs: Vec::new(),
            deny_globs: default_deny_globs(),
            shell_allowlist: default_shell_allowlist(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_deny_globs() -> Vec<String> {
    vec![".git/**".into(), "**/keys.json".into(), "**/.env".into()]
}

fn default_shell_allowlist() -> Vec<String> {
    vec![
        r"^cargo\s+(build|check|test|clippy|fmt)\b".into(),
        r"^git\s+(status|diff|log|add|commit|branch|show)\b".into(),
        r"^npm\s+(test|run\s+build|run\s+lint)\b".into(),
        r"^pnpm\s+(test|build|lint)\b".into(),
        r"^pytest\b".into(),
        r"^ls\b".into(),
    ]
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", content = "detail")]
pub enum GoalState {
    Idle,
    Running,
    Paused,
    Verifying,
    Blocked(String),
    Completed,
    Cleared,
    Failed(String),
}

impl GoalState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            GoalState::Completed | GoalState::Cleared | GoalState::Failed(_)
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            GoalState::Idle => "idle",
            GoalState::Running => "running",
            GoalState::Paused => "paused",
            GoalState::Verifying => "verifying",
            GoalState::Blocked(_) => "blocked",
            GoalState::Completed => "completed",
            GoalState::Cleared => "cleared",
            GoalState::Failed(_) => "failed",
        }
    }
}

// ---------------------------------------------------------------------------
// Events emitted by the worker — TUI consumes these
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GoalEvent {
    Started {
        id: GoalId,
    },
    Checkpoint(Checkpoint),
    VerifierResult {
        name: String,
        pass: bool,
        summary: String,
    },
    Progress {
        line: String,
    },
    Blocked {
        reason: String,
    },
    StateChanged(GoalState),
    Completed {
        metrics: GoalMetrics,
    },
    BudgetWarn {
        kind: String,
        used: f64,
        limit: f64,
    },
}

// ---------------------------------------------------------------------------
// Control messages sent into the worker
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum GoalControl {
    Pause,
    Resume,
    Clear,
    VerifyNow,
    ForceComplete,
    StatusReq(tokio::sync::oneshot::Sender<StatusSnapshot>),
}

// ---------------------------------------------------------------------------
// Checkpoint + Metrics + Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub at: DateTime<Utc>,
    pub turn: u32,
    pub phase: String,
    pub last_action: String,
    pub files_touched: Vec<PathBuf>,
    pub progress_blurb: String,
    pub metrics: GoalMetrics,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalMetrics {
    pub turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cost is observed only — never enforced as a limit.
    pub cost_usd: f64,
    pub wall_secs: u64,
    pub verifiers_passed: u32,
    pub verifiers_failed: u32,
    pub progress_lines: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub id: GoalId,
    pub state: GoalState,
    pub spec_objective: String,
    pub spec_stopping: String,
    pub metrics: GoalMetrics,
    pub last_checkpoint: Option<Checkpoint>,
    pub tail_progress: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSummary {
    pub id: GoalId,
    pub state: GoalState,
    pub objective: String,
    pub created_at: DateTime<Utc>,
    pub turns: u32,
    pub cost_usd: f64,
}
