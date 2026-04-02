use std::path::{Path, PathBuf};

/// Build the system prompt dynamically based on environment
pub fn build_system_prompt(working_dir: &Path, extra: &str) -> String {
    let os_name = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let shell_name = if cfg!(target_os = "windows") {
        "cmd.exe / PowerShell".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
    };
    let shell = shell_name.as_str();
    let cwd = working_dir.display();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M %Z");
    let project_context = detect_project_context(working_dir);
    let git_context = build_git_context(working_dir);
    let dir_tree = build_directory_tree(working_dir);
    let memory_content = load_memory_files(working_dir);

    let mut prompt = format!(
        r#"You are forge, a highly capable agentic coding assistant running directly in the terminal.

## Identity
You are forge — powerful, precise, and productive. You help engineers build, debug,
refactor, and understand code at speed.

## Environment
- Operating System: {os_name} ({arch})
- Shell: {shell}
- Working Directory: {cwd}
- Date & Time: {now}

## Project Context
{project_context}

## Git Status
{git_context}

## Directory Structure
{dir_tree}

## How You Work
You operate in an autonomous agentic loop:
1. Understand the user's goal thoroughly before acting
2. For complex, multi-step tasks use enter_plan_mode to propose a plan first
3. Use todo_write to track your work steps for complex tasks
4. Read all relevant code before making changes using read_file
5. Make precise, targeted edits using edit_file (not full rewrites)
6. Verify your changes work (run tests, check for errors with bash)
7. Report what you did and any issues you encountered
8. Ask the user for clarification using ask_user when requirements are ambiguous

## Tool Usage Guidelines
- Prefer reading before writing — understand code before changing it
- Use edit_file for modifications, write_file only for new files
- Use search_files with context lines (-C 3) to find relevant code
- Run bash commands to verify changes (compile, test, lint)
- Search before writing new code — don't duplicate existing logic
- Use git tools when asked to commit, diff, or manage branches
- When fetching web content, extract only what's relevant
- Use todo_write at the start of complex tasks to plan your steps

## Communication Style
- Be concise — don't over-explain
- Show your work — explain significant decisions
- Flag uncertainty — say when you're not sure
- Use ask_user rather than guessing on ambiguous requirements

## Safety Rules
- Never delete files without explicit confirmation
- Never commit API keys, passwords, or secrets
- Always show significant file changes before applying
- Never rm -rf anything without double confirmation
- Prefer reversible actions over irreversible ones

## Response Format
- For simple tasks: act immediately, summarize at end
- For complex tasks: use enter_plan_mode → present plan → exit_plan_mode → execute
- On errors: explain what went wrong → propose fix → ask to proceed
- On completion: brief summary of changes made"#
    );

    if !memory_content.is_empty() {
        prompt.push_str("\n\n## Memory (from CLAUDE.md files)\n");
        prompt.push_str(&memory_content);
    }

    if !extra.is_empty() {
        prompt.push_str("\n\n## Additional Instructions\n");
        prompt.push_str(extra);
    }

    prompt
}

fn detect_project_context(working_dir: &Path) -> String {
    let mut context_parts: Vec<String> = Vec::new();

    if working_dir.join("Cargo.toml").exists() {
        context_parts.push("- Language: Rust (Cargo.toml detected)".to_string());
        if let Ok(content) = std::fs::read_to_string(working_dir.join("Cargo.toml")) {
            if let Some(name) = content
                .lines()
                .find(|l| l.starts_with("name"))
                .and_then(|l| l.split('"').nth(1))
            {
                context_parts.push(format!("- Project: {name}"));
            }
        }
    }
    if working_dir.join("package.json").exists() {
        context_parts.push("- Language: JavaScript/TypeScript (package.json detected)".to_string());
        if let Ok(content) = std::fs::read_to_string(working_dir.join("package.json")) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(name) = pkg["name"].as_str() {
                    context_parts.push(format!("- Project: {name}"));
                }
            }
        }
    }
    if working_dir.join("pyproject.toml").exists() || working_dir.join("setup.py").exists() {
        context_parts.push("- Language: Python".to_string());
    }
    if working_dir.join("go.mod").exists() {
        context_parts.push("- Language: Go (go.mod detected)".to_string());
    }
    if working_dir.join("pom.xml").exists() || working_dir.join("build.gradle").exists() {
        context_parts.push("- Language: Java".to_string());
    }
    if working_dir.join("Gemfile").exists() {
        context_parts.push("- Language: Ruby".to_string());
    }
    if working_dir.join("composer.json").exists() {
        context_parts.push("- Language: PHP".to_string());
    }
    if working_dir.join("CMakeLists.txt").exists() || working_dir.join("Makefile").exists() {
        context_parts.push("- Build: CMake/Make detected".to_string());
    }
    if working_dir.join(".git").exists() {
        context_parts.push("- Version control: Git repository".to_string());
    }

    if context_parts.is_empty() {
        "No specific project structure detected.".to_string()
    } else {
        context_parts.join("\n")
    }
}

/// Build rich git context: branch, last 5 commits, dirty status, staged/unstaged
fn build_git_context(working_dir: &Path) -> String {
    if !working_dir.join(".git").exists() {
        return "Not a git repository.".to_string();
    }

    let mut parts = Vec::new();

    // Branch name
    let head_path = working_dir.join(".git/HEAD");
    if let Ok(content) = std::fs::read_to_string(&head_path) {
        if let Some(branch) = content.strip_prefix("ref: refs/heads/") {
            parts.push(format!("Branch: {}", branch.trim()));
        } else {
            parts.push(format!("Detached HEAD: {}", content.trim().get(..8).unwrap_or("")));
        }
    }

    // Last 5 commits
    if let Ok(output) = std::process::Command::new("git")
        .args(["log", "--oneline", "-5", "--no-decorate"])
        .current_dir(working_dir)
        .output()
    {
        if output.status.success() {
            let log = String::from_utf8_lossy(&output.stdout);
            let commits: Vec<&str> = log.lines().collect();
            if !commits.is_empty() {
                parts.push(format!("Recent commits:\n{}", commits.iter()
                    .map(|c| format!("  {c}"))
                    .collect::<Vec<_>>()
                    .join("\n")));
            }
        }
    }

    // Working tree status (dirty files)
    if let Ok(output) = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(working_dir)
        .output()
    {
        if output.status.success() {
            let status = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = status.lines().take(10).collect();
            if lines.is_empty() {
                parts.push("Working tree: clean".to_string());
            } else {
                parts.push(format!("Working tree changes ({} files):\n{}{}",
                    status.lines().count(),
                    lines.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"),
                    if status.lines().count() > 10 { "\n  ..." } else { "" }
                ));
            }
        }
    }

    if parts.is_empty() {
        "Git repository (details unavailable).".to_string()
    } else {
        parts.join("\n")
    }
}

/// Build a compact directory tree (2 levels deep, respecting .gitignore)
fn build_directory_tree(working_dir: &Path) -> String {
    use ignore::WalkBuilder;

    let mut entries: Vec<String> = Vec::new();
    let max_entries = 40;

    let walker = WalkBuilder::new(working_dir)
        .max_depth(Some(2))
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .build();

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        if entries.len() >= max_entries { break; }
        let path = entry.path();
        if path == working_dir { continue; }

        let relative = path.strip_prefix(working_dir).unwrap_or(path);
        let depth = relative.components().count();
        let indent = "  ".repeat(depth.saturating_sub(1));
        let name = entry.file_name().to_string_lossy();

        // Skip common noise
        if matches!(name.as_ref(), ".git" | "node_modules" | "target" | "__pycache__" | ".venv") {
            continue;
        }

        let suffix = if path.is_dir() { "/" } else { "" };
        entries.push(format!("{indent}{name}{suffix}"));
    }

    if entries.is_empty() {
        return "(empty directory)".to_string();
    }

    let mut result = entries.join("\n");
    if entries.len() >= max_entries {
        result.push_str("\n  ... (truncated)");
    }
    result
}

/// Load all CLAUDE.md files from the working directory tree and user home
fn load_memory_files(working_dir: &Path) -> String {
    let mut sections: Vec<String> = Vec::new();

    // User-level memory: ~/.claude/CLAUDE.md (Claude Code) or ~/.forge-osh/CLAUDE.md
    let user_home = dirs::home_dir().unwrap_or_default();
    for user_mem_path in [
        user_home.join(".forge-osh").join("CLAUDE.md"),
        user_home.join(".claude").join("CLAUDE.md"),
    ] {
        if let Ok(content) = std::fs::read_to_string(&user_mem_path) {
            if !content.trim().is_empty() {
                sections.push(format!(
                    "### User Memory ({})\n{}",
                    user_mem_path.display(),
                    content.trim()
                ));
            }
        }
    }

    // Walk directory tree looking for CLAUDE.md files
    // Check working_dir and all parent dirs up to home
    let mut check_path: PathBuf = working_dir.to_path_buf();
    let mut project_memories: Vec<(PathBuf, String)> = Vec::new();

    loop {
        let candidate = check_path.join("CLAUDE.md");
        if let Ok(content) = std::fs::read_to_string(&candidate) {
            if !content.trim().is_empty() {
                project_memories.push((candidate, content));
            }
        }

        if check_path == user_home || !check_path.pop() {
            break;
        }
    }

    // Add in reverse order (parent first, more specific last)
    project_memories.reverse();
    for (path, content) in project_memories {
        let is_project_root = path.parent() == Some(working_dir);
        let label = if is_project_root {
            "Project Memory (CLAUDE.md)".to_string()
        } else {
            format!("Memory ({})", path.display())
        };
        sections.push(format!("### {}\n{}", label, content.trim()));
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_system_prompt() {
        let prompt = build_system_prompt(Path::new("."), "");
        assert!(prompt.contains("forge"));
        assert!(prompt.contains("Working Directory"));
    }

    #[test]
    fn test_build_with_extra() {
        let prompt = build_system_prompt(Path::new("."), "Always write tests");
        assert!(prompt.contains("Always write tests"));
    }

    #[test]
    fn test_git_context_non_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = build_git_context(dir.path());
        assert!(result.contains("Not a git repository"));
    }
}
