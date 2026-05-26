use async_trait::async_trait;
use ignore::WalkBuilder;
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

// ─── search_files (enhanced grep) ────────────────────────────────────────────
//
// Now supports:
//   - Context lines: before_context, after_context (like grep -B/-A/-C)
//   - Output modes: "content" (default), "files_with_matches", "count"
//   - File type filter: type_filter ("rs", "py", "js", "ts", "go", etc.)
//   - Head limit: first N results
//   - Fixed string mode: fixed_string (no regex, literal match)
//   - Multiline mode: multiline
//   - Case sensitive/insensitive

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
        file paths and line numbers. Supports context lines, output modes, file type filtering, \
        and more. Respects .gitignore."
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
                    "description": "Case-sensitive search (default: false = case-insensitive)"
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
                    "description": "Output mode: 'content' shows matching lines (default), 'files_with_matches' shows only file paths, 'count' shows match counts per file"
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

        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(false);
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

        // Output mode
        let output_mode = input["output_mode"].as_str().unwrap_or("content");

        // Context lines
        let ctx_both = input["context"].as_u64().map(|n| n as usize);
        let before_ctx =
            ctx_both.unwrap_or_else(|| input["before_context"].as_u64().unwrap_or(0) as usize);
        let after_ctx =
            ctx_both.unwrap_or_else(|| input["after_context"].as_u64().unwrap_or(0) as usize);

        // Build regex (or literal matcher)
        let effective_pattern = if fixed_string {
            regex::escape(pattern_str)
        } else {
            pattern_str.to_string()
        };

        let regex = match RegexBuilder::new(&effective_pattern)
            .case_insensitive(!case_sensitive)
            .multi_line(multiline)
            .dot_matches_new_line(multiline)
            .build()
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("Invalid pattern: {e}")),
        };

        // File extension filter from type shorthand
        let type_extensions = type_filter.map(type_to_extensions).unwrap_or_default();

        // Glob pattern
        let glob_pattern = match file_pattern.map(glob::Pattern::new).transpose() {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid file_pattern glob: {e}")),
        };
        let exclude_glob = match exclude_pattern.map(glob::Pattern::new).transpose() {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid exclude_pattern glob: {e}")),
        };

        // ---- Walk and search ----
        let mut file_matches: Vec<(String, Vec<MatchedLine>)> = Vec::new();
        let mut total_matches = 0usize;
        let mut files_scanned = 0usize;
        let mut files_skipped = 0usize;

        let walker = WalkBuilder::new(&search_path)
            .hidden(!include_hidden)
            .ignore(!include_ignored)
            .git_ignore(!include_ignored)
            .git_global(!include_ignored)
            .git_exclude(!include_ignored)
            .build();

        'walk: for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if total_matches >= max_results {
                break;
            }
            if files_scanned >= max_files {
                break;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            files_scanned += 1;

            // Apply file type filter
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            if !type_extensions.is_empty() && !type_extensions.iter().any(|e| *e == ext) {
                continue;
            }
            if let Some(ref gp) = glob_pattern {
                if !matches_glob(gp, path, &search_path) {
                    continue;
                }
            }
            if let Some(ref gp) = exclude_glob {
                if matches_glob(gp, path, &search_path) {
                    continue;
                }
            }
            if let Ok(meta) = std::fs::metadata(path) {
                if meta.len() > max_file_bytes {
                    files_skipped += 1;
                    continue;
                }
            }

            // Read file
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(_) => {
                    files_skipped += 1;
                    continue;
                }
            };
            if is_probably_binary(&bytes) {
                files_skipped += 1;
                continue;
            }
            let content = match String::from_utf8(bytes) {
                Ok(c) => c,
                Err(_) => {
                    files_skipped += 1;
                    continue;
                }
            };

            let lines: Vec<&str> = content.lines().collect();
            let relative = rel_to(path, &ctx.working_dir);

            // Find matching line indices
            let match_indices = matching_line_indices(&content, &lines, &regex, multiline);

            if match_indices.is_empty() {
                continue;
            }

            let mut matched: Vec<MatchedLine> = Vec::new();

            // Expand with context lines
            let mut printed_lines = std::collections::BTreeSet::new();
            for &mi in &match_indices {
                let start = mi.saturating_sub(before_ctx);
                let end = (mi + after_ctx + 1).min(lines.len());
                for li in start..end {
                    if !printed_lines.contains(&li) {
                        printed_lines.insert(li);
                        let is_match = li == mi;
                        matched.push(MatchedLine {
                            line_no: li + 1,
                            content: lines[li].to_string(),
                            is_match,
                        });
                    }
                }
            }

            total_matches += match_indices.len();
            file_matches.push((relative, matched));

            if total_matches >= max_results {
                break 'walk;
            }
        }

        // ---- Format output based on mode ----
        if file_matches.is_empty() {
            let note = if files_skipped > 0 {
                format!(" ({files_scanned} file(s) scanned, {files_skipped} skipped)")
            } else {
                format!(" ({files_scanned} file(s) scanned)")
            };
            return ToolOutput::success(format!("No matches found for: {pattern_str}{note}"));
        }

        match output_mode {
            "files_with_matches" => {
                let files: Vec<String> = file_matches.iter().map(|(f, _)| f.clone()).collect();
                ToolOutput::success(format!(
                    "Files matching '{}' ({} file(s)):\n\n{}",
                    pattern_str,
                    files.len(),
                    files.join("\n")
                ))
            }
            "count" => {
                let lines: Vec<String> = file_matches
                    .iter()
                    .map(|(f, matches)| {
                        let count = matches.iter().filter(|m| m.is_match).count();
                        format!("{f}: {count}")
                    })
                    .collect();
                ToolOutput::success(format!(
                    "Match counts for '{}':\n\n{}",
                    pattern_str,
                    lines.join("\n")
                ))
            }
            _ => {
                // "content" mode (default)
                let mut output_lines: Vec<String> = Vec::new();
                for (file, matches) in &file_matches {
                    // Separator between files
                    if !output_lines.is_empty() {
                        output_lines.push("--".to_string());
                    }

                    let mut prev_line_no = 0usize;
                    for m in matches {
                        // Add separator for gaps in context
                        if prev_line_no > 0 && m.line_no > prev_line_no + 1 {
                            output_lines.push("  ...".to_string());
                        }
                        let prefix = if m.is_match { ">" } else { " " };
                        output_lines
                            .push(format!("{prefix} {file}:{:>4} | {}", m.line_no, m.content));
                        prev_line_no = m.line_no;
                    }
                }

                let mut notes = Vec::new();
                if total_matches >= max_results {
                    notes.push(format!(
                        "Results truncated at {max_results} matching line(s). Use max_results to increase."
                    ));
                }
                if files_scanned >= max_files {
                    notes.push(format!(
                        "Stopped after scanning {max_files} file(s). Use max_files to increase."
                    ));
                }
                if files_skipped > 0 {
                    notes.push(format!("{files_skipped} file(s) skipped as binary, unreadable, or larger than max_file_bytes."));
                }
                let truncation_note = if notes.is_empty() {
                    String::new()
                } else {
                    format!("\n\n({})", notes.join(" "))
                };

                ToolOutput::success(format!(
                    "Found {} match(es) in {} file(s):\n\n{}{}",
                    total_matches,
                    file_matches.len(),
                    output_lines.join("\n"),
                    truncation_note
                ))
            }
        }
    }
}

struct MatchedLine {
    line_no: usize,
    content: String,
    is_match: bool,
}

/// Map type shorthand to file extensions
fn type_to_extensions(type_name: &str) -> Vec<&'static str> {
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
        "Find files by name or glob pattern. Matches file names and relative paths, respects .gitignore by default, and returns project-relative paths."
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

        let mut results = Vec::new();

        let mut builder = WalkBuilder::new(&search_path);
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

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if results.len() >= max_results {
                break;
            }

            let path = entry.path();
            if path == search_path {
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
                if matches_glob(gp, path, &search_path) {
                    continue;
                }
            }
            if matches_glob(&glob_pattern, path, &search_path) {
                results.push(rel_to(path, &ctx.working_dir));
            }
        }

        if results.is_empty() {
            ToolOutput::success(format!("No files found matching: {pattern_str}"))
        } else {
            let truncated = if results.len() >= max_results {
                format!("\n\n(Results truncated at {max_results} path(s). Use max_results to increase.)")
            } else {
                String::new()
            };
            ToolOutput::success(format!(
                "Found {} path(s):\n\n{}{}",
                results.len(),
                results.join("\n"),
                truncated
            ))
        }
    }
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

        // files_with_matches mode
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

        // count mode
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
        assert!(output.content.contains("2")); // 2 matches in a.rs
    }

    #[tokio::test]
    async fn test_search_files_fixed_string() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "foo.bar\nfoobar\nfoo+bar").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        // Fixed string: "foo.bar" should match literally, not as regex
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
}
