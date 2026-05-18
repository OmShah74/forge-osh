//! On-disk layout and atomic-write helpers for the goal subsystem.
//!
//! Layout (per design doc, multi-goal variant):
//!
//! ```text
//! ~/.forge-osh/goals/
//!   index.json
//!   <goal_id>/
//!     spec.toml
//!     transcript.jsonl
//!     progress.log
//!     metrics.json
//!     checkpoints/
//!       latest.json
//!       <iso_ts>.json
//!     verifier_runs/
//!       <iso_ts>.json
//!   _archive/
//!     <goal_id>/...
//! ```

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Checkpoint, GoalId, GoalMetrics, GoalSpec, GoalState, GoalSummary};

const MAX_CHECKPOINT_FILES: usize = 50;
const MAX_PROGRESS_LINES_RETURN: usize = 200;

// ---------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------

/// `~/.forge-osh/goals/`
pub fn goals_root() -> PathBuf {
    crate::config::config_dir().join("goals")
}

pub fn archive_root() -> PathBuf {
    goals_root().join("_archive")
}

pub fn goal_dir(id: &GoalId) -> PathBuf {
    goals_root().join(id.as_str())
}

pub fn checkpoints_dir(id: &GoalId) -> PathBuf {
    goal_dir(id).join("checkpoints")
}

pub fn verifier_runs_dir(id: &GoalId) -> PathBuf {
    goal_dir(id).join("verifier_runs")
}

pub fn ensure_goal_dirs(id: &GoalId) -> std::io::Result<()> {
    fs::create_dir_all(checkpoints_dir(id))?;
    fs::create_dir_all(verifier_runs_dir(id))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Atomic write
// ---------------------------------------------------------------------------

/// Write `bytes` to `path` via tempfile + rename so readers never see a
/// torn file. Creates parent dirs as needed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp.{}",
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("bin"),
        std::process::id()
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn read_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut f = fs::File::open(path)?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)?;
    Ok(v)
}

pub fn read_string(path: &Path) -> std::io::Result<String> {
    let bytes = read_bytes(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

// ---------------------------------------------------------------------------
// IndexFile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexFile {
    #[serde(default)]
    pub goals: Vec<GoalSummary>,
}

impl IndexFile {
    pub fn path() -> PathBuf {
        goals_root().join("index.json")
    }

    pub fn load_or_default() -> Self {
        let p = Self::path();
        if !p.exists() {
            return Self::default();
        }
        match read_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(io_err)?;
        write_atomic(&Self::path(), &bytes)
    }

    pub fn upsert(&mut self, summary: GoalSummary) {
        if let Some(slot) = self.goals.iter_mut().find(|g| g.id == summary.id) {
            *slot = summary;
        } else {
            self.goals.push(summary);
        }
    }

    pub fn remove(&mut self, id: &GoalId) {
        self.goals.retain(|g| &g.id != id);
    }
}

fn io_err<E: std::error::Error + Send + Sync + 'static>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e)
}

// ---------------------------------------------------------------------------
// Spec
// ---------------------------------------------------------------------------

pub fn save_spec(spec: &GoalSpec) -> std::io::Result<()> {
    ensure_goal_dirs(&spec.id)?;
    let toml_str = toml::to_string_pretty(spec).map_err(io_err)?;
    write_atomic(&goal_dir(&spec.id).join("spec.toml"), toml_str.as_bytes())
}

pub fn load_spec(id: &GoalId) -> std::io::Result<GoalSpec> {
    let p = goal_dir(id).join("spec.toml");
    let s = read_string(&p)?;
    toml::from_str(&s).map_err(io_err)
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

pub fn metrics_path(id: &GoalId) -> PathBuf {
    goal_dir(id).join("metrics.json")
}

pub fn save_metrics(id: &GoalId, m: &GoalMetrics) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(m).map_err(io_err)?;
    write_atomic(&metrics_path(id), &bytes)
}

pub fn load_metrics(id: &GoalId) -> std::io::Result<GoalMetrics> {
    let p = metrics_path(id);
    if !p.exists() {
        return Ok(GoalMetrics::default());
    }
    let s = read_string(&p)?;
    serde_json::from_str(&s).map_err(io_err)
}

// ---------------------------------------------------------------------------
// Checkpoints
// ---------------------------------------------------------------------------

pub fn save_checkpoint(id: &GoalId, c: &Checkpoint) -> std::io::Result<()> {
    ensure_goal_dirs(id)?;
    let bytes = serde_json::to_vec_pretty(c).map_err(io_err)?;
    let ts = c.at.format("%Y-%m-%dT%H-%M-%S%.3fZ").to_string();
    let file = checkpoints_dir(id).join(format!("{ts}.json"));
    write_atomic(&file, &bytes)?;
    let latest = checkpoints_dir(id).join("latest.json");
    write_atomic(&latest, &bytes)?;
    rotate_checkpoints(id)?;
    Ok(())
}

pub fn load_latest_checkpoint(id: &GoalId) -> std::io::Result<Option<Checkpoint>> {
    let p = checkpoints_dir(id).join("latest.json");
    if !p.exists() {
        return Ok(None);
    }
    let s = read_string(&p)?;
    Ok(serde_json::from_str(&s).ok())
}

fn rotate_checkpoints(id: &GoalId) -> std::io::Result<()> {
    let dir = checkpoints_dir(id);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("latest.json"))
        .collect();
    files.sort();
    while files.len() > MAX_CHECKPOINT_FILES {
        if let Some(oldest) = files.first().cloned() {
            let _ = fs::remove_file(&oldest);
            files.remove(0);
        } else {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Progress log
// ---------------------------------------------------------------------------

pub fn progress_log_path(id: &GoalId) -> PathBuf {
    goal_dir(id).join("progress.log")
}

pub fn append_progress(id: &GoalId, line: &str) -> std::io::Result<()> {
    ensure_goal_dirs(id)?;
    let stamp: DateTime<Utc> = Utc::now();
    let entry = format!("{}  {}\n", stamp.format("%Y-%m-%dT%H:%M:%S%.3fZ"), line);
    let p = progress_log_path(id);
    let mut f = fs::OpenOptions::new().create(true).append(true).open(p)?;
    f.write_all(entry.as_bytes())?;
    Ok(())
}

pub fn tail_progress(id: &GoalId, n: usize) -> std::io::Result<Vec<String>> {
    let p = progress_log_path(id);
    if !p.exists() {
        return Ok(Vec::new());
    }
    let s = read_string(&p)?;
    let lines: Vec<&str> = s.lines().collect();
    let take = n.min(lines.len()).min(MAX_PROGRESS_LINES_RETURN);
    let start = lines.len().saturating_sub(take);
    Ok(lines[start..].iter().map(|s| s.to_string()).collect())
}

// ---------------------------------------------------------------------------
// Transcript (append-only JSONL)
// ---------------------------------------------------------------------------

pub fn transcript_path(id: &GoalId) -> PathBuf {
    goal_dir(id).join("transcript.jsonl")
}

pub fn append_transcript_line(id: &GoalId, json_line: &str) -> std::io::Result<()> {
    ensure_goal_dirs(id)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(transcript_path(id))?;
    f.write_all(json_line.as_bytes())?;
    if !json_line.ends_with('\n') {
        f.write_all(b"\n")?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Archive
// ---------------------------------------------------------------------------

pub fn archive_goal(id: &GoalId) -> std::io::Result<()> {
    let src = goal_dir(id);
    if !src.exists() {
        return Ok(());
    }
    let dest = archive_root().join(id.as_str());
    fs::create_dir_all(archive_root())?;
    if dest.exists() {
        // Suffix with timestamp if there's a collision (extremely unlikely).
        let alt = archive_root().join(format!(
            "{}-{}",
            id.as_str(),
            Utc::now().format("%Y%m%d%H%M%S")
        ));
        fs::rename(&src, &alt)?;
    } else {
        fs::rename(&src, &dest)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// State helpers
// ---------------------------------------------------------------------------

/// Update the in-memory + on-disk index entry for `id`.
pub fn upsert_index(
    id: &GoalId,
    state: GoalState,
    objective: &str,
    created_at: DateTime<Utc>,
    metrics: &GoalMetrics,
) -> std::io::Result<()> {
    let mut idx = IndexFile::load_or_default();
    idx.upsert(GoalSummary {
        id: id.clone(),
        state,
        objective: objective.to_string(),
        created_at,
        turns: metrics.turns,
        cost_usd: metrics.cost_usd,
    });
    idx.save()
}

pub fn remove_from_index(id: &GoalId) -> std::io::Result<()> {
    let mut idx = IndexFile::load_or_default();
    idx.remove(id);
    idx.save()
}
