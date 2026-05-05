//! Per-language LSP server registry. Each entry says how to spawn the
//! server, what `languageId` to advertise to it, and which file extensions
//! or file names should map to it.
//!
//! forge-osh ships a broad built-in registry and also loads
//! `~/.forge-osh/lsp.toml` so users can add or override language servers
//! without rebuilding the app.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ServerSpec {
    /// Symbolic key (also used by `/lsp shutdown <language>`).
    pub language: String,
    /// The LSP `languageId` used in didOpen.
    pub language_id: String,
    /// File extensions (lowercase, no dot) that route to this server.
    pub extensions: Vec<String>,
    /// Exact file names that route to this server (e.g. Dockerfile).
    pub file_names: Vec<String>,
    /// Candidate executables to try, in order. The first on PATH wins.
    pub candidates: Vec<ServerCandidate>,
    /// Project marker filenames used to detect the language root.
    pub root_markers: Vec<String>,
    /// Human-friendly install hint shown by `/lsp`.
    pub install_hint: String,
    /// `built-in` or `user`.
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ServerCandidate {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InstallCommand {
    pub program: String,
    pub args: Vec<String>,
    pub display: String,
}

#[derive(Debug, Deserialize)]
struct UserLspConfig {
    #[serde(default)]
    servers: Vec<UserServerSpec>,
}

#[derive(Debug, Deserialize)]
struct UserServerSpec {
    language: String,
    #[serde(default)]
    language_id: Option<String>,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    file_names: Vec<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    candidates: Vec<UserServerCandidate>,
    #[serde(default)]
    root_markers: Vec<String>,
    #[serde(default)]
    install_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserServerCandidate {
    program: String,
    #[serde(default)]
    args: Vec<String>,
}

fn spec(
    language: &str,
    language_id: &str,
    extensions: &[&str],
    file_names: &[&str],
    candidates: &[(&str, &[&str])],
    root_markers: &[&str],
    install_hint: &str,
) -> ServerSpec {
    ServerSpec {
        language: language.to_string(),
        language_id: language_id.to_string(),
        extensions: extensions.iter().map(|s| s.to_ascii_lowercase()).collect(),
        file_names: file_names.iter().map(|s| s.to_string()).collect(),
        candidates: candidates
            .iter()
            .map(|(program, args)| ServerCandidate {
                program: (*program).to_string(),
                args: args.iter().map(|a| (*a).to_string()).collect(),
            })
            .collect(),
        root_markers: root_markers.iter().map(|s| (*s).to_string()).collect(),
        install_hint: install_hint.to_string(),
        source: "built-in".to_string(),
    }
}

/// Built-in registry of supported language servers.
pub fn builtin_servers() -> Vec<ServerSpec> {
    vec![
        spec(
            "rust",
            "rust",
            &["rs"],
            &[],
            &[("rust-analyzer", &[])],
            &["Cargo.toml", "rust-project.json"],
            "rustup component add rust-analyzer",
        ),
        spec(
            "typescript",
            "typescript",
            &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
            &[],
            &[("typescript-language-server", &["--stdio"])],
            &["package.json", "tsconfig.json", "jsconfig.json"],
            "forge-managed npm install: typescript + typescript-language-server",
        ),
        spec(
            "python",
            "python",
            &["py", "pyi"],
            &[],
            &[
                ("pyright-langserver", &["--stdio"]),
                ("pylsp", &[]),
                ("jedi-language-server", &[]),
            ],
            &["pyproject.toml", "setup.py", "requirements.txt", "Pipfile"],
            "forge-managed npm install: pyright",
        ),
        spec(
            "go",
            "go",
            &["go"],
            &[],
            &[("gopls", &[])],
            &["go.mod", "go.work"],
            "go install golang.org/x/tools/gopls@latest",
        ),
        spec(
            "c_cpp",
            "cpp",
            &["c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx"],
            &[],
            &[("clangd", &[])],
            &[
                "compile_commands.json",
                "compile_flags.txt",
                ".clangd",
                "CMakeLists.txt",
            ],
            "Install clangd from LLVM or your package manager",
        ),
        spec(
            "java",
            "java",
            &["java"],
            &[],
            &[("jdtls", &[])],
            &["pom.xml", "build.gradle", "settings.gradle", "gradlew"],
            "Install Eclipse JDT Language Server (`jdtls`)",
        ),
        spec(
            "csharp",
            "csharp",
            &["cs"],
            &[],
            &[("csharp-ls", &[]), ("omnisharp", &["--languageserver"])],
            &["*.sln", "*.csproj", "global.json"],
            "dotnet tool install --global csharp-ls",
        ),
        spec(
            "php",
            "php",
            &["php"],
            &[],
            &[("intelephense", &["--stdio"])],
            &["composer.json", "index.php"],
            "forge-managed npm install: intelephense",
        ),
        spec(
            "ruby",
            "ruby",
            &["rb"],
            &["Gemfile", "Rakefile"],
            &[("ruby-lsp", &[]), ("solargraph", &["stdio"])],
            &["Gemfile", ".ruby-version"],
            "gem install ruby-lsp  # or: gem install solargraph",
        ),
        spec(
            "lua",
            "lua",
            &["lua"],
            &[],
            &[("lua-language-server", &[])],
            &[".luarc.json", ".luarc.jsonc"],
            "Install lua-language-server",
        ),
        spec(
            "bash",
            "shellscript",
            &["sh", "bash", "zsh"],
            &[".bashrc", ".zshrc"],
            &[("bash-language-server", &["start"])],
            &[".git"],
            "forge-managed npm install: bash-language-server",
        ),
        spec(
            "json",
            "json",
            &["json", "jsonc"],
            &[],
            &[("vscode-json-language-server", &["--stdio"])],
            &["package.json"],
            "forge-managed npm install: vscode-langservers-extracted",
        ),
        spec(
            "yaml",
            "yaml",
            &["yaml", "yml"],
            &[],
            &[("yaml-language-server", &["--stdio"])],
            &[".yamllint", ".github"],
            "forge-managed npm install: yaml-language-server",
        ),
        spec(
            "html",
            "html",
            &["html", "htm"],
            &[],
            &[("vscode-html-language-server", &["--stdio"])],
            &["package.json"],
            "forge-managed npm install: vscode-langservers-extracted",
        ),
        spec(
            "css",
            "css",
            &["css", "scss", "sass", "less"],
            &[],
            &[("vscode-css-language-server", &["--stdio"])],
            &["package.json"],
            "forge-managed npm install: vscode-langservers-extracted",
        ),
        spec(
            "vue",
            "vue",
            &["vue"],
            &[],
            &[("vue-language-server", &["--stdio"])],
            &["package.json", "vite.config.ts", "vue.config.js"],
            "forge-managed npm install: @vue/language-server",
        ),
        spec(
            "svelte",
            "svelte",
            &["svelte"],
            &[],
            &[("svelteserver", &["--stdio"])],
            &["package.json", "svelte.config.js"],
            "forge-managed npm install: svelte-language-server",
        ),
        spec(
            "kotlin",
            "kotlin",
            &["kt", "kts"],
            &[],
            &[("kotlin-language-server", &[])],
            &["build.gradle", "settings.gradle", "pom.xml"],
            "Install kotlin-language-server",
        ),
        spec(
            "swift",
            "swift",
            &["swift"],
            &[],
            &[("sourcekit-lsp", &[])],
            &["Package.swift"],
            "Install Xcode command line tools / sourcekit-lsp",
        ),
        spec(
            "dart",
            "dart",
            &["dart"],
            &[],
            &[("dart", &["language-server", "--protocol=lsp"])],
            &["pubspec.yaml"],
            "Install Dart SDK",
        ),
        spec(
            "dockerfile",
            "dockerfile",
            &["dockerfile"],
            &["Dockerfile", "Containerfile"],
            &[("docker-langserver", &["--stdio"])],
            &["Dockerfile", "Containerfile"],
            "forge-managed npm install: dockerfile-language-server-nodejs",
        ),
    ]
}

/// Load built-ins plus user overrides from `~/.forge-osh/lsp.toml`.
pub fn load_server_specs() -> Vec<ServerSpec> {
    let mut by_language: BTreeMap<String, ServerSpec> = builtin_servers()
        .into_iter()
        .map(|s| (s.language.clone(), s))
        .collect();

    for user in load_user_servers() {
        if let Some(spec) = user_spec_to_server_spec(user) {
            by_language.insert(spec.language.clone(), spec);
        }
    }

    by_language.into_values().collect()
}

/// Best-effort installer for built-in languages. This intentionally returns a
/// command for forge-osh to run after explicit user confirmation in the TUI,
/// avoiding manual copy/paste while still not silently modifying the machine.
pub fn install_command_for_language(language: &str) -> Option<InstallCommand> {
    if let Some(packages) = npm_packages_for_language(language) {
        let prefix = managed_node_root().to_string_lossy().to_string();
        let mut args = vec!["install".to_string(), "--prefix".to_string(), prefix];
        args.extend(packages.iter().map(|p| (*p).to_string()));
        return Some(InstallCommand {
            program: "npm".to_string(),
            display: format!("npm {}", args.join(" ")),
            args,
        });
    }

    let (program, args): (&str, &[&str]) = match language {
        "rust" => ("rustup", &["component", "add", "rust-analyzer"]),
        "go" => ("go", &["install", "golang.org/x/tools/gopls@latest"]),
        "csharp" => ("dotnet", &["tool", "install", "--global", "csharp-ls"]),
        "ruby" => ("gem", &["install", "ruby-lsp"]),
        _ => return None,
    };
    Some(InstallCommand {
        program: program.to_string(),
        args: args.iter().map(|a| (*a).to_string()).collect(),
        display: format!("{program} {}", args.join(" ")),
    })
}

fn npm_packages_for_language(language: &str) -> Option<&'static [&'static str]> {
    match language {
        "typescript" => Some(&["typescript", "typescript-language-server"]),
        "python" => Some(&["pyright"]),
        "php" => Some(&["intelephense"]),
        "bash" => Some(&["bash-language-server"]),
        "json" | "html" | "css" => Some(&["vscode-langservers-extracted"]),
        "yaml" => Some(&["yaml-language-server"]),
        "vue" => Some(&["@vue/language-server"]),
        "svelte" => Some(&["svelte-language-server"]),
        "dockerfile" => Some(&["dockerfile-language-server-nodejs"]),
        _ => None,
    }
}

fn load_user_servers() -> Vec<UserServerSpec> {
    let path = std::env::var("FORGE_LSP_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::config::config_dir().join("lsp.toml"));
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    toml::from_str::<UserLspConfig>(&text)
        .map(|c| c.servers)
        .unwrap_or_default()
}

fn user_spec_to_server_spec(user: UserServerSpec) -> Option<ServerSpec> {
    let mut candidates: Vec<ServerCandidate> = user
        .candidates
        .into_iter()
        .map(|c| ServerCandidate {
            program: c.program,
            args: c.args,
        })
        .collect();
    if let Some(command) = user.command {
        candidates.insert(
            0,
            ServerCandidate {
                program: command,
                args: user.args,
            },
        );
    }
    if candidates.is_empty() {
        return None;
    }

    let language = user.language.trim().to_ascii_lowercase();
    if language.is_empty() {
        return None;
    }
    Some(ServerSpec {
        language: language.clone(),
        language_id: user.language_id.unwrap_or(language),
        extensions: user
            .extensions
            .into_iter()
            .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect(),
        file_names: user.file_names,
        candidates,
        root_markers: user.root_markers,
        install_hint: user
            .install_hint
            .unwrap_or_else(|| "Install the configured language server on PATH.".to_string()),
        source: "user".to_string(),
    })
}

/// Find the registered server spec for a given file path.
pub fn server_for_path<'a>(path: &Path, specs: &'a [ServerSpec]) -> Option<&'a ServerSpec> {
    let file_name = path.file_name()?.to_string_lossy();
    for spec in specs {
        if spec.file_names.iter().any(|name| name == &*file_name) {
            return Some(spec);
        }
    }

    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    specs
        .iter()
        .find(|spec| spec.extensions.iter().any(|e| e == &ext))
}

/// Find the registered server spec by language key (e.g. "rust").
pub fn server_for_language<'a>(lang: &str, specs: &'a [ServerSpec]) -> Option<&'a ServerSpec> {
    let lang = lang.trim().to_ascii_lowercase();
    specs.iter().find(|s| s.language == lang)
}

/// Pick the first candidate executable that exists on PATH.
pub fn resolve_candidate(spec: &ServerSpec) -> Option<ServerCandidate> {
    for cand in &spec.candidates {
        if let Some(program) = resolve_program(&cand.program) {
            let mut resolved = cand.clone();
            resolved.program = program.to_string_lossy().to_string();
            return Some(resolved);
        }
    }
    None
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let program_path = Path::new(program);
    if (program_path.is_absolute() || program.contains(std::path::MAIN_SEPARATOR))
        && program_path.exists()
    {
        return Some(program_path.to_path_buf());
    }

    for candidate in managed_program_paths(program) {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    which::which(program).ok()
}

fn managed_program_paths(program: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.join("lsp").join("bin"));
            dirs.push(
                dir.join("lsp")
                    .join("node")
                    .join("node_modules")
                    .join(".bin"),
            );
        }
    }
    dirs.push(crate::config::data_dir().join("lsp").join("bin"));
    dirs.push(managed_node_root().join("node_modules").join(".bin"));

    let mut paths = Vec::new();
    for dir in dirs {
        for exe_name in executable_names(program) {
            paths.push(dir.join(exe_name));
        }
    }
    paths
}

fn executable_names(program: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{program}.exe"),
            format!("{program}.cmd"),
            format!("{program}.bat"),
            program.to_string(),
        ]
    } else {
        vec![program.to_string()]
    }
}

fn managed_node_root() -> PathBuf {
    crate::config::data_dir().join("lsp").join("node")
}

/// Walk up from `start` looking for any marker. Falls back to `start`.
pub fn detect_project_root(start: &Path, markers: &[String]) -> PathBuf {
    let mut cur = start.to_path_buf();
    if cur.is_file() {
        if let Some(p) = cur.parent() {
            cur = p.to_path_buf();
        }
    }
    let original = cur.clone();
    loop {
        for marker in markers {
            if marker.contains('*') {
                if glob::glob(cur.join(marker).to_string_lossy().as_ref())
                    .map(|mut g| g.any(|p| p.is_ok()))
                    .unwrap_or(false)
                {
                    return cur;
                }
            } else if cur.join(marker).exists() {
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

/// Convert a filesystem path to a `file://` URI. Cross-platform, handling
/// Windows drive letters and percent-encoding of unsafe characters.
pub fn path_to_uri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches("//?/").to_string();
    let mut out = String::from("file://");
    if s.chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
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
pub fn uri_to_path(uri: &str) -> PathBuf {
    let s = uri.strip_prefix("file://").unwrap_or(uri);
    let s = if s.starts_with('/')
        && s.chars()
            .nth(1)
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
        && s.chars().nth(2) == Some(':')
    {
        &s[1..]
    } else {
        s
    };
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let decoded = String::from_utf8_lossy(&out).into_owned();
    PathBuf::from(decoded)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
