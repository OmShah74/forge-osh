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

// ---------------------------------------------------------------------------
// Output types (used by the builder)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedDef {
    pub name:      String,
    pub kind:      NodeKind,
    pub start:     u32,
    pub end:       u32,
    pub modifiers: Modifiers,
    pub doc:       Option<String>,
    /// Parent symbol name (e.g. struct/impl that contains this method)
    pub container: Option<String>,
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

// ---------------------------------------------------------------------------
// Rust parser
// ---------------------------------------------------------------------------

fn parse_rust(path: &str, lines: &[String], language: Language) -> ParsedFile {
    // Regex for use statements
    let use_re = Regex::new(r"^\s*use\s+(.+?);").unwrap();
    // Regex for approximate call sites: word followed by `(`
    let call_re = Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(").unwrap();
    // Regex for `let mut` and bare assignments (mutation detection)
    let let_mut_re = Regex::new(r"\blet\s+mut\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
    let assign_re  = Regex::new(r"^\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*(?:\[.*?\])?\s*=\s*[^=]").unwrap();

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
        return Some(ParsedDef { name, kind, start: line_num as u32, end: end as u32, modifiers: mods, doc, container: container.clone() });
    }
    // struct
    if let Some(r) = rest4.strip_prefix("struct ") {
        let name = extract_ident(r)?;
        let end = if line.contains(';') { line_num } else { find_block_end(all_lines, line_num) };
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::Struct, start: line_num as u32, end: end as u32, modifiers: mods, doc, container: container.clone() });
    }
    // enum
    if let Some(r) = rest4.strip_prefix("enum ") {
        let name = extract_ident(r)?;
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::Enum, start: line_num as u32, end: end as u32, modifiers: mods, doc, container: container.clone() });
    }
    // trait
    if let Some(r) = rest4.strip_prefix("trait ") {
        let name = extract_ident(r)?;
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::Trait, start: line_num as u32, end: end as u32, modifiers: mods, doc, container: container.clone() });
    }
    // impl
    if rest4.starts_with("impl") {
        let name = parse_rust_impl_name(rest4);
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::Impl, start: line_num as u32, end: end as u32, modifiers: mods, doc, container: container.clone() });
    }
    // mod
    if let Some(r) = rest4.strip_prefix("mod ") {
        let name = extract_ident(r)?;
        // Only block mods (with `{`), not `mod foo;`
        if line.contains('{') {
            let end = find_block_end(all_lines, line_num);
            let doc = extract_doc(all_lines, line_num);
            return Some(ParsedDef { name, kind: NodeKind::Module, start: line_num as u32, end: end as u32, modifiers: mods, doc, container: container.clone() });
        }
    }
    // type alias
    if let Some(r) = rest4.strip_prefix("type ") {
        let name = extract_ident(r)?;
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::TypeAlias, start: line_num as u32, end: line_num as u32, modifiers: mods, doc, container: container.clone() });
    }
    // const
    if let Some(r) = rest4.strip_prefix("const ") {
        let name = extract_ident(r)?;
        mods.set(mflags::IS_CONST);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::GlobalVar, start: line_num as u32, end: line_num as u32, modifiers: mods, doc, container: container.clone() });
    }
    // static
    if let Some(r) = rest4.strip_prefix("static ") {
        let r2 = if let Some(r) = r.strip_prefix("mut ") { mods.set(mflags::IS_MUT); r.trim_start() } else { r };
        let name = extract_ident(r2)?;
        mods.set(mflags::IS_STATIC);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::GlobalVar, start: line_num as u32, end: line_num as u32, modifiers: mods, doc, container: container.clone() });
    }
    // macro_rules!
    if let Some(r) = line.strip_prefix("macro_rules! ") {
        let name = extract_ident(r)?;
        let end = find_block_end(all_lines, line_num);
        let doc = extract_doc(all_lines, line_num);
        return Some(ParsedDef { name, kind: NodeKind::Macro, start: line_num as u32, end: end as u32, modifiers: mods, doc, container: container.clone() });
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
    let def_re   = Regex::new(r"^(\s*)(?:(async)\s+)?def\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
    let class_re = Regex::new(r"^(\s*)class\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let imp_re   = Regex::new(r"^\s*import\s+(.+)").unwrap();
    let from_re  = Regex::new(r"^\s*from\s+\S+\s+import\s+(.+)").unwrap();
    let call_re  = Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(").unwrap();

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

        // Class
        if let Some(cap) = class_re.captures(line) {
            let indent = cap[1].len();
            let name   = cap[2].to_string();
            let end = python_block_end(lines, i, indent);
            let doc = extract_doc(lines, i);
            defs.push(ParsedDef { name: name.clone(), kind: NodeKind::Class, start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None });
            current_class = Some((name, indent));
            continue;
        }

        // Function / method
        if let Some(cap) = def_re.captures(line) {
            let indent    = cap[1].len();
            let is_async  = cap.get(2).is_some();
            let name      = cap[3].to_string();
            let container = current_class.as_ref().map(|(n, _)| n.clone());
            let kind      = if container.is_some() { NodeKind::Method } else { NodeKind::Function };
            let end = python_block_end(lines, i, indent);
            let doc = extract_doc(lines, i);
            let mut mods = Modifiers::default();
            if is_async { mods.set(mflags::IS_ASYNC); }
            defs.push(ParsedDef { name, kind, start: i as u32, end: end as u32, modifiers: mods, doc, container });
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
    let fn_re    = Regex::new(r"^(?:export\s+(?:default\s+)?)?(?:async\s+)?function\s*\*?\s+([a-zA-Z_$][a-zA-Z0-9_$]*)").unwrap();
    // Arrow / const fn: const foo = (async )? (...) =>
    let arr_re   = Regex::new(r"^(?:export\s+)?(?:const|let|var)\s+([a-zA-Z_$][a-zA-Z0-9_$]*)\s*=\s*(?:async\s+)?\(.*\)\s*=>").unwrap();
    // Class: class Foo, abstract class Foo
    let cls_re   = Regex::new(r"^(?:export\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)").unwrap();
    // Interface (TS)
    let intf_re  = Regex::new(r"^(?:export\s+)?interface\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    // Type alias (TS)
    let type_re  = Regex::new(r"^(?:export\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)\s*=").unwrap();
    // Method inside class: methodName(...) or async methodName(...)
    let mth_re   = Regex::new(r"^\s+(?:async\s+|static\s+|private\s+|protected\s+|public\s+|readonly\s+)*([a-zA-Z_$][a-zA-Z0-9_$]*)\s*\(").unwrap();
    // Import
    let imp_re   = Regex::new(r#"^import\s+.*?\s+from\s+['"](.+?)['"]"#).unwrap();
    let req_re   = Regex::new(r#"require\(['"](.+?)['"]\)"#).unwrap();
    // Calls
    let call_re  = Regex::new(r"\b([a-zA-Z_$][a-zA-Z0-9_$]*)\s*\(").unwrap();

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

        // Class
        if let Some(cap) = cls_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef { name: name.clone(), kind: NodeKind::Class, start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None });
            current_class = Some(name);
            class_depth = prev_depth + 1;
            continue;
        }
        // Interface (TS)
        if let Some(cap) = intf_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef { name, kind: NodeKind::Interface, start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None });
            continue;
        }
        // Type alias
        if let Some(cap) = type_re.captures(line) {
            let name = cap[1].to_string();
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef { name, kind: NodeKind::TypeAlias, start: i as u32, end: i as u32,
                modifiers: Modifiers::default(), doc, container: None });
            continue;
        }
        // Function declaration
        if let Some(cap) = fn_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            let kind = if current_class.is_some() { NodeKind::Method } else { NodeKind::Function };
            defs.push(ParsedDef { name, kind, start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: current_class.clone() });
            continue;
        }
        // Arrow function
        if let Some(cap) = arr_re.captures(line) {
            let name = cap[1].to_string();
            let end  = find_block_end(lines, i);
            let doc  = extract_doc(lines, i);
            defs.push(ParsedDef { name, kind: NodeKind::Function, start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None });
            continue;
        }
        // Method inside class (indented)
        if current_class.is_some() && raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            if let Some(cap) = mth_re.captures(raw_line) {
                let name = cap[1].to_string();
                if !is_js_keyword(&name) {
                    let end = find_block_end(lines, i);
                    let doc = extract_doc(lines, i);
                    defs.push(ParsedDef { name, kind: NodeKind::Method, start: i as u32, end: end as u32,
                        modifiers: Modifiers::default(), doc, container: current_class.clone() });
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
    let func_re  = Regex::new(r"^func\s+(?:\([^)]*\)\s+)?([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
    let type_re  = Regex::new(r"^type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(struct|interface|func|[A-Za-z])").unwrap();
    let imp_re   = Regex::new(r#"^\s+"?([^"]+)"?\s*$"#).unwrap();
    let call_re  = Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(").unwrap();

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
            defs.push(ParsedDef { name, kind, start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None });
            continue;
        }
        // type
        if let Some(cap) = type_re.captures(line) {
            let name  = cap[1].to_string();
            let kword = &cap[2];
            let kind  = match kword { "struct" => NodeKind::Struct, "interface" => NodeKind::Interface, _ => NodeKind::TypeAlias };
            let end   = if line.contains('{') { find_block_end(lines, i) } else { i };
            let doc   = extract_doc(lines, i);
            defs.push(ParsedDef { name, kind, start: i as u32, end: end as u32,
                modifiers: Modifiers::default(), doc, container: None });
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
// Shared helper: extract first identifier from a string
// ---------------------------------------------------------------------------

fn extract_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    if s.is_empty() { return None; }
    let end = s.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(s.len());
    if end == 0 { return None; }
    Some(s[..end].to_string())
}
