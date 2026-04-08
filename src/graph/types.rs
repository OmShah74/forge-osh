use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Modifier bit-flags (stored as u16, no extra dependency needed)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers(pub u16);

pub mod mflags {
    pub const IS_PUBLIC:   u16 = 0b0000_0001;
    pub const IS_ASYNC:    u16 = 0b0000_0010;
    pub const IS_UNSAFE:   u16 = 0b0000_0100;
    pub const IS_STATIC:   u16 = 0b0000_1000;
    pub const IS_MUT:      u16 = 0b0001_0000;
    pub const IS_EXTERN:   u16 = 0b0010_0000;
    pub const IS_CONST:    u16 = 0b0100_0000;
    pub const IS_ABSTRACT: u16 = 0b1000_0000;
}

impl Modifiers {
    #[inline] pub fn is_public(self)   -> bool { self.0 & mflags::IS_PUBLIC   != 0 }
    #[inline] pub fn is_async(self)    -> bool { self.0 & mflags::IS_ASYNC    != 0 }
    #[inline] pub fn is_unsafe(self)   -> bool { self.0 & mflags::IS_UNSAFE   != 0 }
    #[inline] pub fn is_static(self)   -> bool { self.0 & mflags::IS_STATIC   != 0 }
    #[inline] pub fn is_mut(self)      -> bool { self.0 & mflags::IS_MUT      != 0 }
    #[inline] pub fn is_const(self)    -> bool { self.0 & mflags::IS_CONST    != 0 }
    #[inline] pub fn is_abstract(self) -> bool { self.0 & mflags::IS_ABSTRACT != 0 }
    #[inline] pub fn set(&mut self, flag: u16) { self.0 |= flag; }
    #[inline] pub fn with(mut self, flag: u16) -> Self { self.0 |= flag; self }

    pub fn describe(self) -> String {
        let mut parts = Vec::new();
        if self.is_public()   { parts.push("pub"); }
        if self.is_async()    { parts.push("async"); }
        if self.is_unsafe()   { parts.push("unsafe"); }
        if self.is_static()   { parts.push("static"); }
        if self.is_const()    { parts.push("const"); }
        if self.is_mut()      { parts.push("mut"); }
        if self.is_abstract() { parts.push("abstract"); }
        parts.join(" ")
    }
}

// ---------------------------------------------------------------------------
// Node kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Module,
    Class,
    Struct,
    Enum,
    EnumVariant,
    Function,
    Method,
    Trait,
    Interface,
    Impl,
    GlobalVar,
    TypeAlias,
    Macro,
    Field,
    ExternalStub,
}

impl NodeKind {
    pub fn label(&self) -> &'static str {
        match self {
            NodeKind::File          => "FILE",
            NodeKind::Module        => "MODULE",
            NodeKind::Class         => "CLASS",
            NodeKind::Struct        => "STRUCT",
            NodeKind::Enum          => "ENUM",
            NodeKind::EnumVariant   => "VARIANT",
            NodeKind::Function      => "FUNCTION",
            NodeKind::Method        => "METHOD",
            NodeKind::Trait         => "TRAIT",
            NodeKind::Interface     => "INTERFACE",
            NodeKind::Impl          => "IMPL",
            NodeKind::GlobalVar     => "GLOBAL",
            NodeKind::TypeAlias     => "TYPE",
            NodeKind::Macro         => "MACRO",
            NodeKind::Field         => "FIELD",
            NodeKind::ExternalStub  => "EXTERN",
        }
    }
}

// ---------------------------------------------------------------------------
// Edge types (semantic relationships)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    /// Structural containment: File → Module → Class → Method
    Contains,
    /// Module defines a Trait/Interface
    Defines,
    /// Function calls another function
    Calls,
    /// Function instantiates a struct/class
    Instantiates,
    /// Function returns a type
    Returns,
    /// Function reads a global/field (non-mutating)
    ReadsState,
    /// Function mutates a global/field — critical for debugging side-effects
    MutatesState,
    /// Struct/class implements a trait/interface
    Implements,
    /// Class inherits from another class
    Inherits,
    /// File/module imports another
    Imports,
    /// Reference to an external (unresolved) symbol
    ExternalDependency,
}

// ---------------------------------------------------------------------------
// Source span
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub end_line:   u32,
}

// ---------------------------------------------------------------------------
// Code content — token-optimized views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeContent {
    /// Complete source block (may be truncated for very large items)
    pub full_snippet:    String,
    /// First meaningful line — saves LLM tokens when body isn't needed
    pub signature_only:  String,
    /// Approximate token count (~4 chars/token)
    pub token_weight:    usize,
}

impl CodeContent {
    pub fn new(full: String) -> Self {
        let weight = full.len() / 4 + 1;
        // Signature: strip body — take the line up to the first `{`
        let sig = full.lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| {
                if let Some(pos) = l.find('{') { l[..pos].trim().to_string() }
                else { l.trim().to_string() }
            })
            .unwrap_or_default();
        Self { full_snippet: full, signature_only: sig, token_weight: weight }
    }

    pub fn stub(sig: impl Into<String>) -> Self {
        let s = sig.into();
        Self { token_weight: s.len() / 4 + 1, signature_only: s.clone(), full_snippet: s }
    }
}

// ---------------------------------------------------------------------------
// Programming language
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Cpp,
    Java,
    C,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs"                        => Language::Rust,
            "py" | "pyw"                => Language::Python,
            "js" | "mjs" | "cjs"       => Language::JavaScript,
            "ts" | "tsx"                => Language::TypeScript,
            "go"                        => Language::Go,
            "cpp" | "cc" | "cxx" | "hpp" => Language::Cpp,
            "java"                      => Language::Java,
            "c" | "h"                   => Language::C,
            _                           => Language::Unknown,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Language::Rust       => "Rust",
            Language::Python     => "Python",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Go         => "Go",
            Language::Cpp        => "C++",
            Language::Java       => "Java",
            Language::C          => "C",
            Language::Unknown    => "Unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Graph node
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Hash of FQDN (unique ID)
    pub id:            u64,
    /// Fully Qualified Domain Name: "src/agent/loop.rs::AgentLoop::run"
    pub fqdn:          String,
    /// Short symbol name: "run"
    pub name:          String,
    pub kind:          NodeKind,
    pub span:          Span,
    /// Relative path from project root
    pub file_path:     String,
    pub modifiers:     Modifiers,
    pub documentation: Option<String>,
    pub content:       CodeContent,
    pub language:      Language,
}

// ---------------------------------------------------------------------------
// Graph metadata (saved in the artifact header)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMeta {
    /// Artifact format version — bump on breaking changes
    pub version:     u32,
    pub root_path:   String,
    /// Unix timestamp of build
    pub built_at:    i64,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub file_count:  usize,
}

impl GraphMeta {
    pub fn age_description(&self) -> String {
        let now = chrono::Local::now().timestamp();
        let secs = now - self.built_at;
        if secs < 60 { format!("{}s ago", secs) }
        else if secs < 3600 { format!("{}m ago", secs / 60) }
        else if secs < 86400 { format!("{}h ago", secs / 3600) }
        else { format!("{}d ago", secs / 86400) }
    }
}
