//! NotebookReadTool — read Jupyter .ipynb notebooks as human-readable text.
//! Cells are displayed with their type (code/markdown/raw), source, and output.
//! No new dependencies required — uses serde_json which is already in the project.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::types::*;
use super::Tool;

// ---------------------------------------------------------------------------
// NotebookReadTool
// ---------------------------------------------------------------------------

pub struct NotebookReadTool;

#[async_trait]
impl Tool for NotebookReadTool {
    fn name(&self) -> &str { "notebook_read" }

    fn description(&self) -> &str {
        "Read a Jupyter notebook (.ipynb) file and return its cells as formatted text. \
        Shows cell type (code/markdown/raw), source code, and cell outputs (stdout, stderr, \
        results). Use this to understand notebook content before making edits with write_file \
        or edit_file. Works on any .ipynb file without needing Jupyter installed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the .ipynb notebook file"
                },
                "include_outputs": {
                    "type": "boolean",
                    "description": "Include cell outputs in the result (default: true)"
                }
            },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel { PermissionLevel::ReadOnly }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let path_str = match input["path"].as_str() {
            Some(p) => p,
            None => return ToolOutput::error("Missing 'path' parameter"),
        };

        let path = {
            let p = std::path::Path::new(path_str);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                ctx.working_dir.join(p)
            }
        };

        if !path.exists() {
            return ToolOutput::error(format!("File not found: {}", path.display()));
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("Failed to read file: {e}")),
        };

        let notebook: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => return ToolOutput::error(format!("Invalid JSON in notebook: {e}")),
        };

        // Validate it looks like a notebook
        if notebook["nbformat"].is_null() && notebook["cells"].is_null() {
            return ToolOutput::error("File does not appear to be a valid Jupyter notebook");
        }

        let include_outputs = input["include_outputs"].as_bool().unwrap_or(true);

        let cells = match notebook["cells"].as_array() {
            Some(c) => c,
            None => return ToolOutput::error("Notebook has no 'cells' array"),
        };

        let kernel_name = notebook["metadata"]["kernelspec"]["display_name"]
            .as_str()
            .or_else(|| notebook["metadata"]["kernelspec"]["name"].as_str())
            .unwrap_or("unknown");

        let nbformat = notebook["nbformat"].as_u64().unwrap_or(0);
        let total_cells = cells.len();

        let mut output = format!(
            "Notebook: {}\nKernel: {}  |  Format: nbformat {}  |  Cells: {}\n\n",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            kernel_name,
            nbformat,
            total_cells
        );

        for (i, cell) in cells.iter().enumerate() {
            let cell_type = cell["cell_type"].as_str().unwrap_or("unknown");
            let execution_count = cell["execution_count"].as_u64();

            // Cell header
            let header = match (cell_type, execution_count) {
                ("code", Some(n)) => format!("╔═ Cell {} [{}] (code) ══════", i + 1, n),
                ("code", None) => format!("╔═ Cell {} [ ] (code) ══════", i + 1),
                ("markdown", _) => format!("╔═ Cell {} (markdown) ══════", i + 1),
                ("raw", _) => format!("╔═ Cell {} (raw) ══════", i + 1),
                _ => format!("╔═ Cell {} ({}) ══════", i + 1, cell_type),
            };
            output.push_str(&header);
            output.push('\n');

            // Cell source
            let source = extract_source(&cell["source"]);
            if source.is_empty() {
                output.push_str("(empty cell)\n");
            } else {
                output.push_str(&source);
                if !source.ends_with('\n') {
                    output.push('\n');
                }
            }

            // Cell outputs
            if include_outputs && cell_type == "code" {
                if let Some(outputs) = cell["outputs"].as_array() {
                    if !outputs.is_empty() {
                        output.push_str("╠═ Output ══════\n");
                        for out in outputs {
                            let out_type = out["output_type"].as_str().unwrap_or("unknown");
                            match out_type {
                                "stream" => {
                                    let stream_name = out["name"].as_str().unwrap_or("stdout");
                                    let text = extract_source(&out["text"]);
                                    output.push_str(&format!("[{}] {}", stream_name, text));
                                    if !text.ends_with('\n') { output.push('\n'); }
                                }
                                "execute_result" | "display_data" => {
                                    // Try text/plain first
                                    if let Some(text) = out["data"]["text/plain"].as_str() {
                                        output.push_str(text);
                                        if !text.ends_with('\n') { output.push('\n'); }
                                    } else if let Some(arr) = out["data"]["text/plain"].as_array() {
                                        let text = join_source_array(arr);
                                        output.push_str(&text);
                                        if !text.ends_with('\n') { output.push('\n'); }
                                    } else if out["data"]["image/png"].is_string() {
                                        output.push_str("[image/png output — binary, not shown]\n");
                                    } else if out["data"]["text/html"].is_string() {
                                        output.push_str("[HTML output — use read_file to inspect]\n");
                                    }
                                }
                                "error" => {
                                    let ename = out["ename"].as_str().unwrap_or("Error");
                                    let evalue = out["evalue"].as_str().unwrap_or("");
                                    output.push_str(&format!("[Error] {}: {}\n", ename, evalue));
                                }
                                _ => {
                                    output.push_str(&format!("[{} output]\n", out_type));
                                }
                            }
                        }
                    }
                }
            }

            output.push_str("╚═══════════════\n\n");
        }

        ToolOutput::success(output)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract source from either a string or array-of-strings (both valid in nbformat)
fn extract_source(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(arr) => join_source_array(arr),
        _ => String::new(),
    }
}

fn join_source_array(arr: &[Value]) -> String {
    arr.iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join("")
}
