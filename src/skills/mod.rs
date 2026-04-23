use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::agent::hooks::{HookEntry, HooksConfig};
use crate::config::{config_dir, data_dir};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    Project,
    User,
    Bundled,
}

impl SkillSource {
    pub fn label(&self) -> &'static str {
        match self {
            SkillSource::Project => "project",
            SkillSource::User => "user",
            SkillSource::Bundled => "bundled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillExecutionMode {
    Inline,
    Fork,
}

impl SkillExecutionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillExecutionMode::Inline => "inline",
            SkillExecutionMode::Fork => "fork",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillFrontmatter {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub when_to_use: Option<String>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub execution_mode: Option<String>,
    #[serde(default)]
    pub user_invocable: Option<bool>,
    #[serde(default)]
    pub hooks: Option<SkillHooks>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillHooks {
    #[serde(rename = "PreToolUse", default)]
    pub pre_tool_use: Vec<HookEntry>,
    #[serde(rename = "PostToolUse", default)]
    pub post_tool_use: Vec<HookEntry>,
    #[serde(rename = "Stop", default)]
    pub stop: Vec<HookEntry>,
}

impl SkillHooks {
    pub fn as_hooks_config(&self) -> HooksConfig {
        HooksConfig {
            pre_tool_use: self.pre_tool_use.clone(),
            post_tool_use: self.post_tool_use.clone(),
            stop: self.stop.clone(),
            notification: Vec::new(),
            user_prompt_submit: Vec::new(),
            session_start: Vec::new(),
            session_end: Vec::new(),
            pre_compact: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pre_tool_use.is_empty() && self.post_tool_use.is_empty() && self.stop.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub key: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub allowed_tools: Vec<String>,
    pub model: Option<String>,
    pub execution_mode: SkillExecutionMode,
    pub user_invocable: bool,
    pub source: SkillSource,
    pub root_dir: Option<PathBuf>,
    pub canonical_path: Option<PathBuf>,
    pub content: String,
    pub hooks: SkillHooks,
    pub metadata: SkillFrontmatter,
    pub bundled_files: HashMap<String, String>,
}

impl SkillDefinition {
    pub fn base_dir(&self) -> Option<PathBuf> {
        match self.source {
            SkillSource::Bundled if !self.bundled_files.is_empty() => {
                Some(extract_bundled_files(self))
            }
            _ => self.root_dir.clone(),
        }
    }

    pub fn materialize_prompt(&self, session_id: &str, args: Option<&str>) -> String {
        let mut text = self.content.clone();
        if let Some(args) = args {
            text = text.replace("${ARGS}", args);
        }
        if let Some(base_dir) = self.base_dir() {
            let dir_str = base_dir.to_string_lossy().replace('\\', "/");
            text = format!("Base directory for this skill: {dir_str}\n\n{text}");
            text = text.replace("${FORGE_SKILL_DIR}", &dir_str);
        }
        text.replace("${FORGE_SESSION_ID}", session_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillRegistry {
    pub skills: Vec<SkillDefinition>,
}

impl SkillRegistry {
    pub fn find(&self, name: &str) -> Option<&SkillDefinition> {
        let needle = name.trim_start_matches('/');
        self.skills.iter().find(|skill| {
            skill.name.eq_ignore_ascii_case(needle)
                || skill.display_name.eq_ignore_ascii_case(needle)
        })
    }

    pub fn list_for_prompt(&self, max_items: usize) -> Vec<&SkillDefinition> {
        self.skills
            .iter()
            .filter(|s| s.user_invocable)
            .take(max_items)
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInvocationRecord {
    pub skill_name: String,
    pub source: SkillSource,
    pub canonical_path: Option<PathBuf>,
    pub materialized_prompt: String,
    pub invoked_at: chrono::DateTime<chrono::Utc>,
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSkillScope {
    pub skill_name: String,
    pub allowed_tools: Vec<String>,
    pub model_override: Option<String>,
    pub hooks: SkillHooks,
    pub execution_mode: SkillExecutionMode,
}

impl ActiveSkillScope {
    pub fn allows_tool(&self, tool_name: &str) -> bool {
        if self.allowed_tools.is_empty() {
            return true;
        }
        self.allowed_tools
            .iter()
            .any(|allowed| allowed == tool_name)
    }
}

pub struct SkillLoader;

impl SkillLoader {
    pub fn load(working_dir: &Path) -> SkillRegistry {
        let mut by_name: HashMap<String, SkillDefinition> = HashMap::new();

        for skill in bundled_skills() {
            by_name.insert(skill.name.clone(), skill);
        }
        for skill in Self::load_dir(&user_skills_dir(), SkillSource::User) {
            by_name.insert(skill.name.clone(), skill);
        }
        for skill in Self::load_dir(&project_skills_dir(working_dir), SkillSource::Project) {
            by_name.insert(skill.name.clone(), skill);
        }

        let mut skills: Vec<SkillDefinition> = by_name.into_values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        SkillRegistry { skills }
    }

    fn load_dir(dir: &Path, source: SkillSource) -> Vec<SkillDefinition> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            match load_skill_from_file(&skill_md, source) {
                Ok(skill) => out.push(skill),
                Err(err) => warn!("failed to load skill {}: {}", skill_md.display(), err),
            }
        }
        out
    }
}

pub type SharedSkillRegistry = Arc<RwLock<SkillRegistry>>;

pub fn shared_registry(working_dir: &Path) -> SharedSkillRegistry {
    Arc::new(RwLock::new(SkillLoader::load(working_dir)))
}

pub fn refresh_registry(registry: &SharedSkillRegistry, working_dir: &Path) {
    *registry.write() = SkillLoader::load(working_dir);
}

fn parse_markdown_skill(
    path: &Path,
    source: SkillSource,
    raw: &str,
) -> anyhow::Result<SkillDefinition> {
    let (frontmatter, body) = split_frontmatter(raw)?;
    let parsed: SkillFrontmatter = if let Some(frontmatter) = frontmatter {
        parse_frontmatter(frontmatter)
    } else {
        SkillFrontmatter::default()
    };
    let skill_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let dir_name = skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("skill");
    let name = sanitize_name(parsed.name.as_deref().unwrap_or(dir_name));
    let description = parsed
        .description
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| extract_description(body).unwrap_or_else(|| format!("Skill `{name}`")));
    let mode = match parsed.execution_mode.as_deref() {
        Some("fork") => SkillExecutionMode::Fork,
        _ => SkillExecutionMode::Inline,
    };
    let canonical_path = std::fs::canonicalize(path).ok();

    Ok(SkillDefinition {
        key: format!("{}:{name}", source.label()),
        name: name.clone(),
        display_name: parsed.name.clone().unwrap_or_else(|| name.clone()),
        description,
        when_to_use: parsed.when_to_use.clone(),
        allowed_tools: parsed.allowed_tools.clone().unwrap_or_default(),
        model: parsed.model.clone(),
        execution_mode: mode,
        user_invocable: parsed.user_invocable.unwrap_or(true),
        source,
        root_dir: Some(skill_dir.to_path_buf()),
        canonical_path,
        content: body.trim().to_string(),
        hooks: parsed.hooks.clone().unwrap_or_default(),
        metadata: parsed,
        bundled_files: HashMap::new(),
    })
}

fn parse_frontmatter(frontmatter: &str) -> SkillFrontmatter {
    let mut parsed = SkillFrontmatter::default();
    let lines: Vec<&str> = frontmatter.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "name" => parsed.name = some_string(value),
                "description" => parsed.description = some_string(value),
                "when_to_use" => parsed.when_to_use = some_string(value),
                "model" => parsed.model = some_string(value),
                "execution_mode" => parsed.execution_mode = some_string(value),
                "user_invocable" => parsed.user_invocable = parse_bool(value),
                "allowed_tools" => {
                    let (vals, consumed) = parse_string_list(&lines, i, value);
                    parsed.allowed_tools = Some(vals);
                    i = consumed;
                }
                "hooks" => {
                    let (hooks, consumed) = parse_hooks(&lines, i);
                    parsed.hooks = Some(hooks);
                    i = consumed;
                }
                other => {
                    parsed.extra.insert(other.to_string(), value.to_string());
                }
            }
        }
        i += 1;
    }
    parsed
}

fn some_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "True" | "yes" | "1" => Some(true),
        "false" | "False" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn parse_string_list(lines: &[&str], start: usize, inline_value: &str) -> (Vec<String>, usize) {
    if !inline_value.is_empty() && inline_value.starts_with('[') && inline_value.ends_with(']') {
        let vals = inline_value
            .trim_matches(['[', ']'])
            .split(',')
            .map(|item| item.trim().trim_matches('"').trim_matches('\''))
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
            .collect();
        return (vals, start);
    }
    let base_indent = lines[start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();
    let mut values = Vec::new();
    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        if indent <= base_indent || !trimmed.starts_with('-') {
            break;
        }
        values.push(
            trimmed
                .trim_start_matches('-')
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        );
        i += 1;
    }
    (values, i.saturating_sub(1))
}

fn parse_hooks(lines: &[&str], start: usize) -> (SkillHooks, usize) {
    let base_indent = lines[start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();
    let mut hooks = SkillHooks::default();
    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        if indent <= base_indent {
            break;
        }
        if let Some((section, _)) = trimmed.split_once(':') {
            let section_indent = indent;
            let (entries, consumed) = parse_hook_entries(lines, i, section_indent);
            match section.trim() {
                "PreToolUse" => hooks.pre_tool_use = entries,
                "PostToolUse" => hooks.post_tool_use = entries,
                "Stop" => hooks.stop = entries,
                _ => {}
            }
            i = consumed + 1;
            continue;
        }
        i += 1;
    }
    (hooks, i.saturating_sub(1))
}

fn parse_hook_entries(
    lines: &[&str],
    start: usize,
    section_indent: usize,
) -> (Vec<HookEntry>, usize) {
    let mut entries = Vec::new();
    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        if indent <= section_indent {
            break;
        }
        if trimmed.starts_with("- ") {
            let mut entry = HookEntry {
                matcher: "*".to_string(),
                command: String::new(),
                timeout_seconds: 10,
                blocking: false,
            };
            let first = trimmed.trim_start_matches("- ").trim();
            if let Some((k, v)) = first.split_once(':') {
                apply_hook_field(&mut entry, k.trim(), v.trim());
            }
            i += 1;
            while i < lines.len() {
                let sub = lines[i];
                let sub_indent = sub.chars().take_while(|c| c.is_whitespace()).count();
                let sub_trimmed = sub.trim();
                if sub_trimmed.is_empty() {
                    i += 1;
                    continue;
                }
                if sub_indent <= indent || sub_trimmed.starts_with("- ") {
                    break;
                }
                if let Some((k, v)) = sub_trimmed.split_once(':') {
                    apply_hook_field(&mut entry, k.trim(), v.trim());
                }
                i += 1;
            }
            if !entry.command.is_empty() {
                entries.push(entry);
            }
            continue;
        }
        i += 1;
    }
    (entries, i.saturating_sub(1))
}

fn apply_hook_field(entry: &mut HookEntry, key: &str, value: &str) {
    let clean = value.trim().trim_matches('"').trim_matches('\'');
    match key {
        "matcher" => entry.matcher = clean.to_string(),
        "command" => entry.command = clean.to_string(),
        "timeout_seconds" => {
            if let Ok(n) = clean.parse::<u64>() {
                entry.timeout_seconds = n;
            }
        }
        "blocking" => entry.blocking = parse_bool(clean).unwrap_or(false),
        _ => {}
    }
}

fn load_skill_from_file(path: &Path, source: SkillSource) -> anyhow::Result<SkillDefinition> {
    let raw = std::fs::read_to_string(path)?;
    parse_markdown_skill(path, source, &raw)
}

fn split_frontmatter(raw: &str) -> anyhow::Result<(Option<&str>, &str)> {
    let normalized = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let rest = if let Some(rest) = normalized.strip_prefix("---\n") {
        rest
    } else if let Some(rest) = normalized.strip_prefix("---\r\n") {
        rest
    } else {
        return Ok((None, normalized));
    };

    for marker in ["\n---\n", "\n---\r\n", "\r\n---\n", "\r\n---\r\n"] {
        if let Some(close_idx) = rest.find(marker) {
            let frontmatter = &rest[..close_idx];
            let body = &rest[close_idx + marker.len()..];
            return Ok((Some(frontmatter), body));
        }
    }

    Err(anyhow::anyhow!("unterminated frontmatter"))
}

fn extract_description(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("```"))
        .map(|line| line.to_string())
}

fn sanitize_name(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '-' | '_' => c,
            ' ' => '-',
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn user_skills_dir() -> PathBuf {
    config_dir().join("skills")
}

fn project_skills_dir(working_dir: &Path) -> PathBuf {
    working_dir.join(".claude").join("skills")
}

fn bundled_extract_dir(name: &str) -> PathBuf {
    data_dir().join("bundled_skills").join(name)
}

fn extract_bundled_files(skill: &SkillDefinition) -> PathBuf {
    let dir = bundled_extract_dir(&skill.name);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        warn!(
            "failed to create bundled skill dir {}: {}",
            dir.display(),
            err
        );
        return dir;
    }
    for (rel, content) in &skill.bundled_files {
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::write(&target, content) {
            warn!(
                "failed to write bundled skill file {}: {}",
                target.display(),
                err
            );
        }
    }
    dir
}

fn bundled_skills() -> Vec<SkillDefinition> {
    vec![
        bundled_skill(
            "debug",
            "Structured debugging workflow for reproducing issues and isolating root causes.",
            Some("Use when the user needs bug diagnosis, reproduction steps, or root-cause analysis."),
            vec!["read_file", "search_files", "find_files", "list_directory", "bash", "powershell", "run_tests", "git_diff"],
            SkillExecutionMode::Inline,
            include_str!("bundled/debug.md"),
            HashMap::new(),
        ),
        bundled_skill(
            "review",
            "Review changes for bugs, regressions, and missing tests before shipping.",
            Some("Use when the user asks for a review or when validating risky code changes."),
            vec!["read_file", "search_files", "find_files", "list_directory", "git_diff", "git_status", "run_tests"],
            SkillExecutionMode::Inline,
            include_str!("bundled/review.md"),
            HashMap::new(),
        ),
        bundled_skill(
            "refactor",
            "Plan and apply safe refactors while preserving behavior and verifying each step.",
            Some("Use when the user wants code structure improved without changing external behavior."),
            vec!["read_file", "search_files", "find_files", "list_directory", "edit_file", "write_file", "run_tests", "run_formatter"],
            SkillExecutionMode::Inline,
            include_str!("bundled/refactor.md"),
            HashMap::new(),
        ),
        bundled_skill(
            "project-memory",
            "Capture durable project guidance into CLAUDE.md without overwriting existing intent.",
            Some("Use when the user wants to record conventions, workflows, or persistent project memory."),
            vec!["read_file", "write_file", "edit_file", "list_directory"],
            SkillExecutionMode::Inline,
            include_str!("bundled/project_memory.md"),
            HashMap::new(),
        ),
    ]
}

fn bundled_skill(
    name: &str,
    description: &str,
    when_to_use: Option<&str>,
    allowed_tools: Vec<&str>,
    execution_mode: SkillExecutionMode,
    content: &str,
    bundled_files: HashMap<String, String>,
) -> SkillDefinition {
    SkillDefinition {
        key: format!("bundled:{name}"),
        name: name.to_string(),
        display_name: name.to_string(),
        description: description.to_string(),
        when_to_use: when_to_use.map(|s| s.to_string()),
        allowed_tools: allowed_tools.into_iter().map(|s| s.to_string()).collect(),
        model: None,
        execution_mode,
        user_invocable: true,
        source: SkillSource::Bundled,
        root_dir: None,
        canonical_path: None,
        content: content.trim().to_string(),
        hooks: SkillHooks::default(),
        metadata: SkillFrontmatter::default(),
        bundled_files,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillApplyResult {
    pub skill_name: String,
    pub mode: SkillExecutionMode,
    pub materialized_prompt: String,
    pub allowed_tools: Vec<String>,
    pub model_override: Option<String>,
    pub hooks: SkillHooks,
    pub source: SkillSource,
    pub canonical_path: Option<PathBuf>,
}

pub fn apply_skill(
    registry: &SkillRegistry,
    name: &str,
    args: Option<&str>,
    session_id: &str,
) -> anyhow::Result<SkillApplyResult> {
    let skill = registry
        .find(name)
        .ok_or_else(|| anyhow::anyhow!("unknown skill: {name}"))?;
    if !skill.user_invocable {
        return Err(anyhow::anyhow!(
            "skill '{}' is not invocable in this context",
            skill.name
        ));
    }
    let materialized_prompt = skill.materialize_prompt(session_id, args);
    Ok(SkillApplyResult {
        skill_name: skill.name.clone(),
        mode: skill.execution_mode,
        materialized_prompt,
        allowed_tools: skill.allowed_tools.clone(),
        model_override: skill.model.clone(),
        hooks: skill.hooks.clone(),
        source: skill.source,
        canonical_path: skill.canonical_path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let raw = "---\nname: Review Helper\ndescription: Review code carefully\nallowed_tools:\n  - read_file\n  - git_diff\nexecution_mode: fork\nuser_invocable: false\n---\nInspect the diff and report issues.";
        let skill = parse_markdown_skill(
            Path::new("skills/review/SKILL.md"),
            SkillSource::Project,
            raw,
        )
        .unwrap();

        assert_eq!(skill.name, "review-helper");
        assert_eq!(skill.display_name, "Review Helper");
        assert_eq!(skill.description, "Review code carefully");
        assert_eq!(skill.allowed_tools, vec!["read_file", "git_diff"]);
        assert_eq!(skill.execution_mode, SkillExecutionMode::Fork);
        assert!(!skill.user_invocable);
        assert_eq!(skill.content, "Inspect the diff and report issues.");
    }

    #[test]
    fn falls_back_to_first_meaningful_paragraph() {
        let raw =
            "# Heading\n\nUse this skill to debug production regressions.\n\nMore detail later.";
        let skill = parse_markdown_skill(
            Path::new("skills/debug/SKILL.md"),
            SkillSource::Project,
            raw,
        )
        .unwrap();

        assert_eq!(skill.name, "debug");
        assert_eq!(
            skill.description,
            "Use this skill to debug production regressions."
        );
    }

    #[test]
    fn project_overrides_user_and_bundled() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path();
        let project_dir = cwd.join(".claude").join("skills").join("review");
        let user_root = temp.path().join("user-skills");
        let user_dir = user_root.join("review");

        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(
            project_dir.join("SKILL.md"),
            "---\ndescription: Project review\n---\nProject body",
        )
        .unwrap();
        std::fs::write(
            user_dir.join("SKILL.md"),
            "---\ndescription: User review\n---\nUser body",
        )
        .unwrap();

        let mut by_name: HashMap<String, SkillDefinition> = HashMap::new();
        for skill in bundled_skills() {
            by_name.insert(skill.name.clone(), skill);
        }
        for skill in SkillLoader::load_dir(&user_root, SkillSource::User) {
            by_name.insert(skill.name.clone(), skill);
        }
        for skill in
            SkillLoader::load_dir(&cwd.join(".claude").join("skills"), SkillSource::Project)
        {
            by_name.insert(skill.name.clone(), skill);
        }

        let skill = by_name.get("review").unwrap();
        assert_eq!(skill.source, SkillSource::Project);
        assert_eq!(skill.description, "Project review");
    }

    #[test]
    fn apply_skill_rejects_non_invocable_skills() {
        let registry = SkillRegistry {
            skills: vec![SkillDefinition {
                key: "project:hidden".to_string(),
                name: "hidden".to_string(),
                display_name: "hidden".to_string(),
                description: "hidden".to_string(),
                when_to_use: None,
                allowed_tools: Vec::new(),
                model: None,
                execution_mode: SkillExecutionMode::Inline,
                user_invocable: false,
                source: SkillSource::Project,
                root_dir: None,
                canonical_path: None,
                content: "secret".to_string(),
                hooks: SkillHooks::default(),
                metadata: SkillFrontmatter::default(),
                bundled_files: HashMap::new(),
            }],
        };

        let err = apply_skill(&registry, "hidden", None, "sess-1").unwrap_err();
        assert!(err.to_string().contains("not invocable"));
    }
}
