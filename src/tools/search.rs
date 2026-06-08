use async_trait::async_trait;
use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::Tool;
use crate::types::*;

const DEFAULT_MAX_RESULTS: usize = 100;
const DEFAULT_MAX_FILES_SCANNED: usize = 20_000;
const DEFAULT_MAX_FILE_BYTES: u64 = 1_000_000;

fn resolve_path(path_str: Option<&str>, ctx: &ToolContext) -> PathBuf {
    path_str
        .map(|p| {
            let path = Path::new(p);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                ctx.working_dir.join(path)
            }
        })
        .unwrap_or_else(|| ctx.working_dir.clone())
}

fn rel_to(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .or_else(|_| path.strip_prefix(std::env::current_dir().unwrap_or_default()))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn matches_glob(pattern: &glob::Pattern, path: &Path, root: &Path) -> bool {
    let rel = rel_to(path, root);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    pattern.matches(&rel) || pattern.matches(&name)
}

fn is_probably_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

fn matching_line_indices(
    content: &str,
    lines: &[&str],
    regex: &regex::Regex,
    multiline: bool,
) -> Vec<usize> {
    if lines.is_empty() {
        return Vec::new();
    }

    if !multiline {
        return lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| regex.is_match(line).then_some(i))
            .collect();
    }

    let mut line_starts = Vec::with_capacity(lines.len());
    let mut pos = 0usize;
    for line in lines {
        line_starts.push(pos);
        pos += line.len() + 1;
    }

    let mut indices = BTreeSet::new();
    for mat in regex.find_iter(content) {
        let start = mat.start();
        let end = mat.end().max(start + 1);
        let start_line = line_starts.partition_point(|offset| *offset <= start);
        let end_line = line_starts.partition_point(|offset| *offset < end);
        let first = start_line.saturating_sub(1);
        let last = end_line
            .saturating_sub(1)
            .min(lines.len().saturating_sub(1));
        for line in first..=last {
            indices.insert(line);
        }
    }
    indices.into_iter().collect()
}

// ───────────────────────────────────────────────────────────────────────────
// Reusable search core (shared by search_files, the `locate` tool, and the
// benchmark localization harness).
//
// Design notes addressing the discovery audit:
//   * Non-blocking: the heavy walk+read is intended to be driven from a
//     `tokio::task::spawn_blocking` so it never stalls the async runtime.
//   * Parallel: candidate file *reads + regex matching* run on the rayon thread
//     pool (the expensive part), instead of the old single-threaded scan.
//   * Smart-case: callers derive case sensitivity from the pattern unless the
//     user is explicit.
//   * Ranked: results are scored by match density, definition-likeness, file
//     name match, and path proximity, then returned most-relevant first.
// ───────────────────────────────────────────────────────────────────────────

/// Fully-resolved parameters for one search. All fields are owned so the whole
/// struct is `Send + 'static` and can be moved into `spawn_blocking`.
pub struct SearchParams {
    pub regex: regex::Regex,
    /// Human-readable pattern for messages.
    pub pattern_display: String,
    /// Bare identifier-ish term extracted from the pattern, used for filename
    /// matching and definition detection during ranking. May be empty.
    pub term: String,
    pub multiline: bool,
    pub search_path: PathBuf,
    pub working_dir: PathBuf,
    pub glob_pattern: Option<glob::Pattern>,
    pub exclude_glob: Option<glob::Pattern>,
    pub type_extensions: Vec<&'static str>,
    pub include_hidden: bool,
    pub include_ignored: bool,
    pub max_results: usize,
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub before_ctx: usize,
    pub after_ctx: usize,
}

pub struct MatchedLine {
    pub line_no: usize,
    pub content: String,
    pub is_match: bool,
}

/// One file that contained at least one match, with a relevance score.
pub struct FileHit {
    pub rel_path: String,
    pub matches: Vec<MatchedLine>,
    pub match_count: usize,
    pub score: f64,
}

/// Aggregate outcome of a ranked search.
pub struct SearchOutcome {
    /// Matching files, ranked most-relevant first.
    pub files: Vec<FileHit>,
    pub total_matches: usize,
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub hit_result_cap: bool,
    pub hit_file_cap: bool,
}

enum FileScan {
    Hit(FileHit),
    NoMatch,
    Skipped,
}

/// Keywords that strongly indicate a line *defines* (rather than merely uses) a
/// symbol — used to boost the file that actually declares what you searched for.
const DEFINITION_KEYWORDS: &[&str] = &[
    "fn ", "struct ", "enum ", "trait ", "impl ", "class ", "def ", "func ",
    "function ", "interface ", "type ", "const ", "static ", "let ", "var ",
    "module ", "package ", "public ", "private ", "protected ", "abstract ",
    "export ", "pub ",
];

fn line_is_definition(line: &str, term: &str) -> bool {
    let l = line.trim_start();
    let has_kw = DEFINITION_KEYWORDS.iter().any(|kw| l.contains(*kw));
    if !has_kw {
        return false;
    }
    term.is_empty() || l.contains(term)
}

fn is_noisy_path(rel_lower: &str) -> bool {
    rel_lower.contains("/test")
        || rel_lower.starts_with("test")
        || rel_lower.contains("__tests__")
        || rel_lower.contains("/vendor/")
        || rel_lower.contains("/node_modules/")
        || rel_lower.contains("/target/")
        || rel_lower.contains("/dist/")
        || rel_lower.contains("/build/")
        || rel_lower.contains("/.git/")
        || rel_lower.ends_with(".min.js")
        || rel_lower.ends_with(".lock")
}

fn score_file(rel: &str, match_count: usize, has_def: bool, term: &str) -> f64 {
    // Diminishing returns on raw match count so a noisy file does not bury the
    // single file that actually defines the symbol.
    let mut s = (match_count as f64).ln_1p();

    if has_def {
        s += 4.0;
    }

    let lower = rel.to_lowercase();
    if !term.is_empty() {
        let term_l = term.to_lowercase();
        let stem = Path::new(rel)
            .file_stem()
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_lowercase();
        if stem == term_l {
            s += 3.0;
        } else if stem.contains(&term_l) {
            s += 1.5;
        }
    }

    // Shallower paths (closer to the project root) are usually more central.
    let depth = rel.matches('/').count();
    s += 1.0 / (1.0 + depth as f64);

    if is_noisy_path(&lower) {
        s -= 1.5;
    }

    s
}

fn scan_one_file(path: &Path, p: &SearchParams) -> FileScan {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return FileScan::Skipped,
    };
    if is_probably_binary(&bytes) {
        return FileScan::Skipped;
    }
    let content = match String::from_utf8(bytes) {
        Ok(c) => c,
        Err(_) => return FileScan::Skipped,
    };

    let lines: Vec<&str> = content.lines().collect();
    let match_indices = matching_line_indices(&content, &lines, &p.regex, p.multiline);
    if match_indices.is_empty() {
        return FileScan::NoMatch;
    }

    // Expand with context lines, de-duplicating overlapping windows.
    let mut matched: Vec<MatchedLine> = Vec::new();
    let mut printed_lines = BTreeSet::new();
    for &mi in &match_indices {
        let start = mi.saturating_sub(p.before_ctx);
        let end = (mi + p.after_ctx + 1).min(lines.len());
        for li in start..end {
            if printed_lines.insert(li) {
                matched.push(MatchedLine {
                    line_no: li + 1,
                    content: lines[li].to_string(),
                    is_match: li == mi,
                });
            }
        }
    }
    matched.sort_by_key(|m| m.line_no);

    let has_def = match_indices
        .iter()
        .any(|&mi| line_is_definition(lines[mi], &p.term));
    let rel = rel_to(path, &p.working_dir);
    let match_count = match_indices.len();
    let score = score_file(&rel, match_count, has_def, &p.term);

    FileScan::Hit(FileHit {
        rel_path: rel,
        matches: matched,
        match_count,
        score,
    })
}

/// Run a ranked, parallel search. Synchronous and CPU/IO-bound — call it from
/// inside `tokio::task::spawn_blocking` when invoked from async code.
pub fn run_search(p: &SearchParams) -> SearchOutcome {
    // Phase 1 — collect candidate paths using cheap filters only (no reads).
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut files_skipped = 0usize;
    let mut hit_file_cap = false;

    let walker = WalkBuilder::new(&p.search_path)
        .hidden(!p.include_hidden)
        .ignore(!p.include_ignored)
        .git_ignore(!p.include_ignored)
        .git_global(!p.include_ignored)
        .git_exclude(!p.include_ignored)
        .build();

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        if candidates.len() >= p.max_files {
            hit_file_cap = true;
            break;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !p.type_extensions.is_empty() && !p.type_extensions.iter().any(|e| *e == ext) {
            continue;
        }
        if let Some(ref gp) = p.glob_pattern {
            if !matches_glob(gp, path, &p.search_path) {
                continue;
            }
        }
        if let Some(ref gp) = p.exclude_glob {
            if matches_glob(gp, path, &p.search_path) {
                continue;
            }
        }
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > p.max_file_bytes {
                files_skipped += 1;
                continue;
            }
        }
        candidates.push(path.to_path_buf());
    }

    let files_scanned = candidates.len();

    // Phase 2 — read + match in parallel (the expensive part).
    let scans: Vec<FileScan> = candidates
        .par_iter()
        .map(|path| scan_one_file(path, p))
        .collect();

    let mut files: Vec<FileHit> = Vec::new();
    for s in scans {
        match s {
            FileScan::Hit(h) => files.push(h),
            FileScan::Skipped => files_skipped += 1,
            FileScan::NoMatch => {}
        }
    }

    // Rank: score desc, then path asc for stable, deterministic ordering.
    files.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });

    // Enforce the matching-line cap across the ranked list.
    let mut total_matches = 0usize;
    let mut kept: Vec<FileHit> = Vec::new();
    let mut hit_result_cap = false;
    for f in files {
        if total_matches >= p.max_results {
            hit_result_cap = true;
            break;
        }
        total_matches += f.match_count;
        kept.push(f);
    }
    if total_matches >= p.max_results {
        hit_result_cap = true;
    }

    SearchOutcome {
        files: kept,
        total_matches,
        files_scanned,
        files_skipped,
        hit_result_cap,
        hit_file_cap,
    }
}

/// Extract a bare identifier term from a pattern for ranking heuristics.
/// Returns the longest run of identifier characters (letters, digits, `_`).
pub fn extract_term(pattern: &str) -> String {
    let mut best = String::new();
    let mut cur = String::new();
    for ch in pattern.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if cur.len() > best.len() {
                best = std::mem::take(&mut cur);
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() > best.len() {
        best = cur;
    }
    best
}

/// Smart-case: case-sensitive when the pattern contains an uppercase letter,
/// case-insensitive otherwise (matching ripgrep's `--smart-case`).
pub fn smart_case_sensitive(pattern: &str) -> bool {
    pattern.chars().any(|c| c.is_uppercase())
}

// ─── search_files (enhanced grep) ────────────────────────────────────────────

pub struct SearchFilesTool;

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "Search for text patterns in files using regex or fixed strings. Returns matching lines with \
        file paths and line numbers, RANKED so the most relevant files (definitions, filename \
        matches, shallow paths) come first. Runs in parallel and respects .gitignore. \
        Case sensitivity is smart by default: case-insensitive unless the pattern contains an \
        uppercase letter (override with case_sensitive). Supports context lines, output modes, \
        file type filtering, globs, and multiline matching. The regex engine is linear-time and \
        does NOT support look-around or backreferences; such patterns are auto-retried as a \
        literal string."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Search pattern (regex by default, or literal string if fixed_string=true)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (default: working directory)"
                },
                "file_pattern": {
                    "type": "string",
                    "description": "Only search files matching this glob. Matches file names and relative paths (e.g. '*.rs', 'src/**/*.rs')"
                },
                "exclude_pattern": {
                    "type": "string",
                    "description": "Skip files matching this glob. Matches file names and relative paths"
                },
                "type_filter": {
                    "type": "string",
                    "description": "File type shorthand: 'rs', 'py', 'js', 'ts', 'go', 'java', 'c', 'cpp', 'md', 'json', 'toml', 'yaml'"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Force case sensitivity. Omit for smart-case (insensitive unless the pattern has an uppercase letter)."
                },
                "fixed_string": {
                    "type": "boolean",
                    "description": "Treat pattern as a literal fixed string, not regex (default: false)"
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline matching — dot matches newlines (default: false)"
                },
                "include_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files/directories (default: false)"
                },
                "include_ignored": {
                    "type": "boolean",
                    "description": "Include files ignored by .gitignore/.ignore (default: false)"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "Output mode: 'content' shows matching lines (default), 'files_with_matches' shows only file paths (ranked), 'count' shows match counts per file"
                },
                "before_context": {
                    "type": "integer",
                    "description": "Number of lines to show BEFORE each match (like grep -B)"
                },
                "after_context": {
                    "type": "integer",
                    "description": "Number of lines to show AFTER each match (like grep -A)"
                },
                "context": {
                    "type": "integer",
                    "description": "Number of lines to show before AND after each match (like grep -C, overrides before/after_context)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matching lines to return (default: 100)"
                },
                "max_files": {
                    "type": "integer",
                    "description": "Maximum number of files to scan before stopping (default: 20000)"
                },
                "max_file_bytes": {
                    "type": "integer",
                    "description": "Skip files larger than this many bytes (default: 1000000)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let pattern_str = match input["pattern"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'pattern' parameter"),
        };

        let search_path = resolve_path(input["path"].as_str(), ctx);
        if !search_path.exists() {
            return ToolOutput::error(format!("Search path not found: {}", search_path.display()));
        }

        // Smart-case unless the caller is explicit.
        let case_sensitive = input["case_sensitive"]
            .as_bool()
            .unwrap_or_else(|| smart_case_sensitive(pattern_str));
        let fixed_string = input["fixed_string"].as_bool().unwrap_or(false);
        let multiline = input["multiline"].as_bool().unwrap_or(false);
        let max_results = input["max_results"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_RESULTS as u64) as usize;
        let max_files = input["max_files"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_FILES_SCANNED as u64) as usize;
        let max_file_bytes = input["max_file_bytes"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_FILE_BYTES);
        let include_hidden = input["include_hidden"].as_bool().unwrap_or(false);
        let include_ignored = input["include_ignored"].as_bool().unwrap_or(false);
        let file_pattern = input["file_pattern"].as_str();
        let exclude_pattern = input["exclude_pattern"].as_str();
        let type_filter = input["type_filter"].as_str();

        let output_mode = input["output_mode"].as_str().unwrap_or("content").to_string();

        // Context lines
        let ctx_both = input["context"].as_u64().map(|n| n as usize);
        let before_ctx =
            ctx_both.unwrap_or_else(|| input["before_context"].as_u64().unwrap_or(0) as usize);
        let after_ctx =
            ctx_both.unwrap_or_else(|| input["after_context"].as_u64().unwrap_or(0) as usize);

        // Build regex (or literal matcher). If a regex is requested but fails to
        // compile (e.g. uses look-around the linear-time engine rejects), fall
        // back to a literal search rather than erroring out.
        let mut fell_back_to_literal = false;
        let regex_result = if fixed_string {
            build_regex(&regex::escape(pattern_str), case_sensitive, multiline)
        } else {
            match try_build_regex(pattern_str, case_sensitive, multiline) {
                Ok(r) => Ok(r),
                Err(_) => {
                    fell_back_to_literal = true;
                    build_regex(&regex::escape(pattern_str), case_sensitive, multiline)
                }
            }
        };
        let regex = match regex_result {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("Invalid pattern: {e}")),
        };

        let type_extensions = type_filter.map(type_to_extensions).unwrap_or_default();

        let glob_pattern = match file_pattern.map(glob::Pattern::new).transpose() {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid file_pattern glob: {e}")),
        };
        let exclude_glob = match exclude_pattern.map(glob::Pattern::new).transpose() {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid exclude_pattern glob: {e}")),
        };

        let params = SearchParams {
            regex,
            pattern_display: pattern_str.to_string(),
            term: extract_term(pattern_str),
            multiline,
            search_path,
            working_dir: ctx.working_dir.clone(),
            glob_pattern,
            exclude_glob,
            type_extensions,
            include_hidden,
            include_ignored,
            max_results,
            max_files,
            max_file_bytes,
            before_ctx,
            after_ctx,
        };

        // Run the heavy walk+read off the async runtime.
        let outcome = match tokio::task::spawn_blocking(move || run_search(&params)).await {
            Ok(o) => o,
            Err(e) => return ToolOutput::error(format!("Search task failed: {e}")),
        };

        let pattern_disp = pattern_str.to_string();

        if outcome.files.is_empty() {
            let note = if outcome.files_skipped > 0 {
                format!(
                    " ({} file(s) scanned, {} skipped)",
                    outcome.files_scanned, outcome.files_skipped
                )
            } else {
                format!(" ({} file(s) scanned)", outcome.files_scanned)
            };
            return ToolOutput::success(format!("No matches found for: {pattern_disp}{note}"));
        }

        let fallback_note = if fell_back_to_literal {
            " (pattern was not valid regex — searched as a literal string)"
        } else {
            ""
        };

        match output_mode.as_str() {
            "files_with_matches" => {
                let files: Vec<String> =
                    outcome.files.iter().map(|f| f.rel_path.clone()).collect();
                ToolOutput::success(format!(
                    "Files matching '{}' ({} file(s), ranked by relevance){}:\n\n{}",
                    pattern_disp,
                    files.len(),
                    fallback_note,
                    files.join("\n")
                ))
            }
            "count" => {
                let lines: Vec<String> = outcome
                    .files
                    .iter()
                    .map(|f| format!("{}: {}", f.rel_path, f.match_count))
                    .collect();
                ToolOutput::success(format!(
                    "Match counts for '{}' (ranked){}:\n\n{}",
                    pattern_disp,
                    fallback_note,
                    lines.join("\n")
                ))
            }
            _ => {
                let mut output_lines: Vec<String> = Vec::new();
                for f in &outcome.files {
                    if !output_lines.is_empty() {
                        output_lines.push("--".to_string());
                    }
                    let mut prev_line_no = 0usize;
                    for m in &f.matches {
                        if prev_line_no > 0 && m.line_no > prev_line_no + 1 {
                            output_lines.push("  ...".to_string());
                        }
                        let prefix = if m.is_match { ">" } else { " " };
                        output_lines.push(format!(
                            "{prefix} {}:{:>4} | {}",
                            f.rel_path, m.line_no, m.content
                        ));
                        prev_line_no = m.line_no;
                    }
                }

                let mut notes = Vec::new();
                if !fallback_note.is_empty() {
                    notes.push(fallback_note.trim().trim_start_matches('(').trim_end_matches(')').to_string());
                }
                if outcome.hit_result_cap {
                    notes.push(format!(
                        "Results truncated at {max_results} matching line(s). Use max_results to increase."
                    ));
                }
                if outcome.hit_file_cap {
                    notes.push(format!(
                        "Stopped after scanning {max_files} file(s). Use max_files to increase."
                    ));
                }
                if outcome.files_skipped > 0 {
                    notes.push(format!(
                        "{} file(s) skipped as binary, unreadable, or larger than max_file_bytes.",
                        outcome.files_skipped
                    ));
                }
                let truncation_note = if notes.is_empty() {
                    String::new()
                } else {
                    format!("\n\n({})", notes.join(" "))
                };

                ToolOutput::success(format!(
                    "Found {} match(es) in {} file(s), ranked by relevance:\n\n{}{}",
                    outcome.total_matches,
                    outcome.files.len(),
                    output_lines.join("\n"),
                    truncation_note
                ))
            }
        }
    }
}

fn try_build_regex(
    pattern: &str,
    case_sensitive: bool,
    multiline: bool,
) -> Result<regex::Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .multi_line(multiline)
        .dot_matches_new_line(multiline)
        .build()
}

fn build_regex(
    pattern: &str,
    case_sensitive: bool,
    multiline: bool,
) -> Result<regex::Regex, regex::Error> {
    try_build_regex(pattern, case_sensitive, multiline)
}

/// Map type shorthand to file extensions
pub fn type_to_extensions(type_name: &str) -> Vec<&'static str> {
    match type_name {
        "rs" | "rust" => vec!["rs"],
        "py" | "python" => vec!["py", "pyi"],
        "js" | "javascript" => vec!["js", "mjs", "cjs"],
        "ts" | "typescript" => vec!["ts", "tsx"],
        "go" => vec!["go"],
        "java" => vec!["java"],
        "c" => vec!["c", "h"],
        "cpp" | "cxx" | "cc" => vec!["cpp", "cxx", "cc", "hpp", "hxx"],
        "cs" | "csharp" => vec!["cs"],
        "rb" | "ruby" => vec!["rb"],
        "php" => vec!["php"],
        "swift" => vec!["swift"],
        "kt" | "kotlin" => vec!["kt"],
        "md" | "markdown" => vec!["md", "mdx"],
        "json" => vec!["json"],
        "toml" => vec!["toml"],
        "yaml" | "yml" => vec!["yaml", "yml"],
        "html" => vec!["html", "htm"],
        "css" => vec!["css"],
        "sh" | "bash" | "shell" => vec!["sh", "bash"],
        "sql" => vec!["sql"],
        "xml" => vec!["xml"],
        "txt" => vec!["txt"],
        _ => vec![],
    }
}

// ─── find_files ───────────────────────────────────────────────────────────────

pub struct FindFilesTool;

#[async_trait]
impl Tool for FindFilesTool {
    fn name(&self) -> &str {
        "find_files"
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "Find files by name or glob pattern. Matches file names and relative paths, respects \
         .gitignore by default, and returns project-relative paths RANKED so exact/closest name \
         matches in shallow paths come first."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "File name or relative path glob (e.g. '*.rs', 'test_*.py', 'src/**/*.json')"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: working directory)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 100)"
                },
                "include_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files/directories (default: false)"
                },
                "include_ignored": {
                    "type": "boolean",
                    "description": "Include files ignored by .gitignore/.ignore (default: false)"
                },
                "include_dirs": {
                    "type": "boolean",
                    "description": "Include matching directories as well as files (default: false)"
                },
                "type_filter": {
                    "type": "string",
                    "description": "File type shorthand: 'rs', 'py', 'js', 'ts', 'go', 'md', 'json', etc."
                },
                "exclude_pattern": {
                    "type": "string",
                    "description": "Skip paths matching this glob"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum traversal depth from path"
                }
            },
            "required": ["pattern"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let pattern_str = match input["pattern"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'pattern' parameter"),
        };

        let search_path = resolve_path(input["path"].as_str(), ctx);
        if !search_path.exists() {
            return ToolOutput::error(format!("Search path not found: {}", search_path.display()));
        }

        let max_results = input["max_results"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_RESULTS as u64) as usize;
        let include_hidden = input["include_hidden"].as_bool().unwrap_or(false);
        let include_ignored = input["include_ignored"].as_bool().unwrap_or(false);
        let include_dirs = input["include_dirs"].as_bool().unwrap_or(false);
        let max_depth = input["max_depth"].as_u64().map(|n| n as usize);
        let type_extensions = input["type_filter"]
            .as_str()
            .map(type_to_extensions)
            .unwrap_or_default();

        let glob_pattern = match glob::Pattern::new(pattern_str) {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid glob pattern: {e}")),
        };
        let exclude_glob = match input["exclude_pattern"]
            .as_str()
            .map(glob::Pattern::new)
            .transpose()
        {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid exclude_pattern glob: {e}")),
        };

        let term = extract_term(pattern_str);
        let working_dir = ctx.working_dir.clone();
        let search_path_cl = search_path.clone();

        let mut results: Vec<String> = match tokio::task::spawn_blocking(move || {
            let mut builder = WalkBuilder::new(&search_path_cl);
            builder
                .hidden(!include_hidden)
                .ignore(!include_ignored)
                .git_ignore(!include_ignored)
                .git_global(!include_ignored)
                .git_exclude(!include_ignored);
            if let Some(depth) = max_depth {
                builder.max_depth(Some(depth));
            }
            let walker = builder.build();

            let mut out: Vec<String> = Vec::new();
            for entry in walker.into_iter().filter_map(|e| e.ok()) {
                if out.len() >= max_results.saturating_mul(4).max(max_results) {
                    break;
                }
                let path = entry.path();
                if path == search_path_cl {
                    continue;
                }
                if path.is_dir() && !include_dirs {
                    continue;
                }
                if path.is_file() && !type_extensions.is_empty() {
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if !type_extensions.iter().any(|e| *e == ext) {
                        continue;
                    }
                }
                if let Some(ref gp) = exclude_glob {
                    if matches_glob(gp, path, &search_path_cl) {
                        continue;
                    }
                }
                if matches_glob(&glob_pattern, path, &search_path_cl) {
                    out.push(rel_to(path, &working_dir));
                }
            }
            out
        })
        .await
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("find_files task failed: {e}")),
        };

        // Rank: exact stem match → stem contains term → shallow paths → alpha.
        let term_l = term.to_lowercase();
        results.sort_by(|a, b| {
            file_rank_key(a, &term_l)
                .partial_cmp(&file_rank_key(b, &term_l))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });
        results.truncate(max_results);

        if results.is_empty() {
            ToolOutput::success(format!("No files found matching: {pattern_str}"))
        } else {
            let truncated = if results.len() >= max_results {
                format!(
                    "\n\n(Showing top {max_results} path(s), ranked by relevance. Use max_results to increase.)"
                )
            } else {
                String::new()
            };
            ToolOutput::success(format!(
                "Found {} path(s), ranked by relevance:\n\n{}{}",
                results.len(),
                results.join("\n"),
                truncated
            ))
        }
    }
}

/// Lower key sorts first. Encodes: exact-stem (0), stem-contains (1), other (2),
/// then path depth, so the most likely target floats to the top.
fn file_rank_key(rel: &str, term_l: &str) -> f64 {
    let stem = Path::new(rel)
        .file_stem()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_lowercase();
    let lower = rel.to_lowercase();
    let base = if term_l.is_empty() {
        2.0
    } else if stem == *term_l {
        0.0
    } else if stem.contains(term_l) {
        1.0
    } else {
        2.0
    };
    let depth = rel.matches('/').count() as f64;
    let noise = if is_noisy_path(&lower) { 5.0 } else { 0.0 };
    base * 100.0 + depth + noise
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext {
            working_dir: dir.to_path_buf(),
            home_dir: dir.to_path_buf(),
            session_id: "test".to_string(),
            trust_mode: true,
            permission_mode: crate::types::PermissionMode::Default,
            diff_review: true,
            file_cache: None,
            active_skill_scope: None,
            skill_registry: None,
            output_chunk_tx: None,
            tool_call_id: None,
            team_blackboard: None,
        }
    }

    #[tokio::test]
    async fn test_search_files_basic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("test.rs"),
            "fn hello_world() {}\nfn goodbye() {}",
        )
        .unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({"pattern": "hello", "path": dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("hello_world"));
    }

    #[tokio::test]
    async fn test_search_files_context() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("test.rs"),
            "line1\nline2\nline3\nline4\nline5",
        )
        .unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({
                    "pattern": "line3",
                    "path": dir.path().to_str().unwrap(),
                    "context": 1
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("line2"));
        assert!(output.content.contains("line3"));
        assert!(output.content.contains("line4"));
    }

    #[tokio::test]
    async fn test_search_files_modes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "hello\nhello again").unwrap();
        std::fs::write(dir.path().join("b.rs"), "no match here").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());

        let output = tool
            .execute(
                json!({
                    "pattern": "hello",
                    "path": dir.path().to_str().unwrap(),
                    "output_mode": "files_with_matches"
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("a.rs"));
        assert!(!output.content.contains("b.rs"));

        let output = tool
            .execute(
                json!({
                    "pattern": "hello",
                    "path": dir.path().to_str().unwrap(),
                    "output_mode": "count"
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("2"));
    }

    #[tokio::test]
    async fn test_search_files_fixed_string() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "foo.bar\nfoobar\nfoo+bar").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({
                    "pattern": "foo.bar",
                    "path": dir.path().to_str().unwrap(),
                    "fixed_string": true
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("foo.bar"));
    }

    #[tokio::test]
    async fn test_search_files_path_glob_and_exclude() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/bin")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "needle\n").unwrap();
        std::fs::write(dir.path().join("src/bin/main.rs"), "needle\n").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({
                    "pattern": "needle",
                    "path": dir.path().to_str().unwrap(),
                    "file_pattern": "src/**/*.rs",
                    "exclude_pattern": "src/bin/**"
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("src/lib.rs"));
        assert!(!output.content.contains("src/bin/main.rs"));
    }

    #[tokio::test]
    async fn test_search_files_multiline_maps_to_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "alpha\nbeta\ngamma\n").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({
                    "pattern": "alpha.*gamma",
                    "path": dir.path().to_str().unwrap(),
                    "multiline": true
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("alpha"));
        assert!(output.content.contains("gamma"));
    }

    #[tokio::test]
    async fn test_search_ranks_definition_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        // A file that merely uses the symbol many times, deep in the tree.
        std::fs::write(
            dir.path().join("src/uses.rs"),
            "widget();\nwidget();\nwidget();\nwidget();\n",
        )
        .unwrap();
        // The actual definition, shallow.
        std::fs::write(dir.path().join("widget.rs"), "fn widget() {}\n").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({
                    "pattern": "widget",
                    "path": dir.path().to_str().unwrap(),
                    "output_mode": "files_with_matches"
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        let body = output.content;
        let def_pos = body.find("widget.rs").unwrap();
        let use_pos = body.find("uses.rs").unwrap();
        assert!(def_pos < use_pos, "definition file should rank first:\n{body}");
    }

    #[tokio::test]
    async fn test_smart_case() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("t.rs"), "Foo\nfoo\nFOO\n").unwrap();
        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        // Uppercase in pattern → case-sensitive → only "Foo".
        let out = tool
            .execute(
                json!({"pattern": "Foo", "path": dir.path().to_str().unwrap(), "output_mode": "count"}),
                &ctx,
            )
            .await;
        assert!(out.content.contains("t.rs: 1"), "got: {}", out.content);
    }

    #[tokio::test]
    async fn test_regex_fallback_to_literal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("t.txt"), "a(?=b) literal here\n").unwrap();
        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        // Look-ahead is unsupported by the linear engine; should fall back.
        let out = tool
            .execute(
                json!({"pattern": "a(?=b)", "path": dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await;
        assert!(!out.is_error);
        assert!(out.content.contains("literal here") || out.content.contains("No matches"));
    }

    #[tokio::test]
    async fn test_find_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("foo.rs"), "").unwrap();
        std::fs::write(dir.path().join("bar.txt"), "").unwrap();

        let tool = FindFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({"pattern": "*.rs", "path": dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("foo.rs"));
        assert!(!output.content.contains("bar.txt"));
    }

    #[tokio::test]
    async fn test_find_files_matches_relative_path_glob() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/bin")).unwrap();
        std::fs::write(dir.path().join("src/bin/main.rs"), "").unwrap();
        std::fs::write(dir.path().join("main.rs"), "").unwrap();

        let tool = FindFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({"pattern": "src/**/*.rs", "path": dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("src/bin/main.rs"));
        assert!(!output.content.contains("\nmain.rs"));
    }

    #[test]
    fn test_type_to_extensions() {
        assert_eq!(type_to_extensions("rs"), vec!["rs"]);
        assert_eq!(type_to_extensions("py"), vec!["py", "pyi"]);
        assert!(type_to_extensions("unknown").is_empty());
    }

    #[test]
    fn test_extract_term() {
        assert_eq!(extract_term("MyStruct"), "MyStruct");
        assert_eq!(extract_term("foo.*bar"), "bar");
        assert_eq!(extract_term(r"\bAgentLoop\b"), "AgentLoop");
    }

    #[test]
    fn test_smart_case_helper() {
        assert!(smart_case_sensitive("Foo"));
        assert!(!smart_case_sensitive("foo"));
    }
}
