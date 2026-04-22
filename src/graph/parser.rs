/// Language-specific source file parsers.
///
/// Each parser performs two passes:
///   1. **Definition pass** — extract all named symbols (fn, struct, class, etc.)
///      with their line ranges, modifiers, and documentation.
///   2. **Edge pass** — extract imports and approximate call sites.
///
/// Parsing is regex-based (no external grammar crates) so it compiles on every
/// platform without a C toolchain for the grammar libraries.
use regex::Regex;

use crate::graph::types::*;

/// `re!(r"...")` — lazily compiles a regex literal to a static `&'static Regex`.
///
/// Each call site gets its own `OnceLock<Regex>`; the pattern is compiled on the
/// first invocation and reused thereafter. Because the pattern is a compile-time
/// literal, any compile failure is a deterministic programmer bug that surfaces
/// on first parse, not a runtime panic on malformed input.
macro_rules! re {
    ($pattern:expr) => {{
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| Regex::new($pattern).expect("invalid regex literal in parser.rs"))
    }};
}

// ---------------------------------------------------------------------------
// Output types (used by the builder)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedDef {
    pub name:       String,
    pub kind:       NodeKind,
    pub start:      u32,
    pub end:        u32,
    pub modifiers:  Modifiers,
    pub doc:        Option<String>,
    /// Parent symbol name (e.g. struct/impl that contains this method)
    pub container:  Option<String>,
    /// Superclass name (for single-inheritance languages: Python, Java, C#, etc.)
    pub superclass: Option<String>,
    /// Trait / interface names that this type implements or conforms to
    pub interfaces: Vec<String>,
}

impl ParsedDef {
    /// Convenience constructor for definitions with no inheritance.
    fn new(
        name: String, kind: NodeKind,
        start: u32, end: u32,
        modifiers: Modifiers,
        doc: Option<String>,
        container: Option<String>,
    ) -> Self {
        Self { name, kind, start, end, modifiers, doc, container,
               superclass: None, interfaces: Vec::new() }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedImport {
    pub target: String,
    pub line:   u32,
}

#[derive(Debug, Clone)]
pub struct ParsedCall {
    pub target: String,
    pub line:   u32,
}

#[derive(Debug, Clone)]
pub struct ParsedMutation {
    pub target: String,
    pub line:   u32,
}

pub struct ParsedFile {
    pub path:      String,
    pub language:  Language,
    pub lines:     Vec<String>,
    pub defs:      Vec<ParsedDef>,
    pub imports:   Vec<ParsedImport>,
    pub calls:     Vec<ParsedCall>,
    pub mutations: Vec<ParsedMutation>,
}

// ---------------------------------------------------------------------------
// Top-level dispatcher
// ---------------------------------------------------------------------------

pub fn parse_file(path: &str, content: &str) -> ParsedFile {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let language = Language::from_extension(ext);
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    match language {
        Language::Rust       => parse_rust(path, &lines, language),
        Language::Python     => parse_python(path, &lines, language),
        Language::JavaScript |
        Language::TypeScript => parse_js_ts(path, &lines, language),
        Language::Go         => parse_go(path, &lines, language),
        Language::Java       => parse_java(path, &lines, language),
        Language::C |
        Language::Cpp        => parse_c_cpp(path, &lines, language),
        Language::CSharp     => parse_csharp(path, &lines, language),
        Language::Ruby       => parse_ruby(path, &lines, language),
        Language::Kotlin     => parse_kotlin(path, &lines, language),
        Language::Swift      => parse_swift(path, &lines, language),
        Language::PHP        => parse_php(path, &lines, language),
        Language::Lua        => parse_lua(path, &lines, language),
        Language::Scala      => parse_scala(path, &lines, language),
        Language::Bash       => parse_bash(path, &lines, language),
        _                    => ParsedFile {
            path: path.to_string(),
            language,
            lines,
            defs: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            mutations: Vec::new(),
        },
    }
}

// ---------------------------------------------------------------------------
// Utility: brace-depth scanner (skips string literals and line comments)
// ---------------------------------------------------------------------------

/// Scan forward from `start_line` until the opening `{` at depth 0 closes.
/// Returns the line index of the matching `}`.
fn find_block_end(lines: &[String], start: usize) -> usize {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut string_char = b'"';

    for (i, line) in lines.iter().enumerate().skip(start) {
        let bytes = line.as_bytes();
        let mut j = 0;
        while j < bytes.len() {
            let b = bytes[j];
            if in_string {
                if b == b'\\' {
                    j += 1; // skip escaped char
                } else if b == string_char {
                    in_string = false;
                }
            } else {
                match b {
                    b'"' | b'\'' => { in_string = true; string_char = b; }
                    b'/' if j + 1 < bytes.len() && bytes[j+1] == b'/' => break, // line comment
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 { return i; }
                    }
                    _ => {}
                }
            }
            j += 1;
        }
    }
    lines.len().saturating_sub(1)
}

/// Scan forward from `start` for Ruby/Lua `end`-delimited blocks.
/// Counts nested block-openers vs `end` keywords, returns the closing line.
fn find_end_keyword_block(lines: &[String], start: usize) -> usize {
    let mut depth: i32 = 0;
    for (i, line) in lines.iter().enumerate().skip(start) {
        let trimmed = line.trim();
        // Block openers
        if trimmed.starts_with("def ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("module ")
            || trimmed.starts_with("if ")
            || trimmed.starts_with("unless ")
            || trimmed.starts_with("case ")
            || trimmed.starts_with("while ")
            || trimmed.starts_with("until ")
            || trimmed.starts_with("for ")
            || trimmed.starts_with("begin")
            || trimmed.starts_with("do ")
            || trimmed.starts_with("do|")
            || trimmed == "do"
            || trimmed.starts_with("function ")
            || trimmed.starts_with("local function ")
        {
            depth += 1;
        }
        // Inline `do` (e.g. `items.each do |x|`) — only count if not already an opener
        if !trimmed.starts_with("do") && (trimmed.contains(" do |") || trimmed.contains(" do\n") || trimmed.ends_with(" do")) {
            depth += 1;
        }
        if trimmed == "end" || trimmed.starts_with("end ") || trimmed.starts_with("end;")
            || trimmed.starts_with("end)")
        {
            if depth <= 1 { return i; }
            depth -= 1;
        }
    }
    lines.len().saturating_sub(1)
}

/// Extract doc-comment lines immediately preceding `start_line`.
fn extract_doc(lines: &[String], start: usize) -> Option<String> {
    let mut docs: Vec<&str> = Vec::new();
    let mut i = start.saturating_sub(1);
    loop {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("///") {
            docs.push(trimmed.trim_start_matches("///").trim());
        } else if trimmed.starts_with("//!") {
            docs.push(trimmed.trim_start_matches("//!").trim());
        } else if trimmed.starts_with("/**") || trimmed.starts_with("*") || trimmed.starts_with("*/") {
            let stripped = trimmed.trim_start_matches("/**").trim_start_matches("*/")
                .trim_start_matches('*').trim();
            if !stripped.is_empty() { docs.push(stripped); }
        } else if trimmed.starts_with('#') || trimmed.is_empty() {
            // attributes and blank lines — skip over them going upward
        } else {
            break;
        }
        if i == 0 { break; }
        i -= 1;
    }
    if docs.is_empty() { None } else { docs.reverse(); Some(docs.join(" ")) }
}

/// Extract snippet for lines[start..=end], capped at 200 lines.
#[allow(dead_code)]
fn snippet(lines: &[String], start: usize, end: usize) -> String {
    let cap = 200usize;
    let actual_end = end.min(start + cap);
    let actual_end = actual_end.min(lines.len().saturating_sub(1));
    let text = lines[start..=actual_end].join("\n");
    if end > start + cap {
        format!("{}\n// ... (body truncated)", text)
    } else {
        text
    }
}

/// Parse a comma-separated list of identifiers, filtering out keyword args.
fn parse_parent_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| {
            !s.is_empty()
                && !s.contains('=')
                && !s.starts_with('*')
                && s.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
        })
        // Strip generic parameters: Foo<T> → Foo
        .map(|s| {
            if let Some(pos) = s.find('<') { s[..pos].to_string() }
            else if let Some(pos) = s.find('(') { s[..pos].to_string() }
            else { s }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rust parser
// ---------------------------------------------------------------------------

fn parse_rust(path: &str, lines: &[String], language: Language) -> ParsedFile {
    // Regex for use statements
    let use_re = re!(r"^\s*use\s+(.+?);");
    // Regex for approximate call sites: word followed by `(`
    let call_re = re!(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(");
    // Regex for `let mut` and bare assignments (mutation detection)
    let let_mut_re = re!(r"\blet\s+mut\s+([a-zA-Z_][a-zA-Z0-9_]*)");
    let assign_re  = re!(r"^\s*([a-zA-Z_][a-zA-Z0-9_.]*)(?:\[.*?\])?\s*=\s*[^=]");

    let mut defs: Vec<ParsedDef>      = Vec::new();
    let mut imports: Vec<ParsedImport> = Vec::new();
    let mut calls: Vec<ParsedCall>     = Vec::new();
    let mut mutations: Vec<ParsedMutation> = Vec::new();

    // Container stack: (name, brace_depth_when_entered)
    let mut container_stack: Vec<(String, i32)> = Vec::new();
    let mut brace_depth: i32 = 0;

    let mut i = 0;
    while i < lines.len() {
        let raw   = &lines[i];
        let line  = raw.trim();

        // Maintain brace depth
        let mut in_str  = false;
        let mut str_ch  = b'"';
        let bytes = raw.as_bytes();
        let mut j = 0;
        while j < bytes.len() {
            let b = bytes[j];
            if in_str {
                if b == b'\\' { j += 1; }
                else if b == str_ch { in_str = false; }
            } else {
                match b {
                    b'"' | b'\'' => { in_str = true; str_ch = b; }
                    b'/' if j+1 < bytes.len() && bytes[j+1] == b'/' => break,
                    b'{' => brace_depth += 1,
                    b'}' => {
                        brace_depth -= 1;
                        // Pop containers that ended
                        while let Some((_, d)) = container_stack.last() {
                            if brace_depth < *d { container_stack.pop(); } else { break; }
                        }
                    }
                    _ => {}
                }
            }
            j += 1;
        }

        // Skip empty lines and pure comments
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            i += 1;
            continue;
        }

        // ── use statements ───────────────────────────────────────────────
        if let Some(cap) = use_re.captures(line) {
            imports.push(ParsedImport {
                target: cap[1].to_string(),
                line: i as u32,
            });
            i += 1;
            continue;
        }

        // ── Definition parsing ───────────────────────────────────────────
        let current_container = container_stack.last().map(|(n, _)| n.clone());

        if let Some(def) = try_parse_rust_def(line, i, &current_container, lines) {
            let (name, kind) = (def.name.clone(), def.kind.clone());
            // Push impl/struct/mod/trait/enum as containers
            match kind {
                NodeKind::Impl | NodeKind::Struct | NodeKind::Enum |
                NodeKind::Trait | NodeKind::Module | NodeKind::Class => {
                    // The block starts at or after this line
                    // Find the depth at which the block opens
                    let open_depth = brace_depth; // the `{` was already counted above
                    container_stack.push((name, open_depth));
                }
                _ => {}
            }
            defs.push(def);
        }

        // ── Call detection (approximate) ─────────────────────────────────
        for cap in call_re.captures_iter(line) {
            let name = &cap[1];
            // Skip keywords and very-common non-call words
            if !is_rust_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }

        // ── Mutation detection ───────────────────────────────────────────
        for cap in let_mut_re.captures_iter(line) {
            mutations.push(ParsedMutation { target: cap[1].to_string(), line: i as u32 });
        }
        if let Some(cap) = assign_re.captures(line) {
            let target = &cap[1];
            if !target.contains("let ") && !target.starts_with("//") {
                mutations.push(ParsedMutation { target: target.to_string(), line: i as u32 });
            }
        }

        i += 1;
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations }
}

/// Try to parse a Rust definition from a single line (stripped of leading whitespace).
fn try_parse_rust_def(
    line: &str,
    line_num: usize,
    container: &Option<String>,
    all_lines: &[String],
) -> Option<ParsedDef> {
    // Strip visibility modifier
    let (mut mods, rest1) = strip_rust_visibility(line);

    // Strip async
    let rest2 = if let Some(r) = rest1.strip_prefix("async ") { mods.set(mflags::IS_ASYNC); r.trim_start() }
                else { rest1 };
    // Strip unsafe
    let rest3 = if let Some(r) = rest2.strip_prefix("unsafe ") { mods.set(mflags::IS_UNSAFE); r.trim_start() }
                else { rest2 };
    // Strip extern "..."
    let rest4 = if rest3.starts_with("extern ") {
        mods.set(mflags::IS_EXTERN);
        if let Some(pos) = rest3.find("fn ") { &rest3[pos..] } else { rest3 }
    } else { rest3 };

    // fn
    if let Some(r) = rest4.strip_prefix("fn ") {
        let name = extract_ident(r)?;
        let end = find_block_end(all_lines, line_num);
        let kind = if container.is_some() { NodeKind::Method } else { NodeKind::Function };
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, kind, line_num as u32, end as u32, mods, doc, container.clone()));
    }
    // struct
    if let Some(r) = rest4.strip_prefix("struct ") {
        let name = extract_ident(r)?;
        let end = if line.contains(';') { line_num } else { find_block_end(all_lines, line_num) };
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, NodeKind::Struct, line_num as u32, end as u32, mods, doc, container.clone()));
    }
    // enum
    if let Some(r) = rest4.strip_prefix("enum ") {
        let name = extract_ident(r)?;
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, NodeKind::Enum, line_num as u32, end as u32, mods, doc, container.clone()));
    }
    // trait
    if let Some(r) = rest4.strip_prefix("trait ") {
        let name = extract_ident(r)?;
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, NodeKind::Trait, line_num as u32, end as u32, mods, doc, container.clone()));
    }
    // impl — detect `impl Trait for Type` and record the trait in interfaces
    if rest4.starts_with("impl") {
        let name = parse_rust_impl_name(rest4);
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        let mut def = ParsedDef::new(name, NodeKind::Impl, line_num as u32, end as u32, mods, doc, container.clone());
        // Extract trait name from `impl Trait for Type`
        if let Some(trait_name) = parse_rust_impl_trait(rest4) {
            def.interfaces.push(trait_name);
        }
        return Some(def);
    }
    // mod
    if let Some(r) = rest4.strip_prefix("mod ") {
        let name = extract_ident(r)?;
        // Only block mods (with `{`), not `mod foo;`
        if line.contains('{') {
            let end = find_block_end(all_lines, line_num);
            let doc = extract_doc(all_lines, line_num);
            return Some(ParsedDef::new(name, NodeKind::Module, line_num as u32, end as u32, mods, doc, container.clone()));
        }
    }
    // type alias
    if let Some(r) = rest4.strip_prefix("type ") {
        let name = extract_ident(r)?;
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, NodeKind::TypeAlias, line_num as u32, line_num as u32, mods, doc, container.clone()));
    }
    // const — now maps to NodeKind::Constant
    if let Some(r) = rest4.strip_prefix("const ") {
        let name = extract_ident(r)?;
        mods.set(mflags::IS_CONST);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, NodeKind::Constant, line_num as u32, line_num as u32, mods, doc, container.clone()));
    }
    // static
    if let Some(r) = rest4.strip_prefix("static ") {
        let r2 = if let Some(r) = r.strip_prefix("mut ") { mods.set(mflags::IS_MUT); r.trim_start() } else { r };
        let name = extract_ident(r2)?;
        mods.set(mflags::IS_STATIC);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, NodeKind::GlobalVar, line_num as u32, line_num as u32, mods, doc, container.clone()));
    }
    // macro_rules!
    if let Some(r) = line.strip_prefix("macro_rules! ") {
        let name = extract_ident(r)?;
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef::new(name, NodeKind::Macro, line_num as u32, end as u32, mods, doc, container.clone()));
    }

    None
}

/// Strip leading Rust visibility modifier; return (Modifiers, remaining).
fn strip_rust_visibility(s: &str) -> (Modifiers, &str) {
    let mut mods = Modifiers::default();
    if s.starts_with("pub(crate)") {
        mods.set(mflags::IS_PUBLIC);
        return (mods, s["pub(crate)".len()..].trim_start());
    }
    if s.starts_with("pub(super)") {
        mods.set(mflags::IS_PUBLIC);
        return (mods, s["pub(super)".len()..].trim_start());
    }
    if s.starts_with("pub(in ") {
        if let Some(end) = s.find(')') {
            mods.set(mflags::IS_PUBLIC);
            return (mods, s[end+1..].trim_start());
        }
    }
    if let Some(rest) = s.strip_prefix("pub ") {
        mods.set(mflags::IS_PUBLIC);
        return (mods, rest.trim_start());
    }
    (mods, s)
}

/// Extract the implementing type name from an impl line.
fn parse_rust_impl_name(s: &str) -> String {
    // e.g. "impl AgentLoop", "impl<T> Foo", "impl Trait for Type"
    let s = s.trim_start_matches("impl").trim_start();
    // Skip generic parameters
    let s = if s.starts_with('<') {
        s.find('>').map(|i| s[i+1..].trim_start()).unwrap_or(s)
    } else { s };
    // If "Trait for Type", grab Type
    if let Some(pos) = s.find(" for ") {
        return extract_ident(&s[pos + 5..]).unwrap_or_else(|| s.to_string());
    }
    extract_ident(s).unwrap_or_else(|| s.to_string())
}

/// Extract the trait name from `impl Trait for Type`, if present.
fn parse_rust_impl_trait(s: &str) -> Option<String> {
    let s = s.trim_start_matches("impl").trim_start();
    let s = if s.starts_with('<') {
        s.find('>').map(|i| s[i+1..].trim_start()).unwrap_or(s)
    } else { s };
    if s.contains(" for ") {
        // Everything before " for " is the trait name
        let trait_part = s.split(" for ").next()?;
        extract_ident(trait_part)
    } else {
        None
    }
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "while" | "for" | "loop" | "match" | "return" |
               "let" | "mut" | "fn" | "pub" | "use" | "mod" | "struct" | "enum" |
               "trait" | "impl" | "where" | "type" | "self" | "Self" | "super" |
               "crate" | "async" | "await" | "move" | "break" | "continue" |
               "println" | "eprintln" | "format" | "vec" | "panic" | "assert" |
               "Some" | "None" | "Ok" | "Err" | "true" | "false" | "Box" |
               "Arc" | "Rc" | "Vec" | "String" | "Option" | "Result")
}

// ---------------------------------------------------------------------------
// Python parser
// ---------------------------------------------------------------------------

fn parse_python(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let def_re   = re!(r"^(\s*)(?:(async)\s+)?def\s+([a-zA-Z_][a-zA-Z0-9_]*)");
    let class_re = re!(r"^(\s*)class\s+([A-Za-z_][A-Za-z0-9_]*)(?:\(([^)]*)\))?");
    let imp_re   = re!(r"^\s*import\s+(.+)");
    let from_re  = re!(r"^\s*from\s+\S+\s+import\s+(.+)");
    let call_re  = re!(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(");
    let const_re = re!(r"^([A-Z][A-Z0-9_]+)\s*=\s*.+");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<(String, usize)> = None; // (name, indent)

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.as_str();

        // Pop class if indent went back to class level
        if let Some((_, class_indent)) = &current_class {
            let this_indent = line.len() - line.trim_start().len();
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') && this_indent <= *class_indent {
                current_class = None;
            }
        }

        // Imports
        if let Some(cap) = from_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].trim().to_string(), line: i as u32 });
            continue;
        }
        if let Some(cap) = imp_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].trim().to_string(), line: i as u32 });
            continue;
        }

        // Class — with inheritance detection
        if let Some(cap) = class_re.captures(line) {
            let indent = cap[1].len();
            let name   = cap[2].to_string();
            let end = python_block_end(lines, i, indent);
            let doc = extract_doc(lines, i);

            let mut superclass: Option<String> = None;
            let mut interfaces: Vec<String> = Vec::new();

            // Parse parent classes from (Base, Mixin1, Mixin2, metaclass=ABCMeta)
            if let Some(parents_match) = cap.get(3) {
                let parents = parse_parent_list(parents_match.as_str());
                if let Some(first) = parents.first() {
                    superclass = Some(first.clone());
                }
                for p in parents.iter().skip(1) {
                    interfaces.push(p.clone());
                }
            }

            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None,
                superclass, interfaces,
            });
            current_class = Some((name, indent));
            continue;
        }

        // Module-level constants (ALL_CAPS = value)
        if current_class.is_none() {
            let trimmed = line.trim();
            if let Some(cap) = const_re.captures(trimmed) {
                let name = cap[1].to_string();
                let doc = extract_doc(lines, i);
                defs.push(ParsedDef::new(name, NodeKind::Constant, i as u32, i as u32,
                    Modifiers::default(), doc, None));
            }
        }

        // Function / method
        if let Some(cap) = def_re.captures(line) {
            let indent    = cap[1].len();
            let is_async  = cap.get(2).is_some();
            let name      = cap[3].to_string();
            let container = current_class.as_ref().map(|(n, _)| n.clone());
            let kind = if name == "__init__" && container.is_some() {
                NodeKind::Constructor
            } else if container.is_some() {
                NodeKind::Method
            } else {
                NodeKind::Function
            };
            let end = python_block_end(lines, i, indent);
            let doc = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if is_async { mods.set(mflags::IS_ASYNC); }
            if name.starts_with('_') && !name.starts_with("__") {
                // Convention: single underscore = private
            } else if !name.starts_with('_') {
                mods.set(mflags::IS_PUBLIC);
            }
            defs.push(ParsedDef::new(name, kind, i as u32, end as u32, mods, doc, container));
            continue;
        }

        // Calls
        for cap in call_re.captures_iter(line) {
            let name = &cap[1];
            if !is_python_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

/// Find end of a Python block starting at `start` with base `indent`.
fn python_block_end(lines: &[String], start: usize, indent: usize) -> usize {
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
        let this_indent = line.len() - line.trim_start().len();
        if this_indent <= indent { return i.saturating_sub(1); }
    }
    lines.len().saturating_sub(1)
}

fn is_python_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "elif" | "while" | "for" | "with" | "return" |
               "def" | "class" | "import" | "from" | "as" | "not" | "and" | "or" |
               "in" | "is" | "True" | "False" | "None" | "pass" | "break" |
               "continue" | "raise" | "try" | "except" | "finally" | "lambda" |
               "print" | "len" | "range" | "str" | "int" | "float" | "list" |
               "dict" | "set" | "tuple" | "super" | "self")
}

// ---------------------------------------------------------------------------
// JavaScript / TypeScript parser
// ---------------------------------------------------------------------------

fn parse_js_ts(path: &str, lines: &[String], language: Language) -> ParsedFile {
    // Functions: function foo(), async function foo()
    let fn_re    = re!(r"^(?:export\s+(?:default\s+)?)?(?:async\s+)?function\s*\*?\s+([a-zA-Z_$][a-zA-Z0-9_$]*)");
    // Arrow / const fn: const foo = (async )? (...) =>
    let arr_re   = re!(r"^(?:export\s+)?(?:const|let|var)\s+([a-zA-Z_$][a-zA-Z0-9_$]*)\s*=\s*(?:async\s+)?\(.*\)\s*=>");
    // Class: class Foo extends Bar implements Baz, Qux {
    let cls_re   = re!(r"^(?:export\s+(?:default\s+)?)?(?:abstract\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)(?:\s+extends\s+([A-Za-z_$][A-Za-z0-9_$.]*))?(?:\s+implements\s+([^{]+))?");
    // Interface (TS)
    let intf_re  = re!(r"^(?:export\s+)?interface\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s+extends\s+([^{]+))?");
    // Type alias (TS)
    let type_re  = re!(r"^(?:export\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)\s*=");
    // Enum (TS)
    let enum_re  = re!(r"^(?:export\s+)?(?:const\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)");
    // Method inside class: methodName(...) or async methodName(...)
    let mth_re   = re!(r"^\s+(?:async\s+|static\s+|private\s+|protected\s+|public\s+|readonly\s+|override\s+|abstract\s+)*([a-zA-Z_$][a-zA-Z0-9_$]*)\s*\(");
    // Import
    let imp_re   = re!(r#"^import\s+.*?\s+from\s+['"](.+?)['"]"#);
    let req_re   = re!(r#"require\(['"](.+?)['"]\)"#);
    // Calls
    let call_re  = re!(r"\b([a-zA-Z_$][a-zA-Z0-9_$]*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        // Track brace depth
        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if let Some(_) = &current_class {
            if brace_depth < class_depth { current_class = None; }
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") { continue; }

        // Import
        if let Some(cap) = imp_re.captures(raw_line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
        }
        for cap in req_re.captures_iter(raw_line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
        }

        // Class — with extends/implements detection
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);

            let superclass = cap.get(2).map(|m| m.as_str().trim().to_string());
            let interfaces: Vec<String> = cap.get(3)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();

            let mut mods = Modifiers::default();
            if line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
            if line.starts_with("export") { mods.set(mflags::IS_PUBLIC); }

            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: mods, doc, container: None,
                superclass, interfaces,
            });
            current_class = Some(name);
            class_depth = prev_depth + 1;
            continue;
        }
        // Interface (TS) — with extends detection
        if let Some(cap) = intf_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut def = ParsedDef::new(name, NodeKind::Interface, i as u32, end as u32,
                Modifiers::default(), doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            continue;
        }
        // Enum (TS)
        if let Some(cap) = enum_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Enum, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }
        // Type alias
        if let Some(cap) = type_re.captures(line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::TypeAlias, i as u32, i as u32,
                Modifiers::default(), doc, None));
            continue;
        }
        // Function declaration
        if let Some(cap) = fn_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let kind = if current_class.is_some() { NodeKind::Method } else { NodeKind::Function };
            defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                Modifiers::default(), doc, current_class.clone()));
            continue;
        }
        // Arrow function
        if let Some(cap) = arr_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Function, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }
        // Method inside class (indented)
        if current_class.is_some() && raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            if let Some(cap) = mth_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_js_keyword(&name) {
                    let end = find_block_end(lines, i);
                    let doc = extract_doc(lines, i);
                    let kind = if name == "constructor" { NodeKind::Constructor } else { NodeKind::Method };
                    let mut mods = Modifiers::default();
                    if raw_line.contains("static ") { mods.set(mflags::IS_STATIC); }
                    if raw_line.contains("async ") { mods.set(mflags::IS_ASYNC); }
                    if raw_line.contains("override ") { mods.set(mflags::IS_OVERRIDE); }
                    defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                        mods, doc, current_class.clone()));
                }
            }
        }

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_js_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_js_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "while" | "for" | "do" | "switch" | "case" | "break" |
               "continue" | "return" | "throw" | "try" | "catch" | "finally" |
               "new" | "delete" | "typeof" | "instanceof" | "void" | "in" | "of" |
               "function" | "class" | "import" | "export" | "from" | "as" | "default" |
               "const" | "let" | "var" | "async" | "await" | "yield" |
               "true" | "false" | "null" | "undefined" | "this" | "super" |
               "console" | "document" | "window" | "require" | "module" |
               "Promise" | "Array" | "Object" | "String" | "Number" | "Boolean" |
               "Math" | "JSON" | "Error" | "Map" | "Set")
}

// ---------------------------------------------------------------------------
// Go parser
// ---------------------------------------------------------------------------

fn parse_go(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let func_re  = re!(r"^func\s+(?:\([^)]*\)\s+)?([a-zA-Z_][a-zA-Z0-9_]*)");
    let type_re  = re!(r"^type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(struct|interface|func|[A-Za-z])");
    let imp_re   = re!(r#"^\s+"?([^"]+)"?\s*$"#);
    let call_re  = re!(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut in_import_block = false;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();

        if line.is_empty() || line.starts_with("//") { continue; }

        // Import block
        if line == "import (" { in_import_block = true; continue; }
        if line == ")" && in_import_block { in_import_block = false; continue; }
        if in_import_block {
            if let Some(cap) = imp_re.captures(line) {
                imports.push(ParsedImport { target: cap[1].trim_matches('"').to_string(), line: i as u32 });
            }
            continue;
        }
        // Single import
        if line.starts_with("import \"") {
            let t = line.trim_start_matches("import \"").trim_end_matches('"');
            imports.push(ParsedImport { target: t.to_string(), line: i as u32 });
            continue;
        }

        // func
        if let Some(cap) = func_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            // Is it a method (has receiver)?
            let kind = if raw.starts_with("func (") { NodeKind::Method } else { NodeKind::Function };
            let mut mods = Modifiers::default();
            // Go convention: exported if uppercase first letter
            if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                mods.set(mflags::IS_PUBLIC);
            }
            defs.push(ParsedDef::new(name, kind, i as u32, end as u32, mods, doc, None));
            continue;
        }
        // type
        if let Some(cap) = type_re.captures(line) {
            let name  = cap[1].to_string();
            let kword = &cap[2];
            let kind  = match kword { "struct" => NodeKind::Struct, "interface" => NodeKind::Interface, _ => NodeKind::TypeAlias };
            let end   = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc   = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                mods.set(mflags::IS_PUBLIC);
            }
            defs.push(ParsedDef::new(name, kind, i as u32, end as u32, mods, doc, None));
            continue;
        }

        // Calls
        for cap in call_re.captures_iter(line) {
            let name = &cap[1];
            if !is_go_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_go_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "for" | "range" | "switch" | "case" | "default" |
               "break" | "continue" | "return" | "goto" | "fallthrough" | "defer" |
               "go" | "select" | "chan" | "func" | "type" | "struct" | "interface" |
               "map" | "var" | "const" | "import" | "package" | "make" | "new" |
               "len" | "cap" | "append" | "copy" | "delete" | "close" | "panic" |
               "recover" | "print" | "println" | "true" | "false" | "nil" |
               "int" | "int8" | "int16" | "int32" | "int64" | "uint" |
               "float32" | "float64" | "string" | "bool" | "byte" | "rune" | "error")
}

// ---------------------------------------------------------------------------
// Java parser
// ---------------------------------------------------------------------------

fn parse_java(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let imp_re   = re!(r"^\s*import\s+([\w.]+(?:\.\*)?)\s*;");
    let cls_re   = re!(r"(?:public\s+|private\s+|protected\s+)?(?:abstract\s+|final\s+|static\s+)*(?:class|record)\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s+extends\s+([A-Za-z_][\w.]*))?(?:\s+implements\s+([^{]+))?");
    let intf_re  = re!(r"(?:public\s+)?interface\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s+extends\s+([^{]+))?");
    let enum_re  = re!(r"(?:public\s+|private\s+|protected\s+)?enum\s+([A-Za-z_]\w*)");
    let mth_re   = re!(r"^\s+(?:public\s+|private\s+|protected\s+)?(?:static\s+|final\s+|abstract\s+|synchronized\s+|native\s+|default\s+)*(?:<[^>]*>\s+)?(?:[\w<>\[\], ]+?)\s+([a-zA-Z_]\w*)\s*\(");
    let pkg_re   = re!(r"^\s*package\s+([\w.]+)\s*;");
    let call_re  = re!(r"\b([a-zA-Z_]\w*)\s*\(");
    let const_re = re!(r"^\s+(?:public\s+|private\s+|protected\s+)?static\s+final\s+\S+\s+([A-Z][A-Z0-9_]+)\s*=");
    let annot_re = re!(r"^\s*@(\w+)");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;
    let mut pending_override = false;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if current_class.is_some() && brace_depth < class_depth {
            current_class = None;
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") { continue; }

        // Package
        if let Some(cap) = pkg_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // Import
        if let Some(cap) = imp_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // Annotations
        if let Some(cap) = annot_re.captures(line) {
            if &cap[1] == "Override" { pending_override = true; }
            continue;
        }

        // Class / record
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let superclass = cap.get(2).map(|m| m.as_str().trim().to_string());
            let interfaces: Vec<String> = cap.get(3)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            if line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
            if line.contains("final ") { mods.set(mflags::IS_FINAL); }
            if line.contains("static ") { mods.set(mflags::IS_STATIC); }
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: mods, doc, container: None,
                superclass, interfaces,
            });
            current_class = Some(name);
            class_depth = prev_depth + 1;
            continue;
        }

        // Interface
        if let Some(cap) = intf_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            let mut def = ParsedDef::new(name, NodeKind::Interface, i as u32, end as u32, mods, doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            continue;
        }

        // Enum
        if let Some(cap) = enum_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            defs.push(ParsedDef::new(name, NodeKind::Enum, i as u32, end as u32, mods, doc, None));
            continue;
        }

        // Constants (static final)
        if let Some(cap) = const_re.captures(raw_line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            let mut mods = Modifiers(0).with(mflags::IS_STATIC).with(mflags::IS_FINAL);
            if raw_line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            defs.push(ParsedDef::new(name, NodeKind::Constant, i as u32, i as u32, mods, doc, current_class.clone()));
            continue;
        }

        // Method / constructor (inside class)
        if current_class.is_some() {
            // Constructor: same name as class
            if let Some(ref cname) = current_class {
                let ctor_pattern = format!("{}(", cname);
                if line.contains(&ctor_pattern) && !line.contains("=") && !line.contains("new ") {
                    let end = find_block_end(lines, i);
                    let doc = extract_doc(lines, i);
                    let mut mods = Modifiers::default();
                    if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
                    defs.push(ParsedDef::new(cname.clone(), NodeKind::Constructor, i as u32, end as u32,
                        mods, doc, current_class.clone()));
                    pending_override = false;
                    continue;
                }
            }
            // Regular method
            if let Some(cap) = mth_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_java_keyword(&name) && Some(&name) != current_class.as_ref() {
                    let end = find_block_end(lines, i);
                    let doc = extract_doc(lines, i);
                    let mut mods = Modifiers::default();
                    if raw_line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
                    if raw_line.contains("static ") { mods.set(mflags::IS_STATIC); }
                    if raw_line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
                    if raw_line.contains("final ") { mods.set(mflags::IS_FINAL); }
                    if raw_line.contains("synchronized ") { mods.set(mflags::IS_NATIVE); }
                    if pending_override { mods.set(mflags::IS_OVERRIDE); }
                    defs.push(ParsedDef::new(name, NodeKind::Method, i as u32, end as u32,
                        mods, doc, current_class.clone()));
                    pending_override = false;
                    continue;
                }
            }
        }

        pending_override = false;

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_java_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_java_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "while" | "for" | "do" | "switch" | "case" | "break" |
               "continue" | "return" | "throw" | "try" | "catch" | "finally" |
               "new" | "instanceof" | "void" | "class" | "interface" | "enum" |
               "extends" | "implements" | "import" | "package" | "public" |
               "private" | "protected" | "static" | "final" | "abstract" |
               "synchronized" | "native" | "volatile" | "transient" | "default" |
               "this" | "super" | "true" | "false" | "null" |
               "int" | "long" | "short" | "byte" | "float" | "double" |
               "boolean" | "char" | "String" | "Integer" | "Long" | "Double" |
               "Boolean" | "Object" | "List" | "Map" | "Set" | "Arrays" |
               "Collections" | "System" | "Math" | "Optional")
}

// ---------------------------------------------------------------------------
// C / C++ parser
// ---------------------------------------------------------------------------

fn parse_c_cpp(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let inc_re    = re!(r#"^\s*#include\s+[<"]([^>"]+)[>"]"#);
    let define_re = re!(r"^\s*#define\s+([A-Za-z_]\w*)");
    let struct_re = re!(r"^\s*(?:typedef\s+)?struct\s+([A-Za-z_]\w*)");
    let class_re  = re!(r"^\s*(?:template\s*<[^>]*>\s*)?class\s+([A-Za-z_]\w*)(?:\s*:\s*(?:public|protected|private)\s+([A-Za-z_][\w:]*))?");
    let enum_re   = re!(r"^\s*(?:typedef\s+)?enum\s+(?:class\s+)?([A-Za-z_]\w*)");
    let ns_re     = re!(r"^\s*namespace\s+([A-Za-z_]\w*)");
    // Functions: return_type name(params) { — heuristic: look for word(word) { at low indent
    let fn_re     = re!(r"^(?:static\s+|inline\s+|extern\s+|virtual\s+|const\s+|unsigned\s+|signed\s+)*(?:\w[\w*&: ]*?)\s+\*?([a-zA-Z_]\w*)\s*\([^;]*$");
    let typedef_re = re!(r"^\s*typedef\s+.*\s+(\w+)\s*;");
    let call_re   = re!(r"\b([a-zA-Z_]\w*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if current_class.is_some() && brace_depth < class_depth {
            current_class = None;
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") { continue; }

        // #include
        if let Some(cap) = inc_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // #define (macro)
        if let Some(cap) = define_re.captures(line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Macro, i as u32, i as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // namespace
        if let Some(cap) = ns_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Module, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // class (C++)
        if language == Language::Cpp {
            if let Some(cap) = class_re.captures(line) {
                let name = cap[1].to_string();
                let end  = find_block_end(lines, i);
                let doc  = extract_doc(lines, i);
                let superclass = cap.get(2).map(|m| m.as_str().trim().to_string());
                let mut mods = Modifiers::default();
                if line.contains("virtual ") { mods.set(mflags::IS_VIRTUAL); }
                defs.push(ParsedDef {
                    name: name.clone(), kind: NodeKind::Class,
                    start: i as u32, end: end as u32,
                    modifiers: mods, doc, container: None,
                    superclass, interfaces: Vec::new(),
                });
                current_class = Some(name);
                class_depth = prev_depth + 1;
                continue;
            }
        }

        // struct
        if let Some(cap) = struct_re.captures(line) {
            let name = cap[1].to_string();
            let end = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name.clone(), NodeKind::Struct, i as u32, end as u32,
                Modifiers::default(), doc, None));
            if line.contains('{') && language == Language::Cpp {
                current_class = Some(name);
                class_depth = prev_depth + 1;
            }
            continue;
        }

        // enum
        if let Some(cap) = enum_re.captures(line) {
            let name = cap[1].to_string();
            let end = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Enum, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // typedef
        if let Some(cap) = typedef_re.captures(line) {
            let name = cap[1].to_string();
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::TypeAlias, i as u32, i as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // Function / method definition (heuristic)
        if line.contains('(') && !line.starts_with('#') && !line.starts_with("//") {
            if let Some(cap) = fn_re.captures(line) {
                let name = cap[1].to_string();
                if !is_c_keyword(&name) {
                    let end = if line.contains('{') { find_block_end(lines, i) } else { i };
                    let doc = extract_doc(lines, i);
                    let kind = if current_class.is_some() { NodeKind::Method } else { NodeKind::Function };
                    let mut mods = Modifiers::default();
                    if line.contains("static ") { mods.set(mflags::IS_STATIC); }
                    if line.contains("inline ") { mods.set(mflags::IS_INLINE); }
                    if line.contains("virtual ") { mods.set(mflags::IS_VIRTUAL); }
                    if line.contains("extern ") { mods.set(mflags::IS_EXTERN); }
                    defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                        mods, doc, current_class.clone()));
                }
            }
        }

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_c_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_c_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "while" | "for" | "do" | "switch" | "case" | "break" |
               "continue" | "return" | "goto" | "sizeof" | "typedef" | "struct" |
               "union" | "enum" | "class" | "namespace" | "template" | "virtual" |
               "static" | "extern" | "inline" | "const" | "volatile" | "register" |
               "void" | "int" | "char" | "short" | "long" | "float" | "double" |
               "bool" | "signed" | "unsigned" | "auto" | "size_t" | "NULL" |
               "true" | "false" | "nullptr" | "new" | "delete" | "throw" |
               "try" | "catch" | "public" | "private" | "protected" | "using" |
               "std" | "string" | "vector" | "map" | "set" | "pair" | "make_pair" |
               "printf" | "fprintf" | "sprintf" | "malloc" | "free" | "calloc" |
               "realloc" | "memcpy" | "memset" | "strlen" | "strcmp" | "strcpy")
}

// ---------------------------------------------------------------------------
// C# parser
// ---------------------------------------------------------------------------

fn parse_csharp(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let using_re = re!(r"^\s*using\s+([\w.]+)\s*;");
    let ns_re    = re!(r"^\s*namespace\s+([\w.]+)");
    let cls_re   = re!(r"(?:public\s+|private\s+|protected\s+|internal\s+)?(?:abstract\s+|sealed\s+|static\s+|partial\s+)*class\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s*:\s*([^{]+))?");
    let intf_re  = re!(r"(?:public\s+|internal\s+)?interface\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s*:\s*([^{]+))?");
    let enum_re  = re!(r"(?:public\s+|private\s+|internal\s+)?enum\s+([A-Za-z_]\w*)");
    let struct_re = re!(r"(?:public\s+|private\s+|internal\s+)?(?:readonly\s+)?struct\s+([A-Za-z_]\w*)");
    let mth_re   = re!(r"^\s+(?:public\s+|private\s+|protected\s+|internal\s+)?(?:static\s+|virtual\s+|override\s+|abstract\s+|async\s+|sealed\s+)*(?:[\w<>\[\]?, ]+?)\s+([a-zA-Z_]\w*)\s*\(");
    let prop_re  = re!(r"^\s+(?:public\s+|private\s+|protected\s+|internal\s+)?(?:static\s+|virtual\s+|override\s+|abstract\s+)*(?:[\w<>\[\]?, ]+?)\s+([A-Z][a-zA-Z_]\w*)\s*\{");
    let const_re = re!(r"^\s+(?:public\s+|private\s+)?const\s+\S+\s+([A-Za-z_]\w*)\s*=");
    let call_re  = re!(r"\b([a-zA-Z_]\w*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if current_class.is_some() && brace_depth < class_depth {
            current_class = None;
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") { continue; }

        // using
        if let Some(cap) = using_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // namespace
        if let Some(cap) = ns_re.captures(line) {
            let name = cap[1].to_string();
            let end = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Module, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // Class — with inheritance (C# uses `:` for both base class and interfaces)
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);

            let mut superclass: Option<String> = None;
            let mut interfaces: Vec<String> = Vec::new();
            if let Some(parents_match) = cap.get(2) {
                let parents = parse_parent_list(parents_match.as_str());
                for (idx, p) in parents.iter().enumerate() {
                    if idx == 0 && !p.starts_with('I') {
                        // Convention: interfaces start with 'I' in C#
                        superclass = Some(p.clone());
                    } else {
                        interfaces.push(p.clone());
                    }
                }
            }

            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            if line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
            if line.contains("sealed ") { mods.set(mflags::IS_FINAL); }
            if line.contains("static ") { mods.set(mflags::IS_STATIC); }
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: mods, doc, container: None,
                superclass, interfaces,
            });
            current_class = Some(name);
            class_depth = prev_depth + 1;
            continue;
        }

        // Interface
        if let Some(cap) = intf_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            let mut def = ParsedDef::new(name, NodeKind::Interface, i as u32, end as u32, mods, doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            continue;
        }

        // Enum
        if let Some(cap) = enum_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            defs.push(ParsedDef::new(name, NodeKind::Enum, i as u32, end as u32, mods, doc, None));
            continue;
        }

        // Struct
        if let Some(cap) = struct_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            if line.contains("readonly ") { mods.set(mflags::IS_READONLY); }
            defs.push(ParsedDef::new(name, NodeKind::Struct, i as u32, end as u32, mods, doc, None));
            continue;
        }

        // Constants
        if let Some(cap) = const_re.captures(raw_line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            let mods = Modifiers(0).with(mflags::IS_CONST);
            defs.push(ParsedDef::new(name, NodeKind::Constant, i as u32, i as u32, mods, doc, current_class.clone()));
            continue;
        }

        // Property
        if current_class.is_some() {
            if let Some(cap) = prop_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_csharp_keyword(&name) {
                    let end = find_block_end(lines, i);
                    let doc = extract_doc(lines, i);
                    let mut mods = Modifiers::default();
                    if raw_line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
                    if raw_line.contains("static ") { mods.set(mflags::IS_STATIC); }
                    if raw_line.contains("virtual ") { mods.set(mflags::IS_VIRTUAL); }
                    defs.push(ParsedDef::new(name, NodeKind::Property, i as u32, end as u32,
                        mods, doc, current_class.clone()));
                }
            }
        }

        // Method / constructor
        if current_class.is_some() {
            if let Some(cap) = mth_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_csharp_keyword(&name) {
                    let kind = if Some(&name) == current_class.as_ref() {
                        NodeKind::Constructor
                    } else {
                        NodeKind::Method
                    };
                    let end = find_block_end(lines, i);
                    let doc = extract_doc(lines, i);
                    let mut mods = Modifiers::default();
                    if raw_line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
                    if raw_line.contains("static ") { mods.set(mflags::IS_STATIC); }
                    if raw_line.contains("async ") { mods.set(mflags::IS_ASYNC); }
                    if raw_line.contains("virtual ") { mods.set(mflags::IS_VIRTUAL); }
                    if raw_line.contains("override ") { mods.set(mflags::IS_OVERRIDE); }
                    if raw_line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
                    defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                        mods, doc, current_class.clone()));
                }
            }
        }

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_csharp_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_csharp_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "while" | "for" | "foreach" | "do" | "switch" |
               "case" | "break" | "continue" | "return" | "throw" | "try" | "catch" |
               "finally" | "new" | "typeof" | "sizeof" | "is" | "as" | "in" |
               "void" | "class" | "struct" | "interface" | "enum" | "namespace" |
               "using" | "public" | "private" | "protected" | "internal" | "static" |
               "virtual" | "override" | "abstract" | "sealed" | "partial" | "async" |
               "await" | "var" | "const" | "readonly" | "this" | "base" |
               "true" | "false" | "null" | "string" | "int" | "long" | "bool" |
               "float" | "double" | "decimal" | "char" | "byte" | "object" |
               "Task" | "Console" | "String" | "Int32" | "List" | "Dictionary")
}

// ---------------------------------------------------------------------------
// Ruby parser
// ---------------------------------------------------------------------------

fn parse_ruby(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let req_re    = re!(r#"^\s*require(?:_relative)?\s+['"]([^'"]+)['"]"#);
    let class_re  = re!(r"^(\s*)class\s+([A-Za-z_]\w*)(?:\s*<\s*([A-Za-z_][\w:]*))?");
    let module_re = re!(r"^(\s*)module\s+([A-Za-z_]\w*)");
    let def_re    = re!(r"^(\s*)def\s+(self\.)?([a-zA-Z_]\w*[!?=]?)");
    let const_re  = re!(r"^\s*([A-Z][A-Z0-9_]+)\s*=");
    let attr_re   = re!(r"^\s*attr_(?:accessor|reader|writer)\s+:(\w+)");
    let include_re = re!(r"^\s*include\s+([A-Za-z_]\w*)");
    let call_re   = re!(r"\b([a-zA-Z_]\w*)\s*[\(]");

    let mut defs: Vec<ParsedDef>  = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<(String, usize)> = None;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.as_str();
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }

        // Pop class if indent decreased
        if let Some((_, class_indent)) = &current_class {
            let this_indent = line.len() - line.trim_start().len();
            if !trimmed.is_empty() && this_indent <= *class_indent && trimmed == "end" {
                current_class = None;
                continue;
            }
        }

        // require
        if let Some(cap) = req_re.captures(trimmed) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // include Module (mixin = interface equivalent)
        if let Some(cap) = include_re.captures(trimmed) {
            if let Some((ref cname, _)) = current_class {
                // Find the class def and add to its interfaces
                for d in defs.iter_mut().rev() {
                    if d.name == *cname && d.kind == NodeKind::Class {
                        d.interfaces.push(cap[1].to_string());
                        break;
                    }
                }
            }
            continue;
        }

        // class
        if let Some(cap) = class_re.captures(trimmed) {
            let indent = cap[1].len();
            let name = cap[2].to_string();
            let end = find_end_keyword_block(lines, i);
            let doc = extract_doc(lines, i);
            let superclass = cap.get(3).map(|m| m.as_str().to_string());
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None,
                superclass, interfaces: Vec::new(),
            });
            current_class = Some((name, indent));
            continue;
        }

        // module
        if let Some(cap) = module_re.captures(trimmed) {
            let _indent = cap[1].len();
            let name = cap[2].to_string();
            let end = find_end_keyword_block(lines, i);
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Module, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // def
        if let Some(cap) = def_re.captures(trimmed) {
            let _indent = cap[1].len();
            let is_static = cap.get(2).is_some();
            let name = cap[3].to_string();
            let container = current_class.as_ref().map(|(n, _)| n.clone());
            let kind = if name == "initialize" && container.is_some() {
                NodeKind::Constructor
            } else if container.is_some() {
                NodeKind::Method
            } else {
                NodeKind::Function
            };
            let end = find_end_keyword_block(lines, i);
            let doc = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if is_static { mods.set(mflags::IS_STATIC); }
            defs.push(ParsedDef::new(name, kind, i as u32, end as u32, mods, doc, container));
            continue;
        }

        // Constants
        if let Some(cap) = const_re.captures(trimmed) {
            let name = cap[1].to_string();
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Constant, i as u32, i as u32,
                Modifiers::default(), doc, current_class.as_ref().map(|(n, _)| n.clone())));
            continue;
        }

        // attr_accessor → Property
        if let Some(cap) = attr_re.captures(trimmed) {
            let name = cap[1].to_string();
            defs.push(ParsedDef::new(name, NodeKind::Property, i as u32, i as u32,
                Modifiers(0).with(mflags::IS_PUBLIC), None, current_class.as_ref().map(|(n, _)| n.clone())));
            continue;
        }

        // Calls
        for cap in call_re.captures_iter(trimmed) {
            let name = &cap[1];
            if !is_ruby_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_ruby_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "elsif" | "unless" | "while" | "until" | "for" |
               "do" | "end" | "def" | "class" | "module" | "return" | "yield" |
               "begin" | "rescue" | "ensure" | "raise" | "require" | "include" |
               "extend" | "attr_accessor" | "attr_reader" | "attr_writer" |
               "puts" | "print" | "p" | "self" | "super" | "true" | "false" |
               "nil" | "lambda" | "proc" | "new" | "each" | "map" | "select" |
               "reject" | "reduce" | "inject" | "collect" | "detect" | "find" |
               "Integer" | "String" | "Array" | "Hash" | "Symbol" | "Float")
}

// ---------------------------------------------------------------------------
// Kotlin parser
// ---------------------------------------------------------------------------

fn parse_kotlin(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let imp_re    = re!(r"^\s*import\s+([\w.]+)");
    let cls_re    = re!(r"(?:open\s+|abstract\s+|sealed\s+|data\s+|inner\s+)*class\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s*(?:\(.*?\))?\s*:\s*([^{]+))?");
    let intf_re   = re!(r"interface\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s*:\s*([^{]+))?");
    let obj_re    = re!(r"(?:companion\s+)?object\s+([A-Za-z_]\w*)");
    let fun_re    = re!(r"^\s*(?:(?:public|private|protected|internal|override|open|abstract|suspend|inline|infix|operator|tailrec)\s+)*fun\s+(?:<[^>]*>\s+)?([a-zA-Z_]\w*)\s*\(");
    let enum_re   = re!(r"enum\s+class\s+([A-Za-z_]\w*)");
    let const_re  = re!(r"^\s*(?:const\s+)?val\s+([A-Z][A-Z0-9_]+)\s*[=:]");
    let prop_re   = re!(r"^\s+(?:(?:public|private|protected|override|open|lateinit|lazy)\s+)*(?:val|var)\s+([a-zA-Z_]\w*)\s*[=:]");
    let call_re   = re!(r"\b([a-zA-Z_]\w*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if current_class.is_some() && brace_depth < class_depth {
            current_class = None;
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") { continue; }

        // Import
        if let Some(cap) = imp_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // Enum class
        if let Some(cap) = enum_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Enum, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // Class (data class, sealed class, etc.)
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc  = extract_doc(lines, i);

            // In Kotlin, inheritance list after `:` mixes superclass and interfaces
            let mut superclass: Option<String> = None;
            let mut interfaces: Vec<String> = Vec::new();
            if let Some(parents_match) = cap.get(2) {
                let parents = parse_parent_list(parents_match.as_str());
                for p in &parents {
                    // Heuristic: superclass if it has `()` call (constructor invocation)
                    if parents_match.as_str().contains(&format!("{}(", p)) {
                        superclass = Some(p.clone());
                    } else {
                        interfaces.push(p.clone());
                    }
                }
                // If no superclass detected and first parent exists, treat first as superclass
                if superclass.is_none() && !parents.is_empty() {
                    superclass = Some(parents[0].clone());
                    interfaces = parents[1..].to_vec();
                }
            }

            let mut mods = Modifiers::default();
            if line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
            if line.contains("data ") { mods.set(mflags::IS_FINAL); }
            if line.contains("sealed ") { mods.set(mflags::IS_FINAL); }
            if line.contains("open ") { mods.set(mflags::IS_VIRTUAL); }
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: mods, doc, container: None,
                superclass, interfaces,
            });
            if line.contains('{') {
                current_class = Some(name);
                class_depth = prev_depth + 1;
            }
            continue;
        }

        // Interface
        if let Some(cap) = intf_re.captures(line) {
            let name = cap[1].to_string();
            let end  = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut def = ParsedDef::new(name, NodeKind::Interface, i as u32, end as u32,
                Modifiers::default(), doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            continue;
        }

        // Object
        if let Some(cap) = obj_re.captures(line) {
            let name = cap[1].to_string();
            let end  = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Class, i as u32, end as u32,
                Modifiers(0).with(mflags::IS_STATIC), doc, None));
            continue;
        }

        // fun (function/method)
        if let Some(cap) = fun_re.captures(raw_line) {
            let name = cap[1].to_string();
            if !is_kotlin_keyword(&name) {
                let kind = if current_class.is_some() { NodeKind::Method } else { NodeKind::Function };
                let end = find_block_end(lines, i);
                let doc = extract_doc(lines, i);
                let mut mods = Modifiers::default();
                if raw_line.contains("suspend ") { mods.set(mflags::IS_ASYNC); }
                if raw_line.contains("override ") { mods.set(mflags::IS_OVERRIDE); }
                if raw_line.contains("open ") { mods.set(mflags::IS_VIRTUAL); }
                if raw_line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
                if raw_line.contains("inline ") { mods.set(mflags::IS_INLINE); }
                if raw_line.contains("public ") || !raw_line.contains("private ") { mods.set(mflags::IS_PUBLIC); }
                defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                    mods, doc, current_class.clone()));
            }
            continue;
        }

        // Constant val
        if let Some(cap) = const_re.captures(raw_line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Constant, i as u32, i as u32,
                Modifiers(0).with(mflags::IS_CONST), doc, current_class.clone()));
            continue;
        }

        // Property (val/var in class)
        if current_class.is_some() {
            if let Some(cap) = prop_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_kotlin_keyword(&name) {
                    let doc = extract_doc(lines, i);
                    let mut mods = Modifiers::default();
                    if raw_line.contains("override ") { mods.set(mflags::IS_OVERRIDE); }
                    defs.push(ParsedDef::new(name, NodeKind::Property, i as u32, i as u32,
                        mods, doc, current_class.clone()));
                }
            }
        }

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_kotlin_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_kotlin_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "when" | "while" | "for" | "do" | "return" |
               "break" | "continue" | "throw" | "try" | "catch" | "finally" |
               "fun" | "val" | "var" | "class" | "interface" | "object" | "enum" |
               "import" | "package" | "is" | "as" | "in" | "out" | "by" |
               "this" | "super" | "true" | "false" | "null" | "it" |
               "println" | "print" | "listOf" | "mapOf" | "setOf" | "arrayOf" |
               "mutableListOf" | "mutableMapOf" | "String" | "Int" | "Long" |
               "Boolean" | "Double" | "Float" | "Any" | "Unit" | "Nothing")
}

// ---------------------------------------------------------------------------
// Swift parser
// ---------------------------------------------------------------------------

fn parse_swift(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let imp_re    = re!(r"^\s*import\s+(\w+)");
    let cls_re    = re!(r"(?:open\s+|public\s+|internal\s+|private\s+|fileprivate\s+)?(?:final\s+)?class\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s*:\s*([^{]+))?");
    let struct_re = re!(r"(?:public\s+|internal\s+|private\s+)?struct\s+([A-Za-z_]\w*)(?:\s*<[^>]*>)?(?:\s*:\s*([^{]+))?");
    let proto_re  = re!(r"(?:public\s+|internal\s+)?protocol\s+([A-Za-z_]\w*)(?:\s*:\s*([^{]+))?");
    let enum_re   = re!(r"(?:public\s+|internal\s+|private\s+)?(?:indirect\s+)?enum\s+([A-Za-z_]\w*)");
    let func_re   = re!(r"^\s*(?:(?:public|private|internal|open|fileprivate|override|static|class|mutating|@objc|@discardableResult)\s+)*func\s+([a-zA-Z_]\w*)\s*[<(]");
    let init_re   = re!(r"^\s*(?:(?:public|private|internal|required|convenience|override)\s+)*init[?(]");
    let prop_re   = re!(r"^\s+(?:(?:public|private|internal|open|static|lazy|weak|unowned)\s+)*(?:let|var)\s+([a-zA-Z_]\w*)\s*[=:]");
    let call_re   = re!(r"\b([a-zA-Z_]\w*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if current_class.is_some() && brace_depth < class_depth {
            current_class = None;
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") { continue; }

        // Import
        if let Some(cap) = imp_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // class — with protocol conformance
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let mut superclass: Option<String> = None;
            let mut interfaces: Vec<String> = Vec::new();
            if let Some(parents_match) = cap.get(2) {
                let parents = parse_parent_list(parents_match.as_str());
                if let Some(first) = parents.first() {
                    // Heuristic: protocols are capitalized words, classes too
                    // In Swift, first in list is typically superclass if it's a class
                    superclass = Some(first.clone());
                }
                for p in parents.iter().skip(1) {
                    interfaces.push(p.clone());
                }
            }
            let mut mods = Modifiers::default();
            if line.contains("public ") || line.contains("open ") { mods.set(mflags::IS_PUBLIC); }
            if line.contains("final ") { mods.set(mflags::IS_FINAL); }
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: mods, doc, container: None,
                superclass, interfaces,
            });
            current_class = Some(name);
            class_depth = prev_depth + 1;
            continue;
        }

        // struct
        if let Some(cap) = struct_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            let mut def = ParsedDef::new(name.clone(), NodeKind::Struct, i as u32, end as u32, mods, doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            current_class = Some(name);
            class_depth = prev_depth + 1;
            continue;
        }

        // protocol
        if let Some(cap) = proto_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            let mut def = ParsedDef::new(name, NodeKind::Interface, i as u32, end as u32, mods, doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            continue;
        }

        // enum
        if let Some(cap) = enum_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            defs.push(ParsedDef::new(name, NodeKind::Enum, i as u32, end as u32, mods, doc, None));
            continue;
        }

        // init (constructor)
        if init_re.is_match(line) {
            let end = find_block_end(lines, i);
            let doc = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
            let name = "init".to_string();
            defs.push(ParsedDef::new(name, NodeKind::Constructor, i as u32, end as u32,
                mods, doc, current_class.clone()));
            continue;
        }

        // func
        if let Some(cap) = func_re.captures(raw_line) {
            let name = cap[1].to_string();
            if !is_swift_keyword(&name) {
                let kind = if current_class.is_some() { NodeKind::Method } else { NodeKind::Function };
                let end = find_block_end(lines, i);
                let doc = extract_doc(lines, i);
                let mut mods = Modifiers::default();
                if raw_line.contains("public ") || raw_line.contains("open ") { mods.set(mflags::IS_PUBLIC); }
                if raw_line.contains("static ") || raw_line.contains("class ") { mods.set(mflags::IS_STATIC); }
                if raw_line.contains("override ") { mods.set(mflags::IS_OVERRIDE); }
                if raw_line.contains("mutating ") { mods.set(mflags::IS_MUT); }
                defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                    mods, doc, current_class.clone()));
            }
            continue;
        }

        // Property (let/var in class/struct)
        if current_class.is_some() {
            if let Some(cap) = prop_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_swift_keyword(&name) {
                    let doc = extract_doc(lines, i);
                    let mut mods = Modifiers::default();
                    if raw_line.contains("static ") { mods.set(mflags::IS_STATIC); }
                    if raw_line.contains("let ") { mods.set(mflags::IS_READONLY); }
                    defs.push(ParsedDef::new(name, NodeKind::Property, i as u32, i as u32,
                        mods, doc, current_class.clone()));
                }
            }
        }

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_swift_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_swift_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "guard" | "while" | "for" | "repeat" | "switch" |
               "case" | "break" | "continue" | "return" | "throw" | "try" | "catch" |
               "func" | "class" | "struct" | "enum" | "protocol" | "extension" |
               "import" | "let" | "var" | "init" | "deinit" | "self" | "Self" |
               "super" | "true" | "false" | "nil" | "is" | "as" | "in" |
               "print" | "debugPrint" | "fatalError" | "precondition" |
               "String" | "Int" | "Double" | "Float" | "Bool" | "Array" |
               "Dictionary" | "Set" | "Optional" | "Result" | "Error" | "Any")
}

// ---------------------------------------------------------------------------
// PHP parser
// ---------------------------------------------------------------------------

fn parse_php(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let use_re     = re!(r"^\s*use\s+([\w\\]+)");
    let require_re = re!(r#"^\s*(?:require|require_once|include|include_once)\s+['"]([^'"]+)['"]"#);
    let cls_re     = re!(r"(?:abstract\s+|final\s+)?class\s+([A-Za-z_]\w*)(?:\s+extends\s+([A-Za-z_][\w\\]*))?(?:\s+implements\s+([^{]+))?");
    let intf_re    = re!(r"interface\s+([A-Za-z_]\w*)(?:\s+extends\s+([^{]+))?");
    let trait_re   = re!(r"trait\s+([A-Za-z_]\w*)");
    let fn_re      = re!(r"^\s*(?:(?:public|private|protected|static|abstract|final)\s+)*function\s+([a-zA-Z_]\w*)\s*\(");
    let const_re   = re!(r"^\s*(?:(?:public|private|protected)\s+)?const\s+([A-Za-z_]\w*)\s*=");
    let ns_re      = re!(r"^\s*namespace\s+([\w\\]+)");
    let call_re    = re!(r"\b([a-zA-Z_]\w*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if current_class.is_some() && brace_depth < class_depth {
            current_class = None;
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") || line.starts_with("<?") { continue; }

        // use
        if let Some(cap) = use_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }
        // require/include
        if let Some(cap) = require_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }
        // namespace
        if let Some(cap) = ns_re.captures(line) {
            let name = cap[1].to_string();
            let end = if line.contains('{') { find_block_end(lines, i) } else { i };
            defs.push(ParsedDef::new(name, NodeKind::Module, i as u32, end as u32,
                Modifiers::default(), None, None));
            continue;
        }

        // class
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let superclass = cap.get(2).map(|m| m.as_str().trim().to_string());
            let interfaces: Vec<String> = cap.get(3)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut mods = Modifiers::default();
            if line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
            if line.contains("final ") { mods.set(mflags::IS_FINAL); }
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: mods, doc, container: None,
                superclass, interfaces,
            });
            current_class = Some(name);
            class_depth = prev_depth + 1;
            continue;
        }

        // interface
        if let Some(cap) = intf_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut def = ParsedDef::new(name, NodeKind::Interface, i as u32, end as u32,
                Modifiers::default(), doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            continue;
        }

        // trait
        if let Some(cap) = trait_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Trait, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // const
        if let Some(cap) = const_re.captures(raw_line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Constant, i as u32, i as u32,
                Modifiers(0).with(mflags::IS_CONST), doc, current_class.clone()));
            continue;
        }

        // function / method
        if let Some(cap) = fn_re.captures(raw_line) {
            let name = cap[1].to_string();
            if !is_php_keyword(&name) {
                let kind = if name == "__construct" && current_class.is_some() {
                    NodeKind::Constructor
                } else if current_class.is_some() {
                    NodeKind::Method
                } else {
                    NodeKind::Function
                };
                let end = find_block_end(lines, i);
                let doc = extract_doc(lines, i);
                let mut mods = Modifiers::default();
                if raw_line.contains("public ") { mods.set(mflags::IS_PUBLIC); }
                if raw_line.contains("static ") { mods.set(mflags::IS_STATIC); }
                if raw_line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
                if raw_line.contains("final ") { mods.set(mflags::IS_FINAL); }
                defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                    mods, doc, current_class.clone()));
            }
            continue;
        }

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_php_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_php_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "elseif" | "while" | "for" | "foreach" | "do" |
               "switch" | "case" | "break" | "continue" | "return" | "throw" |
               "try" | "catch" | "finally" | "function" | "class" | "interface" |
               "trait" | "extends" | "implements" | "use" | "namespace" | "require" |
               "include" | "require_once" | "include_once" | "new" | "echo" | "print" |
               "static" | "public" | "private" | "protected" | "abstract" | "final" |
               "const" | "var" | "this" | "self" | "parent" | "true" | "false" | "null" |
               "array" | "isset" | "unset" | "empty" | "die" | "exit" |
               "is_array" | "is_string" | "is_null" | "count" | "strlen" | "substr" |
               "str_replace" | "preg_match" | "implode" | "explode" | "in_array")
}

// ---------------------------------------------------------------------------
// Lua parser
// ---------------------------------------------------------------------------

fn parse_lua(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let req_re    = re!(r#"require\s*\(?\s*['"]([^'"]+)['"]"#);
    let func_re   = re!(r"^\s*(?:local\s+)?function\s+([a-zA-Z_][\w.]*)(?::(\w+))?\s*\(");
    let local_fn  = re!(r"^\s*local\s+function\s+([a-zA-Z_]\w*)\s*\(");
    let assign_fn = re!(r"^\s*(?:local\s+)?([a-zA-Z_][\w.]*)\s*=\s*function\s*\(");
    let call_re   = re!(r"\b([a-zA-Z_]\w*)\s*\(");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();

        if line.is_empty() || line.starts_with("--") { continue; }

        // require
        for cap in req_re.captures_iter(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
        }

        // function ClassName:methodName()
        if let Some(cap) = func_re.captures(line) {
            if let Some(method_match) = cap.get(2) {
                // ClassName:method pattern
                let class_name = cap[1].to_string();
                let method_name = method_match.as_str().to_string();
                let end = find_end_keyword_block(lines, i);
                let doc = extract_doc(lines, i);
                defs.push(ParsedDef::new(method_name, NodeKind::Method, i as u32, end as u32,
                    Modifiers::default(), doc, Some(class_name)));
                continue;
            }
            // Regular function
            let name = cap[1].to_string();
            let end = find_end_keyword_block(lines, i);
            let doc = extract_doc(lines, i);
            // Check if it's a Foo.bar pattern (static method)
            if name.contains('.') {
                let parts: Vec<&str> = name.rsplitn(2, '.').collect();
                defs.push(ParsedDef::new(parts[0].to_string(), NodeKind::Method, i as u32, end as u32,
                    Modifiers(0).with(mflags::IS_STATIC), doc, Some(parts[1].to_string())));
            } else {
                let mut mods = Modifiers::default();
                if line.starts_with("local ") { /* local scope */ } else { mods.set(mflags::IS_PUBLIC); }
                defs.push(ParsedDef::new(name, NodeKind::Function, i as u32, end as u32, mods, doc, None));
            }
            continue;
        }

        // local function name()
        if let Some(cap) = local_fn.captures(line) {
            let name = cap[1].to_string();
            let end = find_end_keyword_block(lines, i);
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Function, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // name = function()
        if let Some(cap) = assign_fn.captures(line) {
            let name = cap[1].to_string();
            let end = find_end_keyword_block(lines, i);
            let doc = extract_doc(lines, i);
            if name.contains('.') {
                let parts: Vec<&str> = name.rsplitn(2, '.').collect();
                defs.push(ParsedDef::new(parts[0].to_string(), NodeKind::Method, i as u32, end as u32,
                    Modifiers::default(), doc, Some(parts[1].to_string())));
            } else {
                defs.push(ParsedDef::new(name, NodeKind::Function, i as u32, end as u32,
                    Modifiers::default(), doc, None));
            }
            continue;
        }

        // Calls
        for cap in call_re.captures_iter(line) {
            let name = &cap[1];
            if !is_lua_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_lua_keyword(s: &str) -> bool {
    matches!(s, "if" | "then" | "else" | "elseif" | "end" | "while" | "do" | "for" |
               "repeat" | "until" | "break" | "return" | "function" | "local" |
               "in" | "not" | "and" | "or" | "true" | "false" | "nil" |
               "require" | "print" | "error" | "assert" | "type" | "tostring" |
               "tonumber" | "pcall" | "xpcall" | "pairs" | "ipairs" | "next" |
               "select" | "unpack" | "table" | "string" | "math" | "io" | "os")
}

// ---------------------------------------------------------------------------
// Scala parser
// ---------------------------------------------------------------------------

fn parse_scala(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let imp_re    = re!(r"^\s*import\s+([\w.{}, _*]+)");
    let cls_re    = re!(r"(?:abstract\s+|sealed\s+|final\s+)?(?:case\s+)?class\s+([A-Za-z_]\w*)(?:\s*\[.*?\])?(?:\s*\(.*?\))?(?:\s+extends\s+(\w[\w.]*))?(?:\s+with\s+(.+?))?(?:\s*\{|$)");
    let trait_re  = re!(r"(?:sealed\s+)?trait\s+([A-Za-z_]\w*)(?:\s*\[.*?\])?(?:\s+extends\s+([^{]+))?");
    let obj_re    = re!(r"(?:case\s+)?object\s+([A-Za-z_]\w*)(?:\s+extends\s+(\w[\w.]*))?(?:\s+with\s+(.+?))?(?:\s*\{|$)");
    let def_re    = re!(r"^\s*(?:(?:override|private|protected|final|lazy|implicit|abstract)\s+)*def\s+([a-zA-Z_]\w*)\s*[\[(]?");
    let val_re    = re!(r"^\s*(?:(?:override|private|protected|final|lazy|implicit)\s+)*(?:val|var)\s+([a-zA-Z_]\w*)\s*[=:]");
    let type_re   = re!(r"^\s*type\s+([A-Za-z_]\w*)");
    let pkg_re    = re!(r"^\s*package\s+([\w.]+)");
    let call_re   = re!(r"\b([a-zA-Z_]\w*)\s*[\(]");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();
    let mut current_class: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut class_depth: i32 = 0;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let raw_line = raw.as_str();

        let net: i32 = raw_line.chars().map(|c| match c { '{' => 1, '}' => -1, _ => 0 }).sum();
        let prev_depth = brace_depth;
        brace_depth += net;
        if current_class.is_some() && brace_depth < class_depth {
            current_class = None;
        }

        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") || line.starts_with("*") { continue; }

        // Package
        if let Some(cap) = pkg_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // Import
        if let Some(cap) = imp_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // trait
        if let Some(cap) = trait_re.captures(line) {
            let name = cap[1].to_string();
            let end  = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc  = extract_doc(lines, i);
            let interfaces: Vec<String> = cap.get(2)
                .map(|m| parse_parent_list(m.as_str()))
                .unwrap_or_default();
            let mut mods = Modifiers::default();
            if line.contains("sealed ") { mods.set(mflags::IS_FINAL); }
            let mut def = ParsedDef::new(name, NodeKind::Trait, i as u32, end as u32, mods, doc, None);
            def.interfaces = interfaces;
            defs.push(def);
            continue;
        }

        // class / case class
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc  = extract_doc(lines, i);
            let superclass = cap.get(2).map(|m| m.as_str().trim().to_string());
            let mut interfaces: Vec<String> = Vec::new();
            // `with Trait1 with Trait2` → parse traits
            if let Some(with_match) = cap.get(3) {
                for trait_name in with_match.as_str().split(" with ") {
                    let t = trait_name.trim();
                    if let Some(ident) = extract_ident(t) {
                        interfaces.push(ident);
                    }
                }
            }
            let mut mods = Modifiers::default();
            if line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
            if line.contains("sealed ") || line.contains("case ") { mods.set(mflags::IS_FINAL); }
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: mods, doc, container: None,
                superclass, interfaces,
            });
            if line.contains('{') {
                current_class = Some(name);
                class_depth = prev_depth + 1;
            }
            continue;
        }

        // object / case object
        if let Some(cap) = obj_re.captures(line) {
            let name = cap[1].to_string();
            let end  = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc  = extract_doc(lines, i);
            let superclass = cap.get(2).map(|m| m.as_str().trim().to_string());
            let mut interfaces: Vec<String> = Vec::new();
            if let Some(with_match) = cap.get(3) {
                for trait_name in with_match.as_str().split(" with ") {
                    let t = trait_name.trim();
                    if let Some(ident) = extract_ident(t) {
                        interfaces.push(ident);
                    }
                }
            }
            defs.push(ParsedDef {
                name: name.clone(), kind: NodeKind::Class,
                start: i as u32, end: end as u32,
                modifiers: Modifiers(0).with(mflags::IS_STATIC), doc, container: None,
                superclass, interfaces,
            });
            if line.contains('{') {
                current_class = Some(name);
                class_depth = prev_depth + 1;
            }
            continue;
        }

        // type alias
        if let Some(cap) = type_re.captures(line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::TypeAlias, i as u32, i as u32,
                Modifiers::default(), doc, None));
            continue;
        }

        // def (function/method)
        if let Some(cap) = def_re.captures(raw_line) {
            let name = cap[1].to_string();
            if !is_scala_keyword(&name) {
                let kind = if current_class.is_some() { NodeKind::Method } else { NodeKind::Function };
                let end = if line.contains('{') { find_block_end(lines, i) } else { i };
                let doc = extract_doc(lines, i);
                let mut mods = Modifiers::default();
                if raw_line.contains("override ") { mods.set(mflags::IS_OVERRIDE); }
                if raw_line.contains("implicit ") { mods.set(mflags::IS_INLINE); }
                if raw_line.contains("final ") { mods.set(mflags::IS_FINAL); }
                if raw_line.contains("abstract ") { mods.set(mflags::IS_ABSTRACT); }
                defs.push(ParsedDef::new(name, kind, i as u32, end as u32,
                    mods, doc, current_class.clone()));
            }
            continue;
        }

        // val / var (property)
        if current_class.is_some() {
            if let Some(cap) = val_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_scala_keyword(&name) {
                    let doc = extract_doc(lines, i);
                    let mut mods = Modifiers::default();
                    if raw_line.contains("override ") { mods.set(mflags::IS_OVERRIDE); }
                    if raw_line.contains("lazy ") { mods.set(mflags::IS_READONLY); }
                    defs.push(ParsedDef::new(name, NodeKind::Property, i as u32, i as u32,
                        mods, doc, current_class.clone()));
                }
            }
        }

        // Calls
        for cap in call_re.captures_iter(raw_line) {
            let name = &cap[1];
            if !is_scala_keyword(name) {
                calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_scala_keyword(s: &str) -> bool {
    matches!(s, "if" | "else" | "while" | "for" | "do" | "match" | "case" |
               "return" | "throw" | "try" | "catch" | "finally" | "yield" |
               "def" | "val" | "var" | "type" | "class" | "trait" | "object" |
               "extends" | "with" | "import" | "package" | "sealed" | "abstract" |
               "final" | "implicit" | "lazy" | "override" | "private" | "protected" |
               "this" | "super" | "new" | "true" | "false" | "null" |
               "println" | "print" | "require" | "assert" |
               "String" | "Int" | "Long" | "Double" | "Float" | "Boolean" |
               "Any" | "Unit" | "Nothing" | "Option" | "Some" | "None" |
               "List" | "Map" | "Set" | "Seq" | "Vector" | "Array" | "Tuple2")
}

// ---------------------------------------------------------------------------
// Bash / Shell parser
// ---------------------------------------------------------------------------

fn parse_bash(path: &str, lines: &[String], language: Language) -> ParsedFile {
    let source_re = re!(r#"^\s*(?:source|\.) (?:['"])?([^'";\s]+)"#);
    let fn_re1    = re!(r"^\s*function\s+([a-zA-Z_]\w*)\s*\(?");
    let fn_re2    = re!(r"^\s*([a-zA-Z_]\w*)\s*\(\s*\)\s*\{?");
    let call_re   = re!(r"\b([a-zA-Z_]\w*)\s");

    let mut defs    = Vec::new();
    let mut imports = Vec::new();
    let mut calls   = Vec::new();

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();

        if line.is_empty() || line.starts_with('#') { continue; }

        // source / .
        if let Some(cap) = source_re.captures(line) {
            imports.push(ParsedImport { target: cap[1].to_string(), line: i as u32 });
            continue;
        }

        // function keyword style
        if let Some(cap) = fn_re1.captures(line) {
            let name = cap[1].to_string();
            let end = find_block_end(lines, i);
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef::new(name, NodeKind::Function, i as u32, end as u32,
                Modifiers::default(), doc, None));
            continue;
        }
        // name() { style
        if let Some(cap) = fn_re2.captures(line) {
            let name = cap[1].to_string();
            if !is_bash_keyword(&name) {
                let end = find_block_end(lines, i);
                let doc = extract_doc(lines, i);
                defs.push(ParsedDef::new(name, NodeKind::Function, i as u32, end as u32,
                    Modifiers::default(), doc, None));
            }
            continue;
        }

        // Simple call detection (first word on non-assignment lines)
        if !line.contains('=') && !line.starts_with("local ") && !line.starts_with("export ") {
            for cap in call_re.captures_iter(line) {
                let name = &cap[1];
                if !is_bash_keyword(name) {
                    calls.push(ParsedCall { target: name.to_string(), line: i as u32 });
                    break; // Only first word as command name
                }
            }
        }
    }

    ParsedFile { path: path.to_string(), language, lines: lines.to_vec(), defs, imports, calls, mutations: Vec::new() }
}

fn is_bash_keyword(s: &str) -> bool {
    matches!(s, "if" | "then" | "else" | "elif" | "fi" | "while" | "do" | "done" |
               "for" | "in" | "case" | "esac" | "function" | "return" | "exit" |
               "break" | "continue" | "shift" | "export" | "local" | "readonly" |
               "declare" | "typeset" | "unset" | "set" | "source" | "eval" |
               "exec" | "echo" | "printf" | "read" | "test" | "true" | "false" |
               "cd" | "pwd" | "ls" | "cp" | "mv" | "rm" | "mkdir" | "rmdir" |
               "cat" | "grep" | "sed" | "awk" | "find" | "xargs" | "sort" |
               "uniq" | "wc" | "head" | "tail" | "cut" | "tr" | "tee" | "chmod")
}

// ---------------------------------------------------------------------------
// Shared helper: extract first identifier from a string
// ---------------------------------------------------------------------------

fn extract_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    if s.is_empty() { return None; }
    let end = s.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(s.len());
    if end == 0 { return None; }
    Some(s[..end].to_string())
}
