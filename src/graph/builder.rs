/// Two-pass parallel graph builder.
///
/// Pass 1 (parallel): parse every source file → collect definitions + imports + calls.
/// Pass 2 (sequential): insert nodes, then resolve edges between them.
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::graph::parser::{parse_file, ParsedDef, ParsedFile};
use crate::graph::types::*;
use crate::graph::{CodeGraph, GraphBuildMsg, GRAPH_VERSION};

use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// File collection
// ---------------------------------------------------------------------------

/// Extensions we bother to parse.
const PARSEABLE_EXTS: &[&str] = &[
    // Rust
    "rs", // Python
    "py", "pyw", "pyi", // JavaScript / TypeScript
    "js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts", // Go
    "go",  // C / C++
    "c", "h", "cpp", "cc", "cxx", "hpp", "hxx", "h++",  // Java
    "java", // C#
    "cs",   // Ruby
    "rb", "rake", "gemspec", // Kotlin
    "kt", "kts",   // Swift
    "swift", // Bash / shell
    "sh", "bash", "zsh", "fish", // PHP
    "php", "php3", "php4", "php5", "phtml", // Lua
    "lua",   // Scala
    "scala", "sc",
];

/// Directories we skip unconditionally.
const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".mypy_cache",
    "vendor",
    ".cargo",
];

fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .build();

    walker
        .filter_map(|e| e.ok())
        .filter(|e| {
            // Skip known noise directories
            for comp in e.path().components() {
                if let std::path::Component::Normal(name) = comp {
                    if SKIP_DIRS.contains(&name.to_string_lossy().as_ref()) {
                        return false;
                    }
                }
            }
            // Only files with parseable extensions
            if !e.path().is_file() {
                return false;
            }
            let ext = e.path().extension().and_then(|x| x.to_str()).unwrap_or("");
            PARSEABLE_EXTS.contains(&ext)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

// ---------------------------------------------------------------------------
// FQDN + ID helpers
// ---------------------------------------------------------------------------

fn make_fqdn(file_path: &str, container: Option<&str>, name: &str) -> String {
    match container {
        Some(c) => format!("{}::{}::{}", file_path, c, name),
        None => format!("{}::{}", file_path, name),
    }
}

fn hash_fqdn(fqdn: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(fqdn.as_bytes());
    let b = h.finalize();
    u64::from_le_bytes(b[..8].try_into().unwrap())
}

// ---------------------------------------------------------------------------
// Snippet extraction
// ---------------------------------------------------------------------------

fn extract_snippet(lines: &[String], start: usize, end: usize) -> String {
    const CAP: usize = 200;
    let actual_end = end.min(start + CAP).min(lines.len().saturating_sub(1));
    let text = lines[start..=actual_end].join("\n");
    if end > start + CAP {
        format!("{}\n// ... (truncated)", text)
    } else {
        text
    }
}

// ---------------------------------------------------------------------------
// Main builder
// ---------------------------------------------------------------------------

pub struct GraphBuilder;

impl GraphBuilder {
    /// Build the semantic code graph for `root`, sending progress strings via `progress`.
    pub fn build(root: &Path, progress: &Sender<GraphBuildMsg>) -> anyhow::Result<CodeGraph> {
        let _ = progress.send(GraphBuildMsg::Progress(format!(
            "Scanning {}...",
            root.display()
        )));

        // ── Phase 1: collect & parse files in parallel ───────────────────────
        let files = collect_source_files(root);
        let file_count = files.len();

        let _ = progress.send(GraphBuildMsg::Progress(format!(
            "Found {} source files — parsing...",
            file_count
        )));

        // Parallel parse
        let parsed: Vec<ParsedFile> = files
            .par_iter()
            .filter_map(|abs_path| {
                let rel = abs_path.strip_prefix(root).ok()?;
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                let content = std::fs::read_to_string(abs_path).ok()?;
                let pf = parse_file(&rel_str, &content);
                Some(pf)
            })
            .collect();

        let _ = progress.send(GraphBuildMsg::Progress(format!(
            "Parsed {} files — building graph...",
            parsed.len()
        )));

        // ── Phase 2: build graph (sequential) ────────────────────────────────
        let mut meta = GraphMeta {
            version: GRAPH_VERSION,
            root_path: root.to_string_lossy().to_string(),
            built_at: chrono::Local::now().timestamp(),
            total_nodes: 0,
            total_edges: 0,
            file_count: parsed.len(),
        };

        let mut graph = CodeGraph::new(meta.clone());

        // Insert file nodes
        for pf in &parsed {
            let fqdn = pf.path.clone();
            let node = GraphNode {
                id: hash_fqdn(&fqdn),
                fqdn: fqdn.clone(),
                name: std::path::Path::new(&pf.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| pf.path.clone()),
                kind: NodeKind::File,
                span: Span {
                    start_line: 0,
                    end_line: pf.lines.len() as u32,
                },
                file_path: pf.path.clone(),
                modifiers: Modifiers::default(),
                documentation: None,
                content: CodeContent::stub(format!("// {}", pf.path)),
                language: pf.language.clone(),
            };
            graph.add_node(node);
        }

        // Insert definition nodes and Contains edges
        for pf in &parsed {
            let file_idx = graph.fqdn_index.get(&pf.path).copied();

            for def in &pf.defs {
                let fqdn = make_fqdn(&pf.path, def.container.as_deref(), &def.name);
                // Skip if already present (impl blocks with same type name)
                if graph.fqdn_index.contains_key(&fqdn) {
                    continue;
                }

                let full = extract_snippet(&pf.lines, def.start as usize, def.end as usize);
                let node = GraphNode {
                    id: hash_fqdn(&fqdn),
                    fqdn: fqdn.clone(),
                    name: def.name.clone(),
                    kind: def.kind.clone(),
                    span: Span {
                        start_line: def.start,
                        end_line: def.end,
                    },
                    file_path: pf.path.clone(),
                    modifiers: def.modifiers,
                    documentation: def.doc.clone(),
                    content: CodeContent::new(full),
                    language: pf.language.clone(),
                };
                let node_idx = graph.add_node(node);

                // Contains edge: file → symbol
                if let Some(fi) = file_idx {
                    graph.add_edge(fi, node_idx, EdgeType::Contains);
                }
                // Contains edge: container → method/inner
                if let Some(cont_name) = &def.container {
                    let cont_fqdn = make_fqdn(&pf.path, None, cont_name);
                    if let Some(&ci) = graph.fqdn_index.get(&cont_fqdn) {
                        graph.add_edge(ci, node_idx, EdgeType::Contains);
                    }
                }
            }
        }

        let _ = progress.send(GraphBuildMsg::Progress(format!(
            "Inserted {} nodes — resolving edges...",
            graph.graph.node_count()
        )));

        // ── Phase 3: edge resolution ─────────────────────────────────────────
        // Collect all (from, to, edge_type) tuples first (avoids borrow conflicts),
        // then insert them all at once.

        let mut edges_to_add: Vec<(
            petgraph::stable_graph::NodeIndex,
            petgraph::stable_graph::NodeIndex,
            EdgeType,
        )> = Vec::new();

        for pf in &parsed {
            let file_node = graph.fqdn_index.get(&pf.path).copied();

            // ── Import edges ──────────────────────────────────────────────────
            for imp in &pf.imports {
                let target_normalized = imp.target.replace("::", "/").replace('.', "/");
                let mut matched = false;

                // Try matching a file node by path
                for (fqdn, &idx) in &graph.fqdn_index {
                    if graph
                        .graph
                        .node_weight(idx)
                        .map(|n| n.kind == NodeKind::File)
                        .unwrap_or(false)
                        && (fqdn.ends_with(&target_normalized) || fqdn.contains(&imp.target))
                    {
                        if let Some(fi) = file_node {
                            edges_to_add.push((fi, idx, EdgeType::Imports));
                        }
                        matched = true;
                        break;
                    }
                }

                // Fall back to symbol name lookup
                if !matched {
                    let sym_name = imp
                        .target
                        .split("::")
                        .last()
                        .or_else(|| imp.target.split('.').last())
                        .unwrap_or(&imp.target);
                    let targets: Vec<petgraph::stable_graph::NodeIndex> = graph
                        .name_index
                        .get(sym_name)
                        .map(|v| v.iter().take(3).copied().collect())
                        .unwrap_or_default();
                    if let Some(fi) = file_node {
                        for ti in targets {
                            edges_to_add.push((fi, ti, EdgeType::Imports));
                        }
                    }
                }
            }

            // ── Call edges ────────────────────────────────────────────────────
            for call in &pf.calls {
                let caller_fqdn = find_enclosing_def(pf, call.line);
                let caller_idx = caller_fqdn
                    .as_deref()
                    .and_then(|fq| graph.fqdn_index.get(fq).copied());

                let callee_idxs: Vec<petgraph::stable_graph::NodeIndex> = graph
                    .name_index
                    .get(&call.target)
                    .map(|v| {
                        v.iter()
                            .filter(|&&i| {
                                graph
                                    .graph
                                    .node_weight(i)
                                    .map(|n| {
                                        matches!(
                                            n.kind,
                                            NodeKind::Function
                                                | NodeKind::Method
                                                | NodeKind::Macro
                                                | NodeKind::Constructor
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                            .take(5)
                            .copied()
                            .collect()
                    })
                    .unwrap_or_default();

                if let Some(ci) = caller_idx {
                    for callee_idx in callee_idxs {
                        edges_to_add.push((ci, callee_idx, EdgeType::Calls));
                    }
                }
            }

            // ── MutatesState edges ────────────────────────────────────────────
            for mut_ref in &pf.mutations {
                let caller_fqdn = find_enclosing_def(pf, mut_ref.line);
                if let Some(ref cf) = caller_fqdn {
                    let caller_idx = graph.fqdn_index.get(cf).copied();
                    let target_idxs: Vec<petgraph::stable_graph::NodeIndex> = graph
                        .name_index
                        .get(&mut_ref.target)
                        .map(|v| {
                            v.iter()
                                .filter(|&&i| {
                                    graph
                                        .graph
                                        .node_weight(i)
                                        .map(|n| {
                                            matches!(
                                                n.kind,
                                                NodeKind::GlobalVar
                                                    | NodeKind::Field
                                                    | NodeKind::LocalVar
                                                    | NodeKind::Property
                                            )
                                        })
                                        .unwrap_or(false)
                                })
                                .take(3)
                                .copied()
                                .collect()
                        })
                        .unwrap_or_default();
                    if let Some(ci) = caller_idx {
                        for ti in target_idxs {
                            edges_to_add.push((ci, ti, EdgeType::MutatesState));
                        }
                    }
                }
            }
        }

        // ── Inherits / Implements edges (from parser's superclass/interfaces) ────
        for pf in &parsed {
            for def in &pf.defs {
                let def_fqdn = make_fqdn(&pf.path, def.container.as_deref(), &def.name);
                let def_idx = graph.fqdn_index.get(&def_fqdn).copied();
                if let Some(di) = def_idx {
                    // Superclass → Inherits edge
                    if let Some(ref sc) = def.superclass {
                        if let Some(targets) = graph.name_index.get(sc) {
                            for &ti in targets.iter().take(3) {
                                if let Some(n) = graph.graph.node_weight(ti) {
                                    if matches!(n.kind, NodeKind::Class | NodeKind::Struct) {
                                        edges_to_add.push((di, ti, EdgeType::Inherits));
                                    }
                                }
                            }
                        }
                    }
                    // Interfaces → Implements edges
                    for iface in &def.interfaces {
                        if let Some(targets) = graph.name_index.get(iface) {
                            for &ti in targets.iter().take(3) {
                                if let Some(n) = graph.graph.node_weight(ti) {
                                    if matches!(n.kind, NodeKind::Trait | NodeKind::Interface) {
                                        edges_to_add.push((di, ti, EdgeType::Implements));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Insert all collected edges
        for (from, to, etype) in edges_to_add {
            graph.add_edge(from, to, etype);
        }

        graph.finalize();
        meta.total_nodes = graph.graph.node_count();
        meta.total_edges = graph.graph.edge_count();
        graph.meta = meta;

        let _ = progress.send(GraphBuildMsg::Progress(format!(
            "Graph complete: {} nodes, {} edges",
            graph.graph.node_count(),
            graph.graph.edge_count()
        )));

        Ok(graph)
    }
}

/// Find the FQDN of the definition that encloses `line` in the parsed file.
fn find_enclosing_def(pf: &ParsedFile, line: u32) -> Option<String> {
    // Find the smallest span that contains `line`
    let mut best: Option<&ParsedDef> = None;
    for def in &pf.defs {
        if def.start <= line && line <= def.end {
            match best {
                None => {
                    best = Some(def);
                }
                Some(b) if (def.end - def.start) < (b.end - b.start) => {
                    best = Some(def);
                }
                _ => {}
            }
        }
    }
    best.map(|d| make_fqdn(&pf.path, d.container.as_deref(), &d.name))
}
