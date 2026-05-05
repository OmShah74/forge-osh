/// LLM-facing `graph_query` tool.
///
/// This tool is always registered in the ToolRegistry; it returns a helpful
/// "no graph loaded" message if the user hasn't run `/forge-graph` yet.
use async_trait::async_trait;
use serde_json::json;

use crate::graph::query::GraphQuery;
use crate::graph::SharedGraph;
use crate::lsp::SharedLspManager;
use crate::tools::Tool;
use crate::types::{PermissionLevel, ToolContext, ToolOutput};

pub struct GraphQueryTool {
    graph: SharedGraph,
    lsp: Option<SharedLspManager>,
}

impl GraphQueryTool {
    pub fn new(graph: SharedGraph) -> Self {
        Self { graph, lsp: None }
    }

    pub fn new_with_lsp(graph: SharedGraph, lsp: SharedLspManager) -> Self {
        Self {
            graph,
            lsp: Some(lsp),
        }
    }

    fn with_lsp_overlay(
        &self,
        mut output: String,
        file_path: Option<&str>,
        ctx: &ToolContext,
    ) -> String {
        let (Some(lsp), Some(file_path)) = (&self.lsp, file_path) else {
            return output;
        };
        let path = std::path::Path::new(file_path);
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            ctx.working_dir.join(path)
        };
        if let Some(language) = lsp.language_for_path(&abs) {
            output.push_str(&format!(
                "\n\nLSP overlay: `{language}` is configured for this file. Use `lsp_document_symbols`, `lsp_definition`, `lsp_references`, or `lsp_diagnostics` when live compiler-grade precision is needed."
            ));
        }
        output
    }
}

#[async_trait]
impl Tool for GraphQueryTool {
    fn name(&self) -> &str {
        "graph_query"
    }

    fn description(&self) -> &str {
        "Query the semantic code graph built by /forge-graph. Provides deterministic, \
        token-efficient codebase navigation without reading files. \
        Operations: find (search by name), context_pack (full context for a symbol), \
        blast_radius (what depends on a symbol), file_graph (all symbols in a file), \
        callers (find all direct callers of a function/method), \
        mutations (all mutation points of a variable), stats (graph statistics)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["find", "context_pack", "blast_radius", "file_graph", "callers", "mutations", "stats"],
                    "description": "The query operation to perform."
                },
                "target": {
                    "type": "string",
                    "description": "The symbol name, FQDN (e.g. 'src/agent/loop.rs::AgentLoop::run'), or file path for the operation."
                },
                "token_budget": {
                    "type": "integer",
                    "description": "Max tokens for context_pack (default 8000).",
                    "default": 8000
                }
            },
            "required": ["operation"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolOutput {
        // Check if graph is loaded
        let guard = match self.graph.read() {
            Ok(g) => g,
            Err(_) => return ToolOutput::error("Graph lock poisoned — restart forge-osh."),
        };

        let Some(ref cg) = *guard else {
            return ToolOutput::success(
                "No forge-graph loaded for this project.\n\
                Run `/forge-graph` in the TUI to build the semantic code graph.\n\
                Once built, this tool provides O(1) lookups for any symbol without reading files.",
            );
        };

        let q = GraphQuery::new(cg);
        let op = input["operation"].as_str().unwrap_or("stats");

        match op {
            // ── find ─────────────────────────────────────────────────────
            "find" => {
                let target = match input["target"].as_str() {
                    Some(t) => t,
                    None    => return ToolOutput::error("'target' is required for 'find'"),
                };
                let results = q.fuzzy_search(target, 20);
                ToolOutput::success(q.format_search(&results))
            }

            // ── context_pack ──────────────────────────────────────────────
            "context_pack" => {
                let target = match input["target"].as_str() {
                    Some(t) => t,
                    None    => return ToolOutput::error("'target' is required for 'context_pack'"),
                };
                let budget = input["token_budget"].as_u64().unwrap_or(8000) as usize;

                // Try exact FQDN first, then fuzzy
                let fqdn = if cg.fqdn_index.contains_key(target) {
                    target.to_string()
                } else {
                    let hits = q.fuzzy_search(target, 1);
                    match hits.first() {
                        Some((n, _)) => n.fqdn.clone(),
                        None => return ToolOutput::success(format!("No symbol found matching '{target}'.")),
                    }
                };

                match q.context_pack(&fqdn, budget) {
                    Some(pc) => ToolOutput::success(self.with_lsp_overlay(
                        GraphQuery::format_context(&pc),
                        Some(&pc.primary.file_path),
                        ctx,
                    )),
                    None     => ToolOutput::success(format!("Symbol '{fqdn}' not found in graph.")),
                }
            }

            // ── blast_radius ──────────────────────────────────────────────
            "blast_radius" => {
                let target = match input["target"].as_str() {
                    Some(t) => t,
                    None    => return ToolOutput::error("'target' is required for 'blast_radius'"),
                };

                let fqdn = if cg.fqdn_index.contains_key(target) {
                    target.to_string()
                } else {
                    let hits = q.fuzzy_search(target, 1);
                    match hits.first() {
                        Some((n, _)) => n.fqdn.clone(),
                        None => return ToolOutput::success(format!("No symbol found matching '{target}'.")),
                    }
                };

                if let Some(&idx) = cg.fqdn_index.get(&fqdn) {
                    let radius = q.blast_radius(idx);
                    ToolOutput::success(q.format_blast_radius(&fqdn, &radius))
                } else {
                    ToolOutput::success(format!("Symbol '{fqdn}' not found in graph."))
                }
            }

            // ── file_graph ────────────────────────────────────────────────
            "file_graph" => {
                let target = match input["target"].as_str() {
                    Some(t) => t,
                    None    => return ToolOutput::error("'target' (file path) is required for 'file_graph'"),
                };

                // Fuzzy-match file path
                let file_key = if cg.file_index.contains_key(target) {
                    target.to_string()
                } else {
                    cg.file_index.keys()
                        .find(|k| k.contains(target))
                        .cloned()
                        .unwrap_or_else(|| target.to_string())
                };

                let nodes = q.nodes_in_file(&file_key);
                ToolOutput::success(self.with_lsp_overlay(
                    q.format_file_graph(&file_key, &nodes),
                    Some(&file_key),
                    ctx,
                ))
            }

            // ── callers ───────────────────────────────────────────────────
            "callers" => {
                let target = match input["target"].as_str() {
                    Some(t) => t,
                    None    => return ToolOutput::error("'target' is required for 'callers'"),
                };

                let fqdn = if cg.fqdn_index.contains_key(target) {
                    target.to_string()
                } else {
                    let hits = q.fuzzy_search(target, 1);
                    match hits.first() {
                        Some((n, _)) => n.fqdn.clone(),
                        None => return ToolOutput::success(format!("No symbol found matching '{target}'.")),
                    }
                };

                if let Some(&idx) = cg.fqdn_index.get(&fqdn) {
                    let callers = q.callers(idx);
                    if callers.is_empty() {
                        ToolOutput::success(format!("No callers found for `{fqdn}`. \
                            Either it is never called, or the call pattern wasn't captured during graph build."))
                    } else {
                        let mut out = format!("## Callers of `{fqdn}` ({} total)\n\n", callers.len());
                        for node in &callers {
                            out.push_str(&format!("- [{kind}] `{fqdn}` ({file}:{line})\n  {sig}\n",
                                kind = node.kind.label(), fqdn = node.fqdn,
                                file = node.file_path, line = node.span.start_line + 1,
                                sig = node.content.signature_only));
                        }
                        ToolOutput::success(out)
                    }
                } else {
                    ToolOutput::success(format!("Symbol '{fqdn}' not found in graph."))
                }
            }

            // ── mutations ─────────────────────────────────────────────────
            "mutations" => {
                let target = match input["target"].as_str() {
                    Some(t) => t,
                    None    => return ToolOutput::error("'target' is required for 'mutations'"),
                };

                let fqdn = if cg.fqdn_index.contains_key(target) {
                    target.to_string()
                } else {
                    let hits = q.fuzzy_search(target, 1);
                    match hits.first() {
                        Some((n, _)) => n.fqdn.clone(),
                        None => return ToolOutput::success(format!("No symbol found matching '{target}'.")),
                    }
                };

                if let Some(&idx) = cg.fqdn_index.get(&fqdn) {
                    let sources = q.mutation_sources(idx);
                    if sources.is_empty() {
                        ToolOutput::success(format!("No MutatesState edges found for `{fqdn}`. \
                            Either it is never mutated, or this pattern wasn't captured during graph build."))
                    } else {
                        let mut out = format!("## Mutation sources for `{fqdn}`\n\n");
                        for node in &sources {
                            out.push_str(&format!("- [{kind}] `{fqdn}` ({file}:{line})\n",
                                kind = node.kind.label(), fqdn = node.fqdn,
                                file = node.file_path, line = node.span.start_line + 1));
                        }
                        ToolOutput::success(out)
                    }
                } else {
                    ToolOutput::success(format!("Symbol '{fqdn}' not found in graph."))
                }
            }

            // ── stats ─────────────────────────────────────────────────────
            "stats" => {
                let mut kind_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                for idx in cg.graph.node_indices() {
                    if let Some(n) = cg.graph.node_weight(idx) {
                        *kind_counts.entry(n.kind.label().to_string()).or_insert(0) += 1;
                    }
                }
                let mut kind_lines: Vec<String> = kind_counts.iter()
                    .map(|(k, v)| format!("  {k:<12} {v}"))
                    .collect();
                kind_lines.sort();

                let mut lang_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                for idx in cg.graph.node_indices() {
                    if let Some(n) = cg.graph.node_weight(idx) {
                        if n.kind != crate::graph::types::NodeKind::File {
                            *lang_counts.entry(n.language.name().to_string()).or_insert(0) += 1;
                        }
                    }
                }
                let mut lang_lines: Vec<String> = lang_counts.iter()
                    .map(|(k, v)| format!("  {k:<15} {v}"))
                    .collect();
                lang_lines.sort();

                let out = format!(
                    "## forge-graph Statistics\n\
                    Root:       {root}\n\
                    Built:      {age}\n\
                    Files:      {files}\n\
                    Nodes:      {nodes}\n\
                    Edges:      {edges}\n\
                    \n\
                    By kind:\n{kinds}\n\
                    \n\
                    By language:\n{langs}",
                    root  = cg.meta.root_path,
                    age   = cg.meta.age_description(),
                    files = cg.meta.file_count,
                    nodes = cg.meta.total_nodes,
                    edges = cg.meta.total_edges,
                    kinds = kind_lines.join("\n"),
                    langs = lang_lines.join("\n"),
                );
                ToolOutput::success(out)
            }

            _ => ToolOutput::error(format!(
                "Unknown operation '{op}'. Valid: find, context_pack, blast_radius, file_graph, callers, mutations, stats"
            )),
        }
    }
}
