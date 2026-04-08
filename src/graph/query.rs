/// Retrieval API for the semantic code graph.
///
/// All query methods run in O(1) or O(k) where k is the subgraph/result size.
/// They are designed to be called from both the GraphQueryTool (LLM-facing) and
/// from the system prompt builder.
use petgraph::stable_graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::graph::types::*;
use crate::graph::CodeGraph;

// ---------------------------------------------------------------------------
// PackedContext — token-budget-aware context assembly
// ---------------------------------------------------------------------------

pub struct ContextEntry<'a> {
    pub node:      &'a GraphNode,
    pub view:      ContextView,
    pub edge_type: EdgeType,
}

pub enum ContextView {
    FullSnippet,
    SignatureOnly,
}

pub struct PackedContext<'a> {
    pub primary:      &'a GraphNode,
    pub dependencies: Vec<ContextEntry<'a>>,
    pub total_tokens: usize,
    pub was_pruned:   bool,
}

// ---------------------------------------------------------------------------
// Query handle
// ---------------------------------------------------------------------------

pub struct GraphQuery<'a> {
    pub g: &'a CodeGraph,
}

impl<'a> GraphQuery<'a> {
    pub fn new(g: &'a CodeGraph) -> Self { Self { g } }

    // ── Lookup ───────────────────────────────────────────────────────────────

    /// Find a node by exact FQDN.
    pub fn by_fqdn(&self, fqdn: &str) -> Option<&GraphNode> {
        self.g.fqdn_index.get(fqdn)
            .and_then(|&idx| self.g.graph.node_weight(idx))
    }

    /// Find all nodes with this short name.
    pub fn by_name(&self, name: &str) -> Vec<&GraphNode> {
        self.g.name_index.get(name)
            .map(|idxs| idxs.iter()
                .filter_map(|&i| self.g.graph.node_weight(i))
                .collect())
            .unwrap_or_default()
    }

    /// All non-file nodes in a file.
    pub fn nodes_in_file(&self, file: &str) -> Vec<&GraphNode> {
        self.g.file_index.get(file)
            .map(|idxs| idxs.iter()
                .filter_map(|&i| self.g.graph.node_weight(i))
                .filter(|n| n.kind != NodeKind::File)
                .collect())
            .unwrap_or_default()
    }

    /// Direct outgoing neighbours of a node (things it calls/imports/etc).
    pub fn direct_deps(&self, idx: NodeIndex) -> Vec<(EdgeType, &GraphNode)> {
        self.g.graph.edges(idx)
            .filter_map(|e| {
                let node = self.g.graph.node_weight(e.target())?;
                Some((e.weight().clone(), node))
            })
            .collect()
    }

    /// All nodes that transitively *depend on* the given node (reverse BFS).
    /// This is the "blast radius" — useful before editing to see what might break.
    pub fn blast_radius(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        let mut visited = std::collections::HashSet::new();
        let mut queue   = std::collections::VecDeque::new();
        queue.push_back(idx);
        while let Some(current) = queue.pop_front() {
            if !visited.insert(current) { continue; }
            for e in self.g.graph.edges_directed(current, Direction::Incoming) {
                queue.push_back(e.source());
            }
        }
        visited.into_iter().filter(|&i| i != idx).collect()
    }

    /// Find all callers of a node (direct incoming Calls edges).
    pub fn callers(&self, idx: NodeIndex) -> Vec<&GraphNode> {
        self.g.graph.edges_directed(idx, Direction::Incoming)
            .filter(|e| *e.weight() == EdgeType::Calls)
            .filter_map(|e| self.g.graph.node_weight(e.source()))
            .collect()
    }

    /// Find all sources of MutatesState edges pointing to a target.
    pub fn mutation_sources(&self, idx: NodeIndex) -> Vec<&GraphNode> {
        self.g.graph.edges_directed(idx, Direction::Incoming)
            .filter(|e| *e.weight() == EdgeType::MutatesState)
            .filter_map(|e| self.g.graph.node_weight(e.source()))
            .collect()
    }

    /// Case-insensitive name search across all nodes.
    pub fn fuzzy_search(&self, query: &str, limit: usize) -> Vec<(&GraphNode, f32)> {
        let q = query.to_lowercase();
        let mut results: Vec<(&GraphNode, f32)> = Vec::new();

        for idx in self.g.graph.node_indices() {
            if let Some(node) = self.g.graph.node_weight(idx) {
                if node.kind == NodeKind::File { continue; }
                let name_lc = node.name.to_lowercase();
                if name_lc.contains(&q) {
                    // Score: exact match > starts_with > contains
                    let score = if name_lc == q { 1.0 }
                        else if name_lc.starts_with(&q) { 0.7 }
                        else { 0.4 };
                    results.push((node, score));
                }
            }
        }
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    // ── Context packer ───────────────────────────────────────────────────────

    /// Assemble token-budget-aware context for LLM injection.
    ///
    /// Algorithm:
    ///   1. Load primary node full_snippet.
    ///   2. BFS outward collecting 1st-degree deps (Calls, Imports, Implements).
    ///   3. Greedily pack within `token_budget`; degrade to signature_only when needed.
    pub fn context_pack(&'a self, fqdn: &str, token_budget: usize) -> Option<PackedContext<'a>> {
        let &primary_idx = self.g.fqdn_index.get(fqdn)?;
        let primary      = self.g.graph.node_weight(primary_idx)?;

        let mut total    = primary.content.token_weight;
        let mut deps     = Vec::new();
        let mut pruned   = false;

        // Collect 1st-degree outgoing deps, sorted by edge importance
        let mut edges: Vec<(EdgeType, NodeIndex)> = self.g.graph.edges(primary_idx)
            .map(|e| (e.weight().clone(), e.target()))
            .collect();

        // Sort by importance: MutatesState > Calls > Implements > rest
        edges.sort_by_key(|(et, _)| match et {
            EdgeType::MutatesState  => 0,
            EdgeType::Calls         => 1,
            EdgeType::Implements    => 2,
            EdgeType::Imports       => 3,
            _                       => 4,
        });

        for (et, dep_idx) in edges {
            if let Some(dep) = self.g.graph.node_weight(dep_idx) {
                if dep.kind == NodeKind::File || dep.kind == NodeKind::ExternalStub { continue; }
                if total + dep.content.token_weight <= token_budget {
                    total += dep.content.token_weight;
                    deps.push(ContextEntry { node: dep, view: ContextView::FullSnippet, edge_type: et });
                } else if total + dep.content.signature_only.len() / 4 + 1 <= token_budget {
                    let sig_weight = dep.content.signature_only.len() / 4 + 1;
                    total += sig_weight;
                    deps.push(ContextEntry { node: dep, view: ContextView::SignatureOnly, edge_type: et });
                    pruned = true;
                } else {
                    pruned = true;
                }
            }
        }

        Some(PackedContext { primary, dependencies: deps, total_tokens: total, was_pruned: pruned })
    }

    // ── Formatting helpers (for tool output) ─────────────────────────────────

    /// Format a PackedContext as markdown ready for LLM injection.
    pub fn format_context(pc: &PackedContext) -> String {
        let mut out = String::new();
        let n = pc.primary;
        out.push_str(&format!(
            "## [{kind}] {fqdn}\nFile: {file}  Lines: {start}–{end}\nModifiers: {mods}\n\n```\n{code}\n```\n",
            kind  = n.kind.label(),
            fqdn  = n.fqdn,
            file  = n.file_path,
            start = n.span.start_line + 1,
            end   = n.span.end_line + 1,
            mods  = n.modifiers.describe(),
            code  = n.content.full_snippet,
        ));
        if !pc.dependencies.is_empty() {
            out.push_str("\n### Dependencies\n");
            for dep in &pc.dependencies {
                let code = match dep.view {
                    ContextView::FullSnippet  => &dep.node.content.full_snippet,
                    ContextView::SignatureOnly => &dep.node.content.signature_only,
                };
                out.push_str(&format!(
                    "\n#### [{rel}] [{kind}] {fqdn}\n```\n{code}\n```\n",
                    rel  = format!("{:?}", dep.edge_type).to_uppercase(),
                    kind = dep.node.kind.label(),
                    fqdn = dep.node.fqdn,
                    code = code,
                ));
            }
            if pc.was_pruned {
                out.push_str("\n*(some dependencies degraded to signature-only to stay within token budget)*\n");
            }
        }
        out.push_str(&format!("\nTotal tokens (approx): {}", pc.total_tokens));
        out
    }

    /// Format blast-radius result as markdown.
    pub fn format_blast_radius(&self, target_fqdn: &str, indices: &[NodeIndex]) -> String {
        let mut out = format!("## Blast Radius for `{target_fqdn}`\n");
        out.push_str(&format!("{} nodes depend on this symbol:\n\n", indices.len()));
        for &idx in indices.iter().take(50) {
            if let Some(n) = self.g.graph.node_weight(idx) {
                out.push_str(&format!("- [{kind}] `{fqdn}`\n",
                    kind = n.kind.label(), fqdn = n.fqdn));
            }
        }
        if indices.len() > 50 {
            out.push_str(&format!("\n... and {} more\n", indices.len() - 50));
        }
        out
    }

    /// Format fuzzy search results.
    pub fn format_search(&self, results: &[(&GraphNode, f32)]) -> String {
        if results.is_empty() {
            return "No matches found.".to_string();
        }
        let mut out = format!("Found {} match(es):\n\n", results.len());
        for (node, _) in results {
            out.push_str(&format!(
                "- [{kind}] `{fqdn}`  ({file}:{line})\n  {sig}\n",
                kind = node.kind.label(),
                fqdn = node.fqdn,
                file = node.file_path,
                line = node.span.start_line + 1,
                sig  = node.content.signature_only,
            ));
        }
        out
    }

    /// Format file graph (all symbols in a file).
    pub fn format_file_graph(&self, file: &str, nodes: &[&GraphNode]) -> String {
        if nodes.is_empty() {
            return format!("No symbols found in `{file}`.");
        }
        let mut out = format!("## Symbols in `{file}` ({} total)\n\n", nodes.len());
        for node in nodes {
            let mods = node.modifiers.describe();
            let mods_str = if mods.is_empty() { String::new() } else { format!(" [{mods}]") };
            out.push_str(&format!(
                "- [{kind}]{mods} `{name}` (line {line})\n",
                kind  = node.kind.label(),
                mods  = mods_str,
                name  = node.name,
                line  = node.span.start_line + 1,
            ));
        }
        out
    }
}
