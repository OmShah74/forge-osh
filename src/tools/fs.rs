use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::fs;

use super::Tool;
use crate::agent::file_history;
use crate::types::*;

fn resolve_path(path_str: &str, ctx: &ToolContext) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.working_dir.join(path)
    }
}

// ─── read_file ────────────────────────────────────────────────────────────

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Optionally specify start and end line numbers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to file" },
                "start_line": { "type": "integer", "description": "Start line (1-indexed, optional)" },
                "end_line": { "type": "integer", "description": "End line (1-indexed, inclusive, optional)" }
            },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };
        let path = resolve_path(path_str, ctx);

        if !path.exists() {
            return ToolOutput::error(format!("File not found: {}", path.display()));
        }

        // Detect image files by extension — return metadata instead of binary error
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());
        let is_image = matches!(
            extension.as_deref(),
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" | "tiff" | "tif")
        );

        if is_image {
            let meta = fs::metadata(&path).await;
            let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mime = match extension.as_deref() {
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                Some("bmp") => "image/bmp",
                Some("ico") => "image/x-icon",
                Some("svg") => "image/svg+xml",
                Some("tiff") | Some("tif") => "image/tiff",
                _ => "image/unknown",
            };
            return ToolOutput::success(format!(
                "Image file: {}\nType: {}\nSize: {} bytes ({:.1} KB)\n\n\
                This is a binary image file. To analyze its visual content, \
                describe what you need from it and I can use web search or \
                other tools. SVG files can be read as text if you need the markup.",
                path.display(),
                mime,
                size_bytes,
                size_bytes as f64 / 1024.0
            ));
        }

        // Check for binary
        match fs::read(&path).await {
            Ok(bytes) => {
                if bytes.contains(&0) {
                    return ToolOutput::error(format!(
                        "File appears to be binary: {}",
                        path.display()
                    ));
                }
                let content = String::from_utf8_lossy(&bytes).to_string();
                let lines: Vec<&str> = content.lines().collect();
                let total = lines.len();

                let start = input["start_line"]
                    .as_u64()
                    .map(|n| (n as usize).saturating_sub(1))
                    .unwrap_or(0);
                let end = input["end_line"]
                    .as_u64()
                    .map(|n| n as usize)
                    .unwrap_or(total)
                    .min(total);

                if start >= total {
                    return ToolOutput::error(format!(
                        "Start line {} exceeds file length ({} lines)",
                        start + 1,
                        total
                    ));
                }

                let selected: Vec<String> = lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(i, line)| format!("{:>4} | {}", start + i + 1, line))
                    .collect();

                // Record the read so later edits can detect external mutation.
                if let Some(ref cache) = ctx.file_cache {
                    cache.record_read(&path);
                }

                ToolOutput::success(format!(
                    "File: {} ({} lines total, showing {}-{})\n\n{}",
                    path.display(),
                    total,
                    start + 1,
                    end,
                    selected.join("\n")
                ))
            }
            Err(e) => ToolOutput::error(format!("Failed to read file: {e}")),
        }
    }
}

// ─── write_file ───────────────────────────────────────────────────────────

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file, replacing its entire contents. Creates the file if it does not exist."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };
        let content = match input["content"].as_str() {
            Some(c) => c,
            None => return ToolOutput::error("Missing 'content' parameter"),
        };

        let path = resolve_path(path_str, ctx);

        // Refuse to silently overwrite content the agent hasn't observed yet.
        if let Some(ref cache) = ctx.file_cache {
            if let Err(msg) = cache.check_unchanged(&path) {
                return ToolOutput::error(msg);
            }
        }

        // Read old content before overwriting so we can show a diff
        let old_content = if path.exists() {
            fs::read_to_string(&path).await.unwrap_or_default()
        } else {
            String::new()
        };

        // Snapshot before overwriting (enables /undo)
        file_history::take_snapshot(&path).await;

        // Create parent directories
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return ToolOutput::error(format!("Failed to create directories: {e}"));
            }
        }

        match fs::write(&path, content).await {
            Ok(_) => {
                let action = if old_content.is_empty() {
                    "Created"
                } else {
                    "Updated"
                };
                let diff = generate_diff(&old_content, content);
                // Refresh the fingerprint so the next edit isn't blocked.
                if let Some(ref cache) = ctx.file_cache {
                    cache.record_write(&path);
                }
                ToolOutput::success(format!(
                    "{} {} ({} bytes)\n\n{}",
                    action,
                    path.display(),
                    content.len(),
                    diff
                ))
            }
            Err(e) => ToolOutput::error(format!("Failed to write file: {e}")),
        }
    }
}

// ─── edit_file ────────────────────────────────────────────────────────────

pub struct EditFileTool;

// ─── Helpers for robust string matching ───────────────────────────────────

/// Normalize all line endings to `\n` (handles \r\n and bare \r).
fn normalize_endings(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Strip leading/trailing whitespace from every line while preserving line count.
fn normalize_whitespace_per_line(s: &str) -> String {
    s.lines().map(|l| l.trim()).collect::<Vec<_>>().join("\n")
}

/// Find the closest matching regions in `content` for a given `needle`.
/// Returns `(start_line_1indexed, similarity_score, matched_snippet)`.
fn find_closest_matches(
    content: &str,
    needle: &str,
    max_results: usize,
) -> Vec<(usize, f64, String)> {
    let content_lines: Vec<&str> = content.lines().collect();
    let needle_lines: Vec<&str> = needle.lines().collect();
    let needle_len = needle_lines.len().max(1);

    // Normalize needle for comparison
    let needle_norm: Vec<String> = needle_lines.iter().map(|l| l.trim().to_string()).collect();
    let needle_joined = needle_norm.join("\n");

    let mut candidates: Vec<(usize, f64, String)> = Vec::new();

    for start in 0..content_lines
        .len()
        .saturating_sub(needle_len.saturating_sub(1))
    {
        let end = (start + needle_len).min(content_lines.len());
        let window: Vec<&str> = content_lines[start..end].to_vec();
        let window_norm: Vec<String> = window.iter().map(|l| l.trim().to_string()).collect();
        let window_joined = window_norm.join("\n");

        let similarity = strsim::normalized_levenshtein(&window_joined, &needle_joined);

        if similarity > 0.55 {
            let snippet = content_lines[start..end].join("\n");
            candidates.push((start + 1, similarity, snippet));
        }
    }

    // Sort by descending similarity
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(max_results);
    candidates
}

/// Attempt to apply an edit using various matching strategies.
/// Returns Ok(new_content) on success, or Err(diagnostic_message) on failure.
fn apply_edit_robust(
    content: &str,
    old_str: &str,
    new_str: &str,
    edit_index: usize,
) -> Result<(String, String), String> {
    // Strategy 1: Exact match (original behavior)
    let count = content.matches(old_str).count();
    if count == 1 {
        let result = content.replacen(old_str, new_str, 1);
        return Ok((
            result,
            format!("  Edit {}: applied (exact match)", edit_index),
        ));
    }
    if count > 1 {
        return Err(format!(
            "Edit {}: old_str found {} times (must be unique). Include more surrounding \
             context lines in old_str to narrow it down to exactly one match.",
            edit_index, count
        ));
    }

    // Strategy 2: CRLF-normalized match
    let norm_content = normalize_endings(content);
    let norm_old = normalize_endings(old_str);
    let norm_new = normalize_endings(new_str);
    let norm_count = norm_content.matches(&norm_old).count();
    if norm_count == 1 {
        let result = norm_content.replacen(&norm_old, &norm_new, 1);
        return Ok((
            result,
            format!(
                "  Edit {}: applied (auto-fixed line endings \\r\\n → \\n)",
                edit_index
            ),
        ));
    }

    // Strategy 3: Whitespace-normalized match (trim each line)
    let ws_content = normalize_whitespace_per_line(&norm_content);
    let ws_old = normalize_whitespace_per_line(&norm_old);
    if !ws_old.is_empty() {
        let ws_count = ws_content.matches(&ws_old).count();
        if ws_count == 1 {
            // Find the position in the whitespace-normalized version
            if let Some(ws_pos) = ws_content.find(&ws_old) {
                // Map back to the original: count how many newlines before ws_pos
                let line_start = ws_content[..ws_pos].matches('\n').count();
                let line_count = ws_old.matches('\n').count() + 1;
                let orig_lines: Vec<&str> = norm_content.lines().collect();
                let end_line = (line_start + line_count).min(orig_lines.len());

                // Replace those lines in the normalized content
                let mut result_lines: Vec<&str> = Vec::new();
                result_lines.extend_from_slice(&orig_lines[..line_start]);
                for line in norm_new.lines() {
                    result_lines.push(line);
                }
                result_lines.extend_from_slice(&orig_lines[end_line..]);
                let result = result_lines.join("\n");
                // Preserve trailing newline if original had one
                let result = if norm_content.ends_with('\n') && !result.ends_with('\n') {
                    result + "\n"
                } else {
                    result
                };
                return Ok((
                    result,
                    format!(
                        "  Edit {}: applied (auto-fixed whitespace differences at line {})",
                        edit_index,
                        line_start + 1
                    ),
                ));
            }
        }
    }

    // All strategies failed — build rich diagnostic error
    let candidates = find_closest_matches(&norm_content, old_str, 3);

    let mut err_msg = format!("Edit {}: old_str not found in file.\n", edit_index);

    if !candidates.is_empty() {
        err_msg.push_str("\n── Closest matches found ──\n");
        for (line, score, snippet) in &candidates {
            let preview: String = snippet
                .lines()
                .take(5)
                .map(|l| format!("    │ {l}"))
                .collect::<Vec<_>>()
                .join("\n");
            err_msg.push_str(&format!(
                "  Line {line} ({:.0}% similar):\n{preview}\n\n",
                score * 100.0
            ));
        }
        err_msg.push_str(
            "TIP: Your old_str has whitespace, line-ending, or content differences \
             from what is actually in the file. Copy the EXACT text from a fresh \
             read_file call. If edit_file keeps failing, use write_file to replace \
             the entire file contents instead.",
        );
    } else {
        err_msg.push_str(
            "\nThe text you provided does not exist anywhere in the file (no close matches found).\n\
             The file may have been modified since you last read it.\n\n\
             RECOVERY: Use read_file to get the current content, then either:\n\
             1. Retry edit_file with the EXACT text from the file, OR\n\
             2. Use write_file with the complete corrected file contents."
        );
    }

    Err(err_msg)
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Apply targeted edits to a file using search-and-replace. The old_str must uniquely \
         identify the location. Automatically handles line-ending and whitespace normalization."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_str": { "type": "string", "description": "Exact string to find (must be unique)" },
                            "new_str": { "type": "string", "description": "Replacement string" }
                        },
                        "required": ["old_str", "new_str"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };
        let edits = match input["edits"].as_array() {
            Some(e) => e,
            None => return ToolOutput::error("Missing 'edits' parameter"),
        };

        let path = resolve_path(path_str, ctx);

        if !path.exists() {
            return ToolOutput::error(format!("File not found: {}", path.display()));
        }

        // Refuse to edit if the on-disk content has changed since the last read.
        if let Some(ref cache) = ctx.file_cache {
            if let Err(msg) = cache.check_unchanged(&path) {
                return ToolOutput::error(msg);
            }
        }

        // Snapshot before editing (enables /undo)
        file_history::take_snapshot(&path).await;

        let mut content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("Failed to read file: {e}")),
        };

        let original = content.clone();
        let mut changes = Vec::new();

        for (i, edit) in edits.iter().enumerate() {
            let old_str = match edit["old_str"].as_str() {
                Some(s) => s,
                None => return ToolOutput::error(format!("Edit {}: missing old_str", i + 1)),
            };
            let new_str = match edit["new_str"].as_str() {
                Some(s) => s,
                None => return ToolOutput::error(format!("Edit {}: missing new_str", i + 1)),
            };

            match apply_edit_robust(&content, old_str, new_str, i + 1) {
                Ok((new_content, change_msg)) => {
                    content = new_content;
                    changes.push(change_msg);
                }
                Err(err_msg) => {
                    return ToolOutput::error(err_msg);
                }
            }
        }

        match fs::write(&path, &content).await {
            Ok(_) => {
                // Generate a simple diff summary
                let diff = generate_diff(&original, &content);
                let change_notes = changes.join("\n");
                if let Some(ref cache) = ctx.file_cache {
                    cache.record_write(&path);
                }
                ToolOutput::success(format!(
                    "Applied {} edit(s) to {}\n{}\n\n{}",
                    edits.len(),
                    path.display(),
                    change_notes,
                    diff
                ))
            }
            Err(e) => ToolOutput::error(format!("Failed to write file: {e}")),
        }
    }
}

fn generate_diff(old: &str, new: &str) -> String {
    use similar::TextDiff;
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Delete => "-",
            similar::ChangeTag::Insert => "+",
            similar::ChangeTag::Equal => " ",
        };
        output.push_str(&format!("{sign}{change}"));
    }
    output
}

// ─── create_file ──────────────────────────────────────────────────────────

pub struct CreateFileTool;

#[async_trait]
impl Tool for CreateFileTool {
    fn name(&self) -> &str {
        "create_file"
    }

    fn description(&self) -> &str {
        "Create a new file with content. Errors if the file already exists."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };
        let content = match input["content"].as_str() {
            Some(c) => c,
            None => return ToolOutput::error("Missing 'content' parameter"),
        };

        let path = resolve_path(path_str, ctx);

        if path.exists() {
            return ToolOutput::error(format!("File already exists: {}", path.display()));
        }

        // Snapshot the (non-existent) path so /undo can delete the created file
        file_history::take_snapshot(&path).await;

        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return ToolOutput::error(format!("Failed to create directories: {e}"));
            }
        }

        match fs::write(&path, content).await {
            Ok(_) => {
                let diff = generate_diff("", content);
                if let Some(ref cache) = ctx.file_cache {
                    cache.record_write(&path);
                }
                ToolOutput::success(format!(
                    "Created {} ({} bytes)\n\n{}",
                    path.display(),
                    content.len(),
                    diff
                ))
            }
            Err(e) => ToolOutput::error(format!("Failed to create file: {e}")),
        }
    }
}

// ─── delete_file ──────────────────────────────────────────────────────────

pub struct DeleteFileTool;

#[async_trait]
impl Tool for DeleteFileTool {
    fn name(&self) -> &str {
        "delete_file"
    }

    fn description(&self) -> &str {
        "Delete a file or empty directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Destructive
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };
        let path = resolve_path(path_str, ctx);

        if !path.exists() {
            return ToolOutput::error(format!("Path not found: {}", path.display()));
        }

        // Snapshot before deletion (enables /undo for files; dirs not restored)
        if path.is_file() {
            file_history::take_snapshot(&path).await;
        }

        if path.is_dir() {
            match fs::remove_dir_all(&path).await {
                Ok(_) => {
                    if let Some(ref cache) = ctx.file_cache {
                        cache.invalidate(&path);
                    }
                    ToolOutput::success(format!("Deleted directory: {}", path.display()))
                }
                Err(e) => ToolOutput::error(format!("Failed to delete directory: {e}")),
            }
        } else {
            match fs::remove_file(&path).await {
                Ok(_) => {
                    if let Some(ref cache) = ctx.file_cache {
                        cache.invalidate(&path);
                    }
                    ToolOutput::success(format!("Deleted file: {}", path.display()))
                }
                Err(e) => ToolOutput::error(format!("Failed to delete file: {e}")),
            }
        }
    }
}

// ─── list_directory ───────────────────────────────────────────────────────

pub struct ListDirectoryTool;

#[async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "List contents of a directory with optional recursive traversal and filtering."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "recursive": { "type": "boolean", "default": false },
                "max_depth": { "type": "integer", "default": 3 },
                "include_hidden": { "type": "boolean", "default": false },
                "filter": { "type": "string", "description": "Glob pattern filter (e.g., '*.rs')" }
            },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };
        let path = resolve_path(path_str, ctx);
        let recursive = input["recursive"].as_bool().unwrap_or(false);
        let max_depth = input["max_depth"].as_u64().unwrap_or(3) as usize;
        let include_hidden = input["include_hidden"].as_bool().unwrap_or(false);
        let filter = input["filter"].as_str();

        if !path.exists() {
            return ToolOutput::error(format!("Directory not found: {}", path.display()));
        }

        if !path.is_dir() {
            return ToolOutput::error(format!("Not a directory: {}", path.display()));
        }

        let glob_pattern = filter.and_then(|f| glob::Pattern::new(f).ok());

        let mut entries = Vec::new();
        let walker = walkdir::WalkDir::new(&path)
            .max_depth(if recursive { max_depth } else { 1 })
            .follow_links(false);

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            let entry_path = entry.path();
            if entry_path == path {
                continue;
            }

            let file_name = entry.file_name().to_string_lossy();
            if !include_hidden && file_name.starts_with('.') {
                continue;
            }

            if let Some(ref pattern) = glob_pattern {
                if !pattern.matches(&file_name) && !entry_path.is_dir() {
                    continue;
                }
            }

            let relative = entry_path.strip_prefix(&path).unwrap_or(entry_path);
            let depth = relative.components().count();
            let indent = "  ".repeat(depth.saturating_sub(1));
            let type_indicator = if entry_path.is_dir() { "/" } else { "" };

            entries.push(format!("{indent}{file_name}{type_indicator}"));
        }

        if entries.is_empty() {
            ToolOutput::success(format!("Directory is empty: {}", path.display()))
        } else {
            ToolOutput::success(format!(
                "Contents of {}:\n{}",
                path.display(),
                entries.join("\n")
            ))
        }
    }
}

// ─── move_file ────────────────────────────────────────────────────────────

pub struct MoveFileTool;

#[async_trait]
impl Tool for MoveFileTool {
    fn name(&self) -> &str {
        "move_file"
    }

    fn description(&self) -> &str {
        "Move or rename a file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" },
                "destination": { "type": "string" }
            },
            "required": ["source", "destination"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let src = match input["source"].as_str() {
            Some(p) => resolve_path(p, ctx),
            None => return ToolOutput::error("Missing 'source' parameter"),
        };
        let dst = match input["destination"].as_str() {
            Some(p) => resolve_path(p, ctx),
            None => return ToolOutput::error("Missing 'destination' parameter"),
        };

        if !src.exists() {
            return ToolOutput::error(format!("Source not found: {}", src.display()));
        }

        if let Some(parent) = dst.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return ToolOutput::error(format!("Failed to create directories: {e}"));
            }
        }

        match fs::rename(&src, &dst).await {
            Ok(_) => ToolOutput::success(format!("Moved {} -> {}", src.display(), dst.display())),
            Err(e) => ToolOutput::error(format!("Failed to move file: {e}")),
        }
    }
}

// ─── copy_file ────────────────────────────────────────────────────────────

pub struct CopyFileTool;

#[async_trait]
impl Tool for CopyFileTool {
    fn name(&self) -> &str {
        "copy_file"
    }

    fn description(&self) -> &str {
        "Copy a file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" },
                "destination": { "type": "string" }
            },
            "required": ["source", "destination"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let src = match input["source"].as_str() {
            Some(p) => resolve_path(p, ctx),
            None => return ToolOutput::error("Missing 'source' parameter"),
        };
        let dst = match input["destination"].as_str() {
            Some(p) => resolve_path(p, ctx),
            None => return ToolOutput::error("Missing 'destination' parameter"),
        };

        if !src.exists() {
            return ToolOutput::error(format!("Source not found: {}", src.display()));
        }

        if let Some(parent) = dst.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return ToolOutput::error(format!("Failed to create directories: {e}"));
            }
        }

        match fs::copy(&src, &dst).await {
            Ok(bytes) => ToolOutput::success(format!(
                "Copied {} -> {} ({} bytes)",
                src.display(),
                dst.display(),
                bytes
            )),
            Err(e) => ToolOutput::error(format!("Failed to copy file: {e}")),
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
            active_skill_scope: None,
                skill_registry: None,
        }
    }

    #[tokio::test]
    async fn test_read_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "line1\nline2\nline3\n").unwrap();

        let tool = ReadFileTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(json!({"path": file.to_str().unwrap()}), &ctx)
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("line1"));
        assert!(output.content.contains("line3"));
    }

    #[tokio::test]
    async fn test_read_file_range() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();

        let tool = ReadFileTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({"path": file.to_str().unwrap(), "start_line": 2, "end_line": 4}),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("b"));
        assert!(output.content.contains("d"));
    }

    #[tokio::test]
    async fn test_write_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("new.txt");

        let tool = WriteFileTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({"path": file.to_str().unwrap(), "content": "hello world"}),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn test_edit_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("edit.txt");
        std::fs::write(&file, "fn main() { println!(\"hello\"); }").unwrap();

        let tool = EditFileTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(
                json!({
                    "path": file.to_str().unwrap(),
                    "edits": [{"old_str": "hello", "new_str": "world"}]
                }),
                &ctx,
            )
            .await;
        assert!(!output.is_error);
        assert!(std::fs::read_to_string(&file).unwrap().contains("world"));
    }

    #[tokio::test]
    async fn test_list_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();

        let tool = ListDirectoryTool;
        let ctx = test_ctx(dir.path());
        let output = tool
            .execute(json!({"path": dir.path().to_str().unwrap()}), &ctx)
            .await;
        assert!(!output.is_error);
        assert!(output.content.contains("a.txt"));
        assert!(output.content.contains("b.rs"));
    }
}
