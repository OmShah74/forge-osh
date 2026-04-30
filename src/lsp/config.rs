//! Per-language LSP server registry. Each entry says how to spawn the
//! server, what `languageId` to advertise to it, and which file extensions
//! should map to it.
//!
//! Servers are spawned lazily and only if the executable is present on PATH.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct ServerSpec {
    /// Symbolic key (also doubles as `languageId` unless overridden).
    pub language: &'static str,
    /// The `languageId` field we advertise in didOpen — sometimes differs
    /// from `language` (e.g. `language="ts"`, `language_id="typescript"`).
    pub language_id: &'static str,
    /// File extensions (lowercase, no dot) that route to this server.
    pub extensions: &'static [&'static str],
    /// Candidate executables to try, in order. The first one found on PATH
    /// wins.
    pub candidates: &'static [ServerCandidate],
    /// Project marker filenames that, when present in any ancestor dir,
    /// indicate the project root for that language.
    pub root_markers: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct ServerCandidate {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

/// Built-in registry of supported language servers.
pub fn builtin_servers() -> &'static [ServerSpec] {
    &[
        ServerSpec {
            language: "rust",
            language_id: "rust",
            extensions: &["rs"],
            candidates: &[ServerCandidate {
                program: "rust-analyzer",
                args: &[],
            }],
            root_markers: &["Cargo.toml", "rust-project.json"],
        },
        ServerSpec {
            language: "typescript",
            language_id: "typescript",
            extensions: &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
            candidates: &[
                ServerCandidate {
                    program: "typescript-language-server",
                    args: &["--stdio"],
                },
                ServerCandidate {
                    program: "tsserver",
                    args: &["--stdio"],
                },
            ],
            root_markers: &["package.json", "tsconfig.json", "jsconfig.json"],
        },
        ServerSpec {
            language: "python",
            language_id: "python",
            extensions: &["py", "pyi"],
            candidates: &[
                ServerCandidate {
                    program: "pyright-langserver",
                    args: &["--stdio"],
                },
                ServerCandidate {
                    program: "pylsp",
                    args: &[],
                },
                ServerCandidate {
                    program: "jedi-language-server",
                    args: &[],
                },
            ],
            root_markers: &["pyproject.toml", "setup.py", "requirements.txt", "Pipfile"],
        },
        ServerSpec {
            language: "go",
            language_id: "go",
            extensions: &["go"],
            candidates: &[ServerCandidate {
                program: "gopls",
                args: &[],
            }],
            root_markers: &["go.mod", "go.work"],
        },
    ]
}

/// Find the registered server spec for a given file path (by extension).
pub fn server_for_path(path: &Path) -> Option<&'static ServerSpec> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    for spec in builtin_servers() {
        if spec.extensions.iter().any(|e| **e == *ext) {
            return Some(spec);
        }
    }
    None
}

/// Find the registered server spec by language key (e.g. "rust").
pub fn server_for_language(lang: &str) -> Option<&'static ServerSpec> {
    builtin_servers().iter().find(|s| s.language == lang)
}

/// Pick the first candidate executable that exists on PATH.
pub fn resolve_candidate(spec: &ServerSpec) -> Option<&'static ServerCandidate> {
    for cand in spec.candidates {
        if which::which(cand.program).is_ok() {
            return Some(cand);
        }
    }
    None
}

/// Walk up from `start` looking for any of `markers`. Falls back to `start`.
pub fn detect_project_root(start: &Path, markers: &[&str]) -> std::path::PathBuf {
    let mut cur = start.to_path_buf();
    if cur.is_file() {
        if let Some(p) = cur.parent() {
            cur = p.to_path_buf();
        }
    }
    let original = cur.clone();
    loop {
        for m in markers {
            if cur.join(m).exists() {
                return cur;
            }
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => break,
        }
    }
    original
}

/// Convert a filesystem path to a `file://` URI. Cross-platform — handles
/// Windows drive letters and percent-encoding of unsafe characters.
pub fn path_to_uri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches("//?/").to_string();
    let mut out = String::from("file://");
    // On Windows we need a leading slash before the drive letter.
    if s.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
        && s.chars().nth(1) == Some(':')
    {
        out.push('/');
    } else if !s.starts_with('/') {
        out.push('/');
    }
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' | ':' => out.push(c),
            ' ' => out.push_str("%20"),
            _ => {
                // percent-encode anything else as utf-8 bytes
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    out
}

/// Convert a `file://` URI back to a filesystem path. Best-effort.
pub fn uri_to_path(uri: &str) -> std::path::PathBuf {
    let s = uri.strip_prefix("file://").unwrap_or(uri);
    // Strip leading slash before a Windows drive letter: /C:/foo → C:/foo
    let s = if s.starts_with('/')
        && s.chars().nth(1).map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
        && s.chars().nth(2) == Some(':')
    {
        &s[1..]
    } else {
        s
    };
    // Percent-decode
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                hex_val(bytes[i + 1]),
                hex_val(bytes[i + 2]),
            ) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let decoded = String::from_utf8_lossy(&out).into_owned();
    std::path::PathBuf::from(decoded)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
