use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::path::Path;

use crate::types::*;
use super::Tool;

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
    fn name(&self) -> &str { "search_files" }
    fn is_concurrency_safe(&self) -> bool { true }

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
                    "description": "Only search files matching this glob (e.g. '*.rs', 'test_*.py')"
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
                }
            },
            "required": ["pattern"]
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let pattern_str = match input["pattern"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'pattern' parameter"),
        };

        let search_path = input["path"]
            .as_str()
            .map(|p| {
                let path = Path::new(p);
                if path.is_absolute() { path.to_path_buf() } else { ctx.working_dir.join(path) }
            })
            .unwrap_or_else(|| ctx.working_dir.clone());

        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(false);
        let fixed_string = input["fixed_string"].as_bool().unwrap_or(false);
        let multiline = input["multiline"].as_bool().unwrap_or(false);
        let max_results = input["max_results"].as_u64().unwrap_or(100) as usize;
        let file_pattern = input["file_pattern"].as_str();
        let type_filter = input["type_filter"].as_str();

        // Output mode
        let output_mode = input["output_mode"].as_str().unwrap_or("content");

        // Context lines
        let ctx_both = input["context"].as_u64().map(|n| n as usize);
        let before_ctx = ctx_both.unwrap_or_else(|| input["before_context"].as_u64().unwrap_or(0) as usize);
        let after_ctx = ctx_both.unwrap_or_else(|| input["after_context"].as_u64().unwrap_or(0) as usize);

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
        let glob_pattern = file_pattern.and_then(|p| glob::Pattern::new(p).ok());

        // ---- Walk and search ----
        let mut file_matches: Vec<(String, Vec<MatchedLine>)> = Vec::new();
        let mut total_matches = 0usize;

        let walker = WalkBuilder::new(&search_path)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .build();

        'walk: for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if total_matches >= max_results { break; }
            let path = entry.path();
            if !path.is_file() { continue; }

            // Apply file type filter
            let file_name = entry.file_name().to_string_lossy();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            if !type_extensions.is_empty() && !type_extensions.iter().any(|e| *e == ext) {
                continue;
            }
            if let Some(ref gp) = glob_pattern {
                if !gp.matches(&file_name) {
                    continue;
                }
            }

            // Read file
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let lines: Vec<&str> = content.lines().collect();
            let relative = path.strip_prefix(&search_path).unwrap_or(path).to_string_lossy().to_string();

            // Find matching line indices
            let mut match_indices: Vec<usize> = Vec::new();
            for (i, line) in lines.iter().enumerate() {
                if regex.is_match(line) {
                    match_indices.push(i);
                }
            }

            if match_indices.is_empty() { continue; }

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

            if total_matches >= max_results { break 'walk; }
        }

        // ---- Format output based on mode ----
        if file_matches.is_empty() {
            return ToolOutput::success(format!("No matches found for: {pattern_str}"));
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
                let lines: Vec<String> = file_matches.iter().map(|(f, matches)| {
                    let count = matches.iter().filter(|m| m.is_match).count();
                    format!("{f}: {count}")
                }).collect();
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
                        output_lines.push(format!(
                            "{prefix} {file}:{:>4} | {}",
                            m.line_no, m.content
                        ));
                        prev_line_no = m.line_no;
                    }
                }

                let truncation_note = if total_matches >= max_results {
                    format!("\n\n(Results truncated at {max_results} matches. Use max_results to increase.)")
                } else {
                    String::new()
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
    fn name(&self) -> &str { "find_files" }
    fn is_concurrency_safe(&self) -> bool { true }

    fn description(&self) -> &str {
        "Find files by name or glob pattern. Respects .gitignore. Returns paths relative to the search directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "File name pattern (glob, e.g. '*.rs', 'test_*.py', '**/*.json')"
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
                }
            },
            "required": ["pattern"]
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let pattern_str = match input["pattern"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'pattern' parameter"),
        };

        let search_path = input["path"]
            .as_str()
            .map(|p| {
                let path = Path::new(p);
                if path.is_absolute() { path.to_path_buf() } else { ctx.working_dir.join(path) }
            })
            .unwrap_or_else(|| ctx.working_dir.clone());

        let max_results = input["max_results"].as_u64().unwrap_or(100) as usize;
        let include_hidden = input["include_hidden"].as_bool().unwrap_or(false);

        let glob_pattern = match glob::Pattern::new(pattern_str) {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid glob pattern: {e}")),
        };

        let mut results = Vec::new();

        let walker = WalkBuilder::new(&search_path)
            .hidden(!include_hidden)
            .git_ignore(true)
            .git_global(true)
            .build();

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if results.len() >= max_results { break; }

            let path = entry.path();
            if let Some(name) = path.file_name() {
                if glob_pattern.matches(&name.to_string_lossy()) {
                    let relative = path
                        .strip_prefix(&search_path)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();
                    results.push(relative);
                }
            }
        }

        if results.is_empty() {
            ToolOutput::success(format!("No files found matching: {pattern_str}"))
        } else {
            ToolOutput::success(format!(
                "Found {} file(s):\n\n{}",
                results.len(),
                results.join("\n")
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
        file_cache: None,
        }
    }

    #[tokio::test]
    async fn test_search_files_basic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.rs"), "fn hello_world() {}\nfn goodbye() {}").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(json!({"pattern": "hello", "path": dir.path().to_str().unwrap()}), &ctx)
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("hello_world"));
    }

    #[tokio::test]
    async fn test_search_files_context() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.rs"), "line1\nline2\nline3\nline4\nline5").unwrap();

        let tool = SearchFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(json!({
                "pattern": "line3",
                "path": dir.path().to_str().unwrap(),
                "context": 1
            }), &ctx)
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
        let output = tool.execute(json!({
            "pattern": "hello",
            "path": dir.path().to_str().unwrap(),
            "output_mode": "files_with_matches"
        }), &ctx).await;
        assert!(!output.is_error);
        assert!(output.content.contains("a.rs"));
        assert!(!output.content.contains("b.rs"));

        // count mode
        let output = tool.execute(json!({
            "pattern": "hello",
            "path": dir.path().to_str().unwrap(),
            "output_mode": "count"
        }), &ctx).await;
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
        let output = tool.execute(json!({
            "pattern": "foo.bar",
            "path": dir.path().to_str().unwrap(),
            "fixed_string": true
        }), &ctx).await;
        assert!(!output.is_error);
        assert!(output.content.contains("foo.bar"));
    }

    #[tokio::test]
    async fn test_find_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("foo.rs"), "").unwrap();
        std::fs::write(dir.path().join("bar.txt"), "").unwrap();

        let tool = FindFilesTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(json!({"pattern": "*.rs", "path": dir.path().to_str().unwrap()}), &ctx)
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("foo.rs"));
        assert!(!output.content.contains("bar.txt"));
    }

    #[test]
    fn test_type_to_extensions() {
        assert_eq!(type_to_extensions("rs"), vec!["rs"]);
        assert_eq!(type_to_extensions("py"), vec!["py", "pyi"]);
        assert!(type_to_extensions("unknown").is_empty());
    }
}
