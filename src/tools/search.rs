use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::path::Path;

use crate::types::*;
use super::Tool;

// ─── search_files ─────────────────────────────────────────────────────────

pub struct SearchFilesTool;

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str { "search_files" }

    fn description(&self) -> &str {
        "Search for text patterns in files. Returns matching lines with file paths and line numbers. Respects .gitignore."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Search pattern (regex supported)" },
                "path": { "type": "string", "description": "Directory to search in (default: cwd)" },
                "file_pattern": { "type": "string", "description": "Only search files matching this glob (e.g., '*.rs')" },
                "case_sensitive": { "type": "boolean", "default": false },
                "max_results": { "type": "integer", "default": 50 }
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
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    ctx.working_dir.join(path)
                }
            })
            .unwrap_or_else(|| ctx.working_dir.clone());

        let case_sensitive = input["case_sensitive"].as_bool().unwrap_or(false);
        let max_results = input["max_results"].as_u64().unwrap_or(50) as usize;
        let file_pattern = input["file_pattern"].as_str();

        let regex = match RegexBuilder::new(pattern_str)
            .case_insensitive(!case_sensitive)
            .build()
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("Invalid regex: {e}")),
        };

        let glob_pattern = file_pattern.and_then(|p| glob::Pattern::new(p).ok());

        let mut results = Vec::new();

        let walker = WalkBuilder::new(&search_path)
            .hidden(true)
            .git_ignore(true)
            .build();

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if results.len() >= max_results {
                break;
            }

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Apply file pattern filter
            if let Some(ref gp) = glob_pattern {
                if let Some(name) = path.file_name() {
                    if !gp.matches(&name.to_string_lossy()) {
                        continue;
                    }
                }
            }

            // Read file
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue, // Skip binary or unreadable files
            };

            let relative = path
                .strip_prefix(&search_path)
                .unwrap_or(path)
                .to_string_lossy();

            for (line_num, line) in content.lines().enumerate() {
                if results.len() >= max_results {
                    break;
                }
                if regex.is_match(line) {
                    results.push(format!("{}:{}: {}", relative, line_num + 1, line.trim()));
                }
            }
        }

        if results.is_empty() {
            ToolOutput::success(format!("No matches found for pattern: {pattern_str}"))
        } else {
            let total = results.len();
            ToolOutput::success(format!(
                "Found {} match(es):\n\n{}",
                total,
                results.join("\n")
            ))
        }
    }
}

// ─── find_files ───────────────────────────────────────────────────────────

pub struct FindFilesTool;

#[async_trait]
impl Tool for FindFilesTool {
    fn name(&self) -> &str { "find_files" }

    fn description(&self) -> &str {
        "Find files by name/glob pattern. Respects .gitignore."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "File name pattern (glob, e.g., '*.rs', 'test_*.py')" },
                "path": { "type": "string", "description": "Directory to search in (default: cwd)" },
                "max_results": { "type": "integer", "default": 50 }
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
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    ctx.working_dir.join(path)
                }
            })
            .unwrap_or_else(|| ctx.working_dir.clone());

        let max_results = input["max_results"].as_u64().unwrap_or(50) as usize;

        let glob_pattern = match glob::Pattern::new(pattern_str) {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(format!("Invalid glob pattern: {e}")),
        };

        let mut results = Vec::new();

        let walker = WalkBuilder::new(&search_path)
            .hidden(true)
            .git_ignore(true)
            .build();

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if results.len() >= max_results {
                break;
            }

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
        }
    }

    #[tokio::test]
    async fn test_search_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.rs"), "fn hello_world() {}\nfn goodbye() {}").unwrap();

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
}
