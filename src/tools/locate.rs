//! `locate` — one-shot code localization meta-tool.
//!
//! This is the orchestration layer the discovery audit called for: instead of
//! making the model hand-drive the grep→read funnel, `locate` runs the cheap
//! precise layers in the right order and returns a single, ranked candidate set
//! of files (and symbols, when a semantic graph is loaded) for a natural query.
//!
//! Funnel:
//!   1. Semantic graph fuzzy symbol search (if `/forge-graph` was built) — high
//!      confidence, points at exact definitions.
//!   2. Ranked parallel text search (the shared [`crate::tools::search`] core) —
//!      always available, finds definitions and usages.
//!   3. Merge + rank: graph symbol hits boost their file; text scores add in.
//!
//! Output is compact and decision-oriented: the model gets "here are the most
//! likely files, why, and the key symbol/line" so it can jump straight to
//! `read_file` / `graph_query context_pack` instead of guessing.

use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::search::{extract_term, run_search, smart_case_sensitive, SearchParams};
use super::Tool;
use crate::graph::query::GraphQuery;
use crate::graph::SharedGraph;
use crate::types::{PermissionLevel, ToolContext, ToolOutput};

pub struct LocateTool {
    graph: SharedGraph,
}

impl LocateTool {
    pub fn new(graph: SharedGraph) -> Self {
        Self { graph }
    }
}

/// One merged candidate file.
struct Candidate {
    rel_path: String,
    text_score: f64,
    match_count: usize,
    /// Best symbol the graph associated with this file (fqdn, kind, line).
    symbol: Option<(String, String, u32)>,
    graph_score: f64,
}

impl Candidate {
    fn total(&self) -> f64 {
        // Graph hits are strong evidence of "the place"; weight them heavily.
        self.text_score + self.graph_score * 6.0
    }
}

#[async_trait]
impl Tool for LocateTool {
    fn name(&self) -> &str {
        "locate"
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "Find WHERE something lives in the codebase in one shot. Give a symbol name or a short \
         phrase (e.g. \"AgentLoop\", \"permission prompt\", \"retry backoff\") and `locate` runs the \
         semantic graph (if built) plus a ranked parallel text search and returns the most likely \
         files — with the key symbol/line and why each ranked — so you can go straight to read_file \
         or graph_query. Prefer this as your FIRST step when you don't yet know which file to open. \
         Params: `query` (required), `path` (optional subtree), `limit` (default 10), \
         `type_filter` (optional language shorthand like 'rs','ts','py')."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Symbol name or short natural-language description of what to find."
                },
                "path": {
                    "type": "string",
                    "description": "Restrict the search to this directory subtree (default: working dir)."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max candidate files to return (default 10)."
                },
                "type_filter": {
                    "type": "string",
                    "description": "Language shorthand to restrict file types (e.g. 'rs','ts','py')."
                }
            },
            "required": ["query"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let query = match input["query"].as_str() {
            Some(q) if !q.trim().is_empty() => q.trim().to_string(),
            _ => return ToolOutput::error("locate requires a non-empty 'query'."),
        };
        let limit = input["limit"].as_u64().unwrap_or(10).clamp(1, 50) as usize;
        let type_filter = input["type_filter"].as_str().map(|s| s.to_string());

        let search_path = match input["path"].as_str() {
            Some(p) => {
                let path = std::path::Path::new(p);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    ctx.working_dir.join(path)
                }
            }
            None => ctx.working_dir.clone(),
        };
        if !search_path.exists() {
            return ToolOutput::error(format!("Path not found: {}", search_path.display()));
        }

        let term = extract_term(&query);

        // ── 1. Graph symbol hits (best-effort; empty if no graph) ────────────
        // Collect into owned tuples so we don't hold the lock across await.
        let graph_hits: Vec<(String, String, String, u32, f32)> = {
            match self.graph.read() {
                Ok(guard) => match guard.as_ref() {
                    Some(cg) => {
                        let q = GraphQuery::new(cg);
                        let needle = if term.is_empty() { query.as_str() } else { term.as_str() };
                        q.fuzzy_search(needle, 12)
                            .into_iter()
                            .map(|(n, score)| {
                                (
                                    n.file_path.replace('\\', "/"),
                                    n.fqdn.clone(),
                                    n.kind.label().to_string(),
                                    n.span.start_line + 1,
                                    score,
                                )
                            })
                            .collect()
                    }
                    None => Vec::new(),
                },
                Err(_) => Vec::new(),
            }
        };

        // ── 2. Ranked text search (always available) ─────────────────────────
        // Use the query as a literal so phrases and punctuation are safe.
        let case_sensitive = smart_case_sensitive(&query);
        let regex = match regex::RegexBuilder::new(&regex::escape(&query))
            .case_insensitive(!case_sensitive)
            .build()
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("Could not build search: {e}")),
        };
        let type_extensions = type_filter
            .as_deref()
            .map(super::search::type_to_extensions)
            .unwrap_or_default();

        let params = SearchParams {
            regex,
            pattern_display: query.clone(),
            term: term.clone(),
            multiline: false,
            search_path,
            working_dir: ctx.working_dir.clone(),
            glob_pattern: None,
            exclude_glob: None,
            type_extensions,
            include_hidden: false,
            include_ignored: false,
            // Cast a wide net; we rank and trim ourselves.
            max_results: 2_000,
            max_files: 20_000,
            max_file_bytes: 1_000_000,
            before_ctx: 0,
            after_ctx: 0,
        };
        let outcome = match tokio::task::spawn_blocking(move || run_search(&params)).await {
            Ok(o) => o,
            Err(e) => return ToolOutput::error(format!("locate search task failed: {e}")),
        };

        // ── 3. Merge + rank ──────────────────────────────────────────────────
        let mut by_file: HashMap<String, Candidate> = HashMap::new();

        for f in &outcome.files {
            by_file.insert(
                f.rel_path.clone(),
                Candidate {
                    rel_path: f.rel_path.clone(),
                    text_score: f.score,
                    match_count: f.match_count,
                    symbol: None,
                    graph_score: 0.0,
                },
            );
        }

        for (file, fqdn, kind, line, score) in graph_hits {
            let entry = by_file.entry(file.clone()).or_insert_with(|| Candidate {
                rel_path: file.clone(),
                text_score: 0.0,
                match_count: 0,
                symbol: None,
                graph_score: 0.0,
            });
            // Keep the strongest symbol per file.
            if score as f64 >= entry.graph_score {
                entry.graph_score = score as f64;
                entry.symbol = Some((fqdn, kind, line));
            }
        }

        let mut candidates: Vec<Candidate> = by_file.into_values().collect();
        candidates.sort_by(|a, b| {
            b.total()
                .partial_cmp(&a.total())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.rel_path.cmp(&b.rel_path))
        });

        if candidates.is_empty() {
            return ToolOutput::success(format!(
                "locate found nothing for '{query}' ({} file(s) scanned). Try a different term, a \
                 broader phrase, or search_files with a regex.",
                outcome.files_scanned
            ));
        }

        let shown = candidates.len().min(limit);
        let graph_loaded = self
            .graph
            .read()
            .ok()
            .map(|g| g.is_some())
            .unwrap_or(false);

        let mut out = format!(
            "Top {shown} candidate(s) for '{query}' (ranked; {} file(s) scanned):\n\n",
            outcome.files_scanned
        );
        for (i, c) in candidates.iter().take(shown).enumerate() {
            let mut why: Vec<String> = Vec::new();
            if let Some((fqdn, kind, line)) = &c.symbol {
                why.push(format!("graph: {kind} `{fqdn}` @ line {line}"));
            }
            if c.match_count > 0 {
                why.push(format!("{} text match(es)", c.match_count));
            }
            if why.is_empty() {
                why.push("name/path match".to_string());
            }
            out.push_str(&format!(
                "{}. {}  —  {}\n",
                i + 1,
                c.rel_path,
                why.join("; ")
            ));
        }

        out.push_str("\nNext: read_file the top candidate");
        if graph_loaded {
            out.push_str(", or graph_query context_pack on the named symbol");
        }
        out.push('.');

        ToolOutput::success(out)
    }
}
