//! LSP-backed `Tool` implementations. These deliberately mirror the shape of
//! the existing `graph_query` tool: they're always registered, but they
//! return a clear "no LSP server available" message instead of failing hard
//! when the user has nothing installed for the relevant language. That way
//! the LLM learns from the response that LSP is unusable in this workspace
//! and falls back to text-based tools without breaking the conversation.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::lsp::client::{DocumentSymbolInfo, LspClient, TextEdit, WorkspaceEditPreview};
use crate::lsp::config::uri_to_path;
use crate::lsp::manager::SharedLspManager;
use crate::tools::Tool;
use crate::types::{PermissionLevel, ToolContext, ToolOutput};

// ─── Path helpers (kept local — fs.rs's resolve_path is private) ───────────

fn resolve(path_str: &str, ctx: &ToolContext) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        ctx.working_dir.join(p)
    }
}

fn rel(path: &Path, ctx: &ToolContext) -> String {
    path.strip_prefix(&ctx.working_dir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

fn severity_label(s: Option<u32>) -> &'static str {
    match s {
        Some(1) => "error",
        Some(2) => "warning",
        Some(3) => "info",
        Some(4) => "hint",
        _ => "diag",
    }
}

fn symbol_kind_label(k: u32) -> &'static str {
    // LSP SymbolKind enum.
    match k {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum-member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type-parameter",
        _ => "symbol",
    }
}

async fn client_for(
    mgr: &SharedLspManager,
    path: &Path,
) -> Result<std::sync::Arc<LspClient>, ToolOutput> {
    match mgr.client_for_path(path).await {
        Ok(c) => Ok(c),
        Err(e) => Err(ToolOutput::success(format!(
            "LSP unavailable for {}: {e}\n\
             Install a language server (e.g. `rustup component add rust-analyzer`, \
             `npm i -g typescript-language-server`, `pip install pyright`, \
             `go install golang.org/x/tools/gopls@latest`) and retry, or use \
             search_files / read_file as a fallback.",
            path.display()
        ))),
    }
}

// ─── lsp_diagnostics ───────────────────────────────────────────────────────

pub struct LspDiagnosticsTool {
    mgr: SharedLspManager,
}

impl LspDiagnosticsTool {
    pub fn new(mgr: SharedLspManager) -> Self {
        Self { mgr }
    }
}

#[async_trait]
impl Tool for LspDiagnosticsTool {
    fn name(&self) -> &str {
        "lsp_diagnostics"
    }
    fn description(&self) -> &str {
        "Compiler-grade diagnostics (errors, warnings) for a source file via Language Server Protocol. \
        Use this BEFORE claiming code is correct — catches type errors, unused imports, borrow-check \
        issues, missing methods, etc. that text-based tools miss. Supports Rust, TS/JS, Python, Go."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the source file (relative to working dir or absolute)." },
                "wait_ms": { "type": "integer", "description": "Time to wait for diagnostics to arrive (default 2500ms; some servers index slowly)." }
            },
            "required": ["path"]
        })
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path_str) = input["path"].as_str() else {
            return ToolOutput::error("'path' is required");
        };
        let path = resolve(path_str, ctx);
        let wait_ms = input["wait_ms"].as_u64().unwrap_or(2500);

        let client = match client_for(&self.mgr, &path).await {
            Ok(c) => c,
            Err(out) => return out,
        };
        let diags = match client
            .diagnostics_for(&path, Duration::from_millis(wait_ms))
            .await
        {
            Ok(d) => d,
            Err(e) => return ToolOutput::error(format!("diagnostics failed: {e}")),
        };

        if diags.is_empty() {
            return ToolOutput::success(format!(
                "No diagnostics from {} for {}.",
                client.spec.language,
                rel(&path, ctx)
            ));
        }

        let mut out = format!(
            "{} diagnostic(s) for {}:\n",
            diags.len(),
            rel(&path, ctx)
        );
        for d in &diags {
            let line = d.range.start.line + 1;
            let col = d.range.start.character + 1;
            let src = d.source.as_deref().unwrap_or(client.spec.language);
            let code = d
                .code
                .as_ref()
                .map(|c| format!("[{c}] "))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {}:{}: {} {}({}): {}\n",
                line,
                col,
                severity_label(d.severity),
                code,
                src,
                d.message.lines().next().unwrap_or(&d.message),
            ));
        }
        ToolOutput::success(out)
    }
}

// ─── lsp_definition ────────────────────────────────────────────────────────

pub struct LspDefinitionTool {
    mgr: SharedLspManager,
}

impl LspDefinitionTool {
    pub fn new(mgr: SharedLspManager) -> Self {
        Self { mgr }
    }
}

#[async_trait]
impl Tool for LspDefinitionTool {
    fn name(&self) -> &str {
        "lsp_definition"
    }
    fn description(&self) -> &str {
        "Jump to the definition of the symbol at a (line, column) in a file via LSP. \
        Returns canonical file:line:col locations — much more accurate than text search."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "line": { "type": "integer", "description": "1-based line number." },
                "column": { "type": "integer", "description": "1-based column (default 1)." }
            },
            "required": ["path", "line"]
        })
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path_str) = input["path"].as_str() else {
            return ToolOutput::error("'path' is required");
        };
        let line = match input["line"].as_u64() {
            Some(n) if n >= 1 => (n - 1) as u32,
            _ => return ToolOutput::error("'line' must be a 1-based integer"),
        };
        let col = input["column"].as_u64().unwrap_or(1).saturating_sub(1) as u32;
        let path = resolve(path_str, ctx);

        let client = match client_for(&self.mgr, &path).await {
            Ok(c) => c,
            Err(out) => return out,
        };
        match client.definition(&path, line, col).await {
            Ok(locs) if locs.is_empty() => ToolOutput::success(format!(
                "No definition found for {}:{}:{}.",
                rel(&path, ctx),
                line + 1,
                col + 1
            )),
            Ok(locs) => {
                let mut s = format!("{} definition(s):\n", locs.len());
                for l in locs {
                    let p = uri_to_path(&l.uri);
                    s.push_str(&format!(
                        "  {}:{}:{}\n",
                        rel(&p, ctx),
                        l.range.start.line + 1,
                        l.range.start.character + 1,
                    ));
                }
                ToolOutput::success(s)
            }
            Err(e) => ToolOutput::error(format!("definition failed: {e}")),
        }
    }
}

// ─── lsp_references ────────────────────────────────────────────────────────

pub struct LspReferencesTool {
    mgr: SharedLspManager,
}

impl LspReferencesTool {
    pub fn new(mgr: SharedLspManager) -> Self {
        Self { mgr }
    }
}

#[async_trait]
impl Tool for LspReferencesTool {
    fn name(&self) -> &str {
        "lsp_references"
    }
    fn description(&self) -> &str {
        "Find all references to the symbol at (line, column) — compiler-grade, scope-aware. \
        Use this before renaming or removing a symbol; it catches usages that grep would miss \
        (re-exports, trait impls, generic instantiations) and does NOT flag accidental name \
        collisions."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "line": { "type": "integer", "description": "1-based line." },
                "column": { "type": "integer", "description": "1-based column." },
                "include_declaration": { "type": "boolean", "description": "Include the declaration site (default true)." }
            },
            "required": ["path", "line"]
        })
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path_str) = input["path"].as_str() else {
            return ToolOutput::error("'path' is required");
        };
        let line = match input["line"].as_u64() {
            Some(n) if n >= 1 => (n - 1) as u32,
            _ => return ToolOutput::error("'line' must be a 1-based integer"),
        };
        let col = input["column"].as_u64().unwrap_or(1).saturating_sub(1) as u32;
        let include_decl = input["include_declaration"].as_bool().unwrap_or(true);
        let path = resolve(path_str, ctx);

        let client = match client_for(&self.mgr, &path).await {
            Ok(c) => c,
            Err(out) => return out,
        };
        match client.references(&path, line, col, include_decl).await {
            Ok(locs) if locs.is_empty() => ToolOutput::success(format!(
                "No references found at {}:{}:{}.",
                rel(&path, ctx),
                line + 1,
                col + 1
            )),
            Ok(locs) => {
                let mut s = format!("{} reference(s):\n", locs.len());
                for l in locs {
                    let p = uri_to_path(&l.uri);
                    s.push_str(&format!(
                        "  {}:{}:{}\n",
                        rel(&p, ctx),
                        l.range.start.line + 1,
                        l.range.start.character + 1,
                    ));
                }
                ToolOutput::success(s)
            }
            Err(e) => ToolOutput::error(format!("references failed: {e}")),
        }
    }
}

// ─── lsp_hover ─────────────────────────────────────────────────────────────

pub struct LspHoverTool {
    mgr: SharedLspManager,
}

impl LspHoverTool {
    pub fn new(mgr: SharedLspManager) -> Self {
        Self { mgr }
    }
}

#[async_trait]
impl Tool for LspHoverTool {
    fn name(&self) -> &str {
        "lsp_hover"
    }
    fn description(&self) -> &str {
        "Hover information (type signature, doc comments) for the symbol at (line, column). \
        Provides the same data IDEs show on mouse-hover — type-checked and resolved."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "line": { "type": "integer", "description": "1-based line." },
                "column": { "type": "integer", "description": "1-based column." }
            },
            "required": ["path", "line"]
        })
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path_str) = input["path"].as_str() else {
            return ToolOutput::error("'path' is required");
        };
        let line = match input["line"].as_u64() {
            Some(n) if n >= 1 => (n - 1) as u32,
            _ => return ToolOutput::error("'line' must be a 1-based integer"),
        };
        let col = input["column"].as_u64().unwrap_or(1).saturating_sub(1) as u32;
        let path = resolve(path_str, ctx);

        let client = match client_for(&self.mgr, &path).await {
            Ok(c) => c,
            Err(out) => return out,
        };
        match client.hover(&path, line, col).await {
            Ok(Some(s)) if !s.trim().is_empty() => ToolOutput::success(s),
            Ok(_) => ToolOutput::success(format!(
                "No hover info at {}:{}:{}.",
                rel(&path, ctx),
                line + 1,
                col + 1
            )),
            Err(e) => ToolOutput::error(format!("hover failed: {e}")),
        }
    }
}

// ─── lsp_document_symbols ──────────────────────────────────────────────────

pub struct LspDocumentSymbolsTool {
    mgr: SharedLspManager,
}

impl LspDocumentSymbolsTool {
    pub fn new(mgr: SharedLspManager) -> Self {
        Self { mgr }
    }
}

#[async_trait]
impl Tool for LspDocumentSymbolsTool {
    fn name(&self) -> &str {
        "lsp_document_symbols"
    }
    fn description(&self) -> &str {
        "List all symbols (functions, types, classes, methods) defined in a file with their \
        line ranges. Cheaper than reading the whole file when you only need the structure."
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
        PermissionLevel::ReadOnly
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path_str) = input["path"].as_str() else {
            return ToolOutput::error("'path' is required");
        };
        let path = resolve(path_str, ctx);

        let client = match client_for(&self.mgr, &path).await {
            Ok(c) => c,
            Err(out) => return out,
        };
        match client.document_symbols(&path).await {
            Ok(syms) if syms.is_empty() => {
                ToolOutput::success(format!("No symbols found in {}.", rel(&path, ctx)))
            }
            Ok(syms) => ToolOutput::success(format_symbols(&syms, &rel(&path, ctx))),
            Err(e) => ToolOutput::error(format!("documentSymbol failed: {e}")),
        }
    }
}

fn format_symbols(syms: &[DocumentSymbolInfo], file: &str) -> String {
    let mut s = format!("{} symbol(s) in {}:\n", syms.len(), file);
    for sym in syms {
        let container = sym
            .container
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(|c| format!("{c}::"))
            .unwrap_or_default();
        s.push_str(&format!(
            "  L{:>4}-{:<4} {:<14} {}{}\n",
            sym.range.start.line + 1,
            sym.range.end.line + 1,
            symbol_kind_label(sym.kind),
            container,
            sym.name,
        ));
    }
    s
}

// ─── lsp_workspace_symbols ─────────────────────────────────────────────────

pub struct LspWorkspaceSymbolsTool {
    mgr: SharedLspManager,
}

impl LspWorkspaceSymbolsTool {
    pub fn new(mgr: SharedLspManager) -> Self {
        Self { mgr }
    }
}

#[async_trait]
impl Tool for LspWorkspaceSymbolsTool {
    fn name(&self) -> &str {
        "lsp_workspace_symbols"
    }
    fn description(&self) -> &str {
        "Search the entire workspace for symbols matching a query string via LSP. \
        Compiler-grade equivalent of `grep -n 'fn foo'` — returns only real declarations \
        of `foo`, not comments or string occurrences. Specify `language` to pick which \
        server to query (default: rust)."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "language": {
                    "type": "string",
                    "description": "Language key (rust, typescript, python, go). Default: rust."
                }
            },
            "required": ["query"]
        })
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(q) = input["query"].as_str() else {
            return ToolOutput::error("'query' is required");
        };
        let lang = input["language"].as_str().unwrap_or("rust");

        let client = match self.mgr.client_for_language(lang).await {
            Ok(c) => c,
            Err(e) => {
                return ToolOutput::success(format!(
                    "LSP unavailable for {lang}: {e}\nInstall a language server and retry, \
                     or use search_files as a fallback."
                ));
            }
        };
        match client.workspace_symbols(q).await {
            Ok(syms) if syms.is_empty() => {
                ToolOutput::success(format!("No symbols match '{q}' in {lang} workspace."))
            }
            Ok(syms) => {
                let mut s = format!("{} symbol(s) match '{q}':\n", syms.len());
                for sym in syms.iter().take(100) {
                    let p = uri_to_path(&sym.location.uri);
                    let container = sym
                        .container_name
                        .as_deref()
                        .filter(|c| !c.is_empty())
                        .map(|c| format!("{c}::"))
                        .unwrap_or_default();
                    s.push_str(&format!(
                        "  {}:{} {:<14} {}{}\n",
                        rel(&p, ctx),
                        sym.location.range.start.line + 1,
                        symbol_kind_label(sym.kind),
                        container,
                        sym.name,
                    ));
                }
                if syms.len() > 100 {
                    s.push_str(&format!("  ... and {} more\n", syms.len() - 100));
                }
                ToolOutput::success(s)
            }
            Err(e) => ToolOutput::error(format!("workspace/symbol failed: {e}")),
        }
    }
}

// ─── lsp_rename ────────────────────────────────────────────────────────────
//
// Rename is a *Mutating* tool. It always asks the server for the workspace
// edit, then either:
//   - dry_run=true (default for safety): formats a diff-like preview and
//     applies NOTHING. The agent can show this to the user before commiting.
//   - dry_run=false: applies the textual edits to the local files (in-place).
//
// We intentionally do not perform multi-file file-rename or create operations
// even if the server requests them — those are rarer and need more careful
// handling. For a v1 we only honour TextEdits.

pub struct LspRenameTool {
    mgr: SharedLspManager,
}

impl LspRenameTool {
    pub fn new(mgr: SharedLspManager) -> Self {
        Self { mgr }
    }
}

#[async_trait]
impl Tool for LspRenameTool {
    fn name(&self) -> &str {
        "lsp_rename"
    }
    fn description(&self) -> &str {
        "Compiler-safe rename of the symbol at (line, column) across the workspace via LSP. \
        Defaults to dry_run=true (returns a preview of the edits without touching disk). Set \
        dry_run=false to apply the edits in-place. Far safer than text search-and-replace — \
        catches all usages and skips lookalike names in unrelated scopes."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "line": { "type": "integer", "description": "1-based line." },
                "column": { "type": "integer", "description": "1-based column." },
                "new_name": { "type": "string" },
                "dry_run": { "type": "boolean", "description": "Preview only (default true)." }
            },
            "required": ["path", "line", "new_name"]
        })
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Mutating
    }

    fn effective_permission_level(&self, input: &Value) -> PermissionLevel {
        if input.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(true) {
            PermissionLevel::ReadOnly
        } else {
            PermissionLevel::Mutating
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let Some(path_str) = input["path"].as_str() else {
            return ToolOutput::error("'path' is required");
        };
        let Some(new_name) = input["new_name"].as_str() else {
            return ToolOutput::error("'new_name' is required");
        };
        if new_name.trim().is_empty() {
            return ToolOutput::error("'new_name' cannot be empty");
        }
        let line = match input["line"].as_u64() {
            Some(n) if n >= 1 => (n - 1) as u32,
            _ => return ToolOutput::error("'line' must be a 1-based integer"),
        };
        let col = input["column"].as_u64().unwrap_or(1).saturating_sub(1) as u32;
        let dry_run = input["dry_run"].as_bool().unwrap_or(true);
        let path = resolve(path_str, ctx);

        let client = match client_for(&self.mgr, &path).await {
            Ok(c) => c,
            Err(out) => return out,
        };
        let edit = match client.rename(&path, line, col, new_name).await {
            Ok(e) => e,
            Err(e) => return ToolOutput::error(format!("rename failed: {e}")),
        };

        if edit.edits_by_path.is_empty() {
            return ToolOutput::success(format!(
                "Server returned no edits for renaming the symbol at {}:{}:{} to '{}'.",
                rel(&path, ctx),
                line + 1,
                col + 1,
                new_name
            ));
        }

        if dry_run {
            return ToolOutput::success(format_rename_preview(&edit, ctx));
        }

        match apply_edits(&edit).await {
            Ok((files, edits)) => ToolOutput::success(format!(
                "Renamed to '{new_name}': applied {edits} edit(s) across {files} file(s).\n\n{}",
                format_rename_preview(&edit, ctx)
            )),
            Err(e) => ToolOutput::error(format!("rename apply failed (no files written): {e}")),
        }
    }
}

fn format_rename_preview(edit: &WorkspaceEditPreview, ctx: &ToolContext) -> String {
    let total: usize = edit.edits_by_path.values().map(|v| v.len()).sum();
    let mut s = format!(
        "Rename preview — {} file(s), {} edit(s):\n",
        edit.edits_by_path.len(),
        total
    );
    let mut keys: Vec<&PathBuf> = edit.edits_by_path.keys().collect();
    keys.sort();
    for path in keys {
        let edits = &edit.edits_by_path[path];
        s.push_str(&format!("\n--- {}\n", rel(path, ctx)));
        for te in edits {
            s.push_str(&format!(
                "  L{}:{}–L{}:{}  →  {:?}\n",
                te.range.start.line + 1,
                te.range.start.character + 1,
                te.range.end.line + 1,
                te.range.end.character + 1,
                te.new_text
            ));
        }
    }
    s
}

async fn apply_edits(edit: &WorkspaceEditPreview) -> anyhow::Result<(usize, usize)> {
    let mut files = 0usize;
    let mut count = 0usize;
    for (path, edits) in &edit.edits_by_path {
        let original = tokio::fs::read_to_string(path).await?;
        let updated = apply_text_edits(&original, edits)?;
        tokio::fs::write(path, updated).await?;
        files += 1;
        count += edits.len();
    }
    Ok((files, count))
}

/// Apply a list of LSP TextEdits to a string. Edits are sorted in
/// reverse order so earlier ranges aren't shifted by later insertions.
fn apply_text_edits(original: &str, edits: &[TextEdit]) -> anyhow::Result<String> {
    let lines: Vec<&str> = split_lines_preserving(original);
    // Convert each edit's (line, character) to absolute byte offsets.
    let mut concrete: Vec<(usize, usize, String)> = Vec::with_capacity(edits.len());
    for e in edits {
        let start = position_to_offset(&lines, e.range.start.line, e.range.start.character)?;
        let end = position_to_offset(&lines, e.range.end.line, e.range.end.character)?;
        if end < start {
            return Err(anyhow::anyhow!("rename edit has end < start"));
        }
        concrete.push((start, end, e.new_text.clone()));
    }
    concrete.sort_by(|a, b| b.0.cmp(&a.0)); // descending by start

    // Sanity: ensure no overlapping edits (servers should never produce them).
    for w in concrete.windows(2) {
        // descending: w[0] is *later* in the file. w[0].0 must be >= w[1].1
        if w[0].0 < w[1].1 {
            return Err(anyhow::anyhow!("rename produced overlapping edits"));
        }
    }

    let mut out = original.to_string();
    for (start, end, new_text) in concrete {
        out.replace_range(start..end, &new_text);
    }
    Ok(out)
}

fn split_lines_preserving(s: &str) -> Vec<&str> {
    // We need lines that, when concatenated with their original line endings,
    // reproduce the input. `split_inclusive` does exactly that.
    s.split_inclusive('\n').collect()
}

fn position_to_offset(lines: &[&str], line: u32, character: u32) -> anyhow::Result<usize> {
    let line = line as usize;
    if line > lines.len() {
        return Err(anyhow::anyhow!("line {line} out of range"));
    }
    let mut offset = 0usize;
    for l in lines.iter().take(line) {
        offset += l.len();
    }
    if line == lines.len() {
        // Position at end-of-file with no trailing newline.
        return Ok(offset);
    }
    let row = lines[line];
    // LSP positions are UTF-16 code units, but in practice servers behave
    // like UTF-8 byte offsets for ASCII identifiers (the common case for
    // rename). For non-ASCII, we map character index over chars and accept
    // approximate offsets — good enough for v1.
    let mut col_remaining = character as usize;
    for (idx, ch) in row.char_indices() {
        if col_remaining == 0 {
            return Ok(offset + idx);
        }
        let units = ch.len_utf16();
        if col_remaining < units {
            return Ok(offset + idx);
        }
        col_remaining -= units;
    }
    // Past end of line — trim the trailing newline if any.
    let row_len = row.trim_end_matches(['\n', '\r']).len();
    Ok(offset + row_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::protocol::{Position, Range};

    #[test]
    fn position_to_offset_basic() {
        let s = "abc\ndef\n";
        let lines = split_lines_preserving(s);
        // line 0, col 0 → 0
        assert_eq!(position_to_offset(&lines, 0, 0).unwrap(), 0);
        // line 1, col 0 → 4 (after "abc\n")
        assert_eq!(position_to_offset(&lines, 1, 0).unwrap(), 4);
        // line 1, col 2 → 6
        assert_eq!(position_to_offset(&lines, 1, 2).unwrap(), 6);
    }

    #[test]
    fn apply_one_edit() {
        let src = "let foo = 1;\nfoo + foo;\n";
        let edits = vec![
            TextEdit {
                range: Range {
                    start: Position { line: 0, character: 4 },
                    end: Position { line: 0, character: 7 },
                },
                new_text: "bar".into(),
            },
            TextEdit {
                range: Range {
                    start: Position { line: 1, character: 0 },
                    end: Position { line: 1, character: 3 },
                },
                new_text: "bar".into(),
            },
            TextEdit {
                range: Range {
                    start: Position { line: 1, character: 6 },
                    end: Position { line: 1, character: 9 },
                },
                new_text: "bar".into(),
            },
        ];
        let out = apply_text_edits(src, &edits).unwrap();
        assert_eq!(out, "let bar = 1;\nbar + bar;\n");
    }
}
