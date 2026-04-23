pub mod builder;
pub mod parser;
pub mod query;
pub mod tools;
pub mod types;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::Directed;
use serde::{Deserialize, Serialize};

pub use types::*;

// ---------------------------------------------------------------------------
// Shared graph reference — all components (TUI, AgentLoop, Tool) hold a clone
// ---------------------------------------------------------------------------

/// Arc<RwLock<Option<CodeGraph>>> — None until /forge-graph has been built.
pub type SharedGraph = Arc<RwLock<Option<CodeGraph>>>;

/// Create an empty shared graph (graph not yet built)
pub fn new_shared_graph() -> SharedGraph {
    Arc::new(RwLock::new(None))
}

// ---------------------------------------------------------------------------
// Progress messages sent from the build thread back to the TUI
// ---------------------------------------------------------------------------

#[allow(clippy::large_enum_variant)]
pub enum GraphBuildMsg {
    Progress(String),
    Done {
        graph: CodeGraph,
        artifact_path: PathBuf,
    },
    Error(String),
}

// ---------------------------------------------------------------------------
// Artifact versioning
// ---------------------------------------------------------------------------

/// Increment when the binary format changes in a breaking way.
pub const GRAPH_VERSION: u32 = 2;

// ---------------------------------------------------------------------------
// The semantic code graph
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct CodeGraph {
    pub meta: GraphMeta,
    pub graph: StableGraph<GraphNode, EdgeType, Directed>,

    // ── Indices rebuilt on load (not serialized) ──────────────────────────
    #[serde(skip)]
    pub fqdn_index: HashMap<String, NodeIndex>,
    #[serde(skip)]
    pub file_index: HashMap<String, Vec<NodeIndex>>,
    #[serde(skip)]
    pub name_index: HashMap<String, Vec<NodeIndex>>,
}

impl CodeGraph {
    pub fn new(meta: GraphMeta) -> Self {
        Self {
            meta,
            graph: StableGraph::new(),
            fqdn_index: HashMap::new(),
            file_index: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    /// Add a node and update all in-memory indices.
    pub fn add_node(&mut self, node: GraphNode) -> NodeIndex {
        let fqdn = node.fqdn.clone();
        let name = node.name.clone();
        let file = node.file_path.clone();
        let idx = self.graph.add_node(node);
        self.fqdn_index.insert(fqdn, idx);
        self.file_index.entry(file).or_default().push(idx);
        self.name_index.entry(name).or_default().push(idx);
        idx
    }

    /// Add an edge (avoids duplicate parallel edges).
    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, edge: EdgeType) {
        if !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, edge);
        }
    }

    /// Rebuild all in-memory indices from graph data (called after deserialization).
    pub fn rebuild_indices(&mut self) {
        self.fqdn_index.clear();
        self.file_index.clear();
        self.name_index.clear();

        for idx in self.graph.node_indices() {
            if let Some(node) = self.graph.node_weight(idx) {
                self.fqdn_index.insert(node.fqdn.clone(), idx);
                self.file_index
                    .entry(node.file_path.clone())
                    .or_default()
                    .push(idx);
                self.name_index
                    .entry(node.name.clone())
                    .or_default()
                    .push(idx);
            }
        }
    }

    /// Update metadata totals (call before saving).
    pub fn finalize(&mut self) {
        self.meta.total_nodes = self.graph.node_count();
        self.meta.total_edges = self.graph.edge_count();
    }

    // ── Artifact path helpers ────────────────────────────────────────────────

    /// Deterministic artifact filename for a project root directory.
    pub fn artifact_name(root: &Path) -> String {
        let dir_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string());
        let sanitized: String = dir_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("forge_graph_{sanitized}.bin")
    }

    /// Full path to the artifact.
    pub fn artifact_path(root: &Path, exe_dir: &Path) -> PathBuf {
        exe_dir.join(Self::artifact_name(root))
    }

    /// Detect the artifact directory: binary location, then current dir.
    pub fn artifact_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    // ── Persistence ─────────────────────────────────────────────────────────

    /// Try to find and load the graph artifact for `root`. Returns None if not
    /// present, version-mismatched, or corrupted.
    pub fn try_load(root: &Path) -> Option<Self> {
        let exe_dir = Self::artifact_dir();
        let artifact = Self::artifact_path(root, &exe_dir);
        if !artifact.exists() {
            return None;
        }
        match Self::load(&artifact) {
            Ok(g) if g.meta.version == GRAPH_VERSION => Some(g),
            _ => None, // stale or corrupt — treat as missing
        }
    }

    /// Save the graph to a binary file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let bytes = bincode::serialize(self)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Load from a binary file and rebuild indices.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        let mut graph: CodeGraph = bincode::deserialize(&bytes)?;
        graph.rebuild_indices();
        Ok(graph)
    }
}
