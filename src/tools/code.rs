use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::types::*;
use super::Tool;

/// Detect project type from files in working directory
fn detect_project_type(working_dir: &Path) -> Vec<ProjectType> {
    let mut types = Vec::new();
    if working_dir.join("Cargo.toml").exists() {
        types.push(ProjectType::Rust);
    }
    if working_dir.join("package.json").exists() {
        types.push(ProjectType::Node);
    }
    if working_dir.join("pyproject.toml").exists()
        || working_dir.join("setup.py").exists()
        || working_dir.join("requirements.txt").exists()
    {
        types.push(ProjectType::Python);
    }
    if working_dir.join("go.mod").exists() {
        types.push(ProjectType::Go);
    }
    if working_dir.join("pom.xml").exists() || working_dir.join("build.gradle").exists() {
        types.push(ProjectType::Java);
    }
    if working_dir.join("Gemfile").exists() {
        types.push(ProjectType::Ruby);
    }
    types
}

#[derive(Debug)]
enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Java,
    Ruby,
}

async fn run_command(cmd: &str, args: &[&str], working_dir: &Path) -> ToolOutput {
    let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
    let flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };
    let full_cmd = format!("{} {}", cmd, args.join(" "));

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        async {
            let mut child = Command::new(shell)
                .arg(flag)
                .arg(&full_cmd)
                .current_dir(working_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let mut stdout = String::new();
            let mut stderr = String::new();

            if let Some(mut out) = child.stdout.take() {
                out.read_to_string(&mut stdout).await?;
            }
            if let Some(mut err) = child.stderr.take() {
                err.read_to_string(&mut stderr).await?;
            }

            let status = child.wait().await?;
            Ok::<(String, String, bool), std::io::Error>((stdout, stderr, status.success()))
        },
    )
    .await;

    match result {
        Ok(Ok((stdout, stderr, success))) => {
            let mut output = stdout;
            if !stderr.is_empty() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&stderr);
            }
            if success {
                ToolOutput::success(if output.is_empty() {
                    format!("Command completed successfully: {full_cmd}")
                } else {
                    output
                })
            } else {
                ToolOutput::error(output)
            }
        }
        Ok(Err(e)) => ToolOutput::error(format!("Failed to run {cmd}: {e}")),
        Err(_) => ToolOutput::error(format!("Command timed out: {full_cmd}")),
    }
}

// ─── run_linter ───────────────────────────────────────────────────────────

pub struct RunLinterTool;

#[async_trait]
impl Tool for RunLinterTool {
    fn name(&self) -> &str { "run_linter" }
    fn description(&self) -> &str {
        "Run the project's linter. Auto-detects the project type (cargo clippy, eslint, ruff, etc.)."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "fix": { "type": "boolean", "description": "Auto-fix issues if supported", "default": false }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Shell }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let fix = input["fix"].as_bool().unwrap_or(false);
        let types = detect_project_type(&ctx.working_dir);

        if types.is_empty() {
            return ToolOutput::error("Could not detect project type for linting");
        }

        for pt in &types {
            match pt {
                ProjectType::Rust => {
                    let args = if fix {
                        vec!["clippy", "--fix", "--allow-dirty"]
                    } else {
                        vec!["clippy", "--", "-W", "clippy::all"]
                    };
                    return run_command("cargo", &args.iter().copied().collect::<Vec<_>>(), &ctx.working_dir).await;
                }
                ProjectType::Node => {
                    let cmd = if fix { "npx eslint . --fix" } else { "npx eslint ." };
                    return run_command(cmd, &[], &ctx.working_dir).await;
                }
                ProjectType::Python => {
                    let cmd = if fix { "ruff check . --fix" } else { "ruff check ." };
                    return run_command(cmd, &[], &ctx.working_dir).await;
                }
                ProjectType::Go => {
                    return run_command("golangci-lint", &["run"], &ctx.working_dir).await;
                }
                _ => continue,
            }
        }

        ToolOutput::error("No supported linter found for this project type")
    }
}

// ─── run_tests ────────────────────────────────────────────────────────────

pub struct RunTestsTool;

#[async_trait]
impl Tool for RunTestsTool {
    fn name(&self) -> &str { "run_tests" }
    fn description(&self) -> &str {
        "Run the project's test suite. Auto-detects the project type."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter": { "type": "string", "description": "Test name filter/pattern" },
                "verbose": { "type": "boolean", "default": false }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Shell }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let filter = input["filter"].as_str();
        let verbose = input["verbose"].as_bool().unwrap_or(false);
        let types = detect_project_type(&ctx.working_dir);

        if types.is_empty() {
            return ToolOutput::error("Could not detect project type for testing");
        }

        for pt in &types {
            match pt {
                ProjectType::Rust => {
                    let mut args = vec!["test"];
                    if verbose {
                        args.push("--verbose");
                    }
                    if let Some(f) = filter {
                        args.push("--");
                        args.push(f);
                    }
                    return run_command("cargo", &args, &ctx.working_dir).await;
                }
                ProjectType::Node => {
                    return run_command("npm", &["test"], &ctx.working_dir).await;
                }
                ProjectType::Python => {
                    let mut args = vec!["-m", "pytest"];
                    if verbose {
                        args.push("-v");
                    }
                    if let Some(f) = filter {
                        args.push("-k");
                        args.push(f);
                    }
                    return run_command("python", &args, &ctx.working_dir).await;
                }
                ProjectType::Go => {
                    let mut args = vec!["test", "./..."];
                    if verbose {
                        args.push("-v");
                    }
                    return run_command("go", &args, &ctx.working_dir).await;
                }
                _ => continue,
            }
        }

        ToolOutput::error("No supported test runner found for this project type")
    }
}

// ─── run_formatter ────────────────────────────────────────────────────────

pub struct RunFormatterTool;

#[async_trait]
impl Tool for RunFormatterTool {
    fn name(&self) -> &str { "run_formatter" }
    fn description(&self) -> &str {
        "Run the project's code formatter. Auto-detects the project type."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "check": { "type": "boolean", "description": "Check only, don't modify", "default": false }
            },
            "required": []
        })
    }
    fn permission_level(&self) -> PermissionLevel { PermissionLevel::Shell }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolOutput {
        let check = input["check"].as_bool().unwrap_or(false);
        let types = detect_project_type(&ctx.working_dir);

        if types.is_empty() {
            return ToolOutput::error("Could not detect project type for formatting");
        }

        for pt in &types {
            match pt {
                ProjectType::Rust => {
                    let args = if check {
                        vec!["fmt", "--check"]
                    } else {
                        vec!["fmt"]
                    };
                    return run_command("cargo", &args, &ctx.working_dir).await;
                }
                ProjectType::Node => {
                    let args = if check {
                        "npx prettier --check ."
                    } else {
                        "npx prettier --write ."
                    };
                    return run_command(args, &[], &ctx.working_dir).await;
                }
                ProjectType::Python => {
                    let args = if check {
                        vec!["--check", "."]
                    } else {
                        vec!["."]
                    };
                    return run_command("black", &args, &ctx.working_dir).await;
                }
                ProjectType::Go => {
                    return run_command("gofmt", &["-w", "."], &ctx.working_dir).await;
                }
                _ => continue,
            }
        }

        ToolOutput::error("No supported formatter found for this project type")
    }
}
