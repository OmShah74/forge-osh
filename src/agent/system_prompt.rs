use std::path::Path;

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
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let project_context = detect_project_context(working_dir);
    let git_info = detect_git_info(working_dir);

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
- Git: {git_info}

## Project Context
{project_context}

## How You Work
You operate in an autonomous agentic loop:
1. Understand the user's goal thoroughly before acting
2. For complex tasks, plan your steps first (then execute)
3. Read all relevant code before making changes
4. Make precise, targeted edits using edit_file (not full rewrites)
5. Verify your changes work (run tests, check for errors)
6. Report what you did and any issues you encountered

## Tool Usage Guidelines
- Prefer reading before writing — understand before changing
- Use edit_file for modifications, write_file only for new files
- Run bash commands to verify changes (compile, test, lint)
- Search before writing new code — don't duplicate existing logic
- Use git tools only when explicitly asked
- When fetching web content, extract only what's relevant

## Communication Style
- Be concise — don't over-explain
- Show your work — explain significant decisions
- Flag uncertainty — say when you're not sure
- Ask rather than assume — one clarifying question beats wrong action

## Safety Rules
- Never delete files without explicit confirmation
- Never commit API keys, passwords, or secrets
- Always show significant file changes before applying
- Never rm -rf anything without double confirmation
- Prefer reversible actions over irreversible ones

## Response Format
- For simple tasks: act immediately, summarize at end
- For complex tasks: state plan → execute step by step → summarize
- On errors: explain what went wrong → propose fix → ask to proceed
- On completion: brief summary of changes made"#
    );

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
    if working_dir.join(".git").exists() {
        context_parts.push("- Version control: Git repository".to_string());
    }

    if context_parts.is_empty() {
        "No specific project structure detected.".to_string()
    } else {
        context_parts.join("\n")
    }
}

fn detect_git_info(working_dir: &Path) -> String {
    if !working_dir.join(".git").exists() {
        return "Not a git repository".to_string();
    }

    // Try to get branch name
    let head_path = working_dir.join(".git/HEAD");
    if let Ok(content) = std::fs::read_to_string(head_path) {
        if let Some(branch) = content.strip_prefix("ref: refs/heads/") {
            return format!("On branch {}", branch.trim());
        }
        return format!("Detached HEAD at {}", &content.trim()[..8.min(content.trim().len())]);
    }

    "Git repository (branch unknown)".to_string()
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
}
