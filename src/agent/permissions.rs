//! Permission rules system — wildcard pattern-based auto-allow/deny rules
//! persisted across sessions in ~/.forge-osh/permissions.json
//!
//! Format:  tool_name(pattern)
//! Examples:
//!   bash(git *)         — allow all git commands
//!   bash(npm test)      — allow exactly "npm test"
//!   read_file(/src/*)   — allow reading anything in /src/
//!   read_file(*)        — allow all file reads
//!   edit_file(*)        — allow all file edits

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::config_dir;
use crate::types::PermissionLevel;

/// A single permission rule entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionRule {
    /// Tool name, e.g. "bash", "read_file", "edit_file"
    pub tool: String,
    /// Pattern to match against the tool input (glob-style), e.g. "git *", "/src/*"
    pub pattern: String,
    /// Whether this is an allow or deny rule
    pub allow: bool,
    /// Human-readable description
    pub description: String,
}

impl PermissionRule {
    pub fn new_allow(tool: impl Into<String>, pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        let tool = tool.into();
        let description = format!("Auto-allow {}({})", tool, pattern);
        Self {
            tool,
            pattern,
            allow: true,
            description,
        }
    }

    pub fn new_deny(tool: impl Into<String>, pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        let tool = tool.into();
        let description = format!("Auto-deny {}({})", tool, pattern);
        Self {
            tool,
            pattern,
            allow: false,
            description,
        }
    }

    /// Check if this rule matches the given tool call.
    /// The `input_summary` is the key string to match against the pattern.
    pub fn matches(&self, tool_name: &str, input_summary: &str) -> bool {
        if self.tool != tool_name && self.tool != "*" {
            return false;
        }
        // Glob-style pattern matching
        if self.pattern == "*" {
            return true;
        }
        glob_match(&self.pattern, input_summary)
    }
}

/// Check if pattern (glob-style) matches subject
fn glob_match(pattern: &str, subject: &str) -> bool {
    let pattern_lower = pattern.to_lowercase();
    let subject_lower = subject.to_lowercase();

    if pattern_lower == subject_lower {
        return true;
    }

    // Convert glob pattern to a simple prefix/suffix/contains match
    if let Some(prefix) = pattern_lower.strip_suffix('*') {
        if prefix.is_empty() {
            return true;
        }
        return subject_lower.starts_with(prefix.trim());
    }
    if let Some(suffix) = pattern_lower.strip_prefix('*') {
        if suffix.is_empty() {
            return true;
        }
        return subject_lower.ends_with(suffix.trim());
    }
    if pattern_lower.starts_with('*') && pattern_lower.ends_with('*') {
        let inner = &pattern_lower[1..pattern_lower.len() - 1];
        return subject_lower.contains(inner);
    }

    // Try using the glob crate for real glob matching
    if let Ok(p) = glob::Pattern::new(&pattern_lower) {
        if p.matches(&subject_lower) {
            return true;
        }
    }

    false
}

/// The persistent permission rule store
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionStore {
    pub rules: Vec<PermissionRule>,
}

impl PermissionStore {
    fn storage_path() -> PathBuf {
        config_dir().join("permissions.json")
    }

    /// Load from disk, creating defaults if not found
    pub fn load() -> Self {
        let path = Self::storage_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(store) = serde_json::from_str(&content) {
                return store;
            }
        }
        Self::default()
    }

    /// Save to disk
    pub fn save(&self) {
        let path = Self::storage_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Add an allow rule (if not already present)
    pub fn add_allow(&mut self, tool: &str, pattern: &str) {
        let rule = PermissionRule::new_allow(tool, pattern);
        if !self
            .rules
            .iter()
            .any(|r| r.tool == rule.tool && r.pattern == rule.pattern && r.allow)
        {
            self.rules.push(rule);
            self.save();
        }
    }

    /// Add a deny rule
    pub fn add_deny(&mut self, tool: &str, pattern: &str) {
        let rule = PermissionRule::new_deny(tool, pattern);
        if !self
            .rules
            .iter()
            .any(|r| r.tool == rule.tool && r.pattern == rule.pattern && !r.allow)
        {
            self.rules.push(rule);
            self.save();
        }
    }

    /// Remove a rule by index
    pub fn remove(&mut self, index: usize) {
        if index < self.rules.len() {
            self.rules.remove(index);
            self.save();
        }
    }

    /// Check if the tool call is auto-allowed by a stored rule.
    /// Returns Some(true) = auto-allow, Some(false) = auto-deny, None = ask user
    pub fn check(&self, tool_name: &str, input_summary: &str) -> Option<bool> {
        // Deny rules take precedence
        for rule in &self.rules {
            if !rule.allow && rule.matches(tool_name, input_summary) {
                return Some(false);
            }
        }
        // Then check allow rules
        for rule in &self.rules {
            if rule.allow && rule.matches(tool_name, input_summary) {
                return Some(true);
            }
        }
        None // no matching rule — ask user
    }

    /// Format all rules for display
    pub fn display(&self) -> String {
        if self.rules.is_empty() {
            return "No permission rules stored. Add rules with /permissions add bash(git *)."
                .to_string();
        }
        let lines: Vec<String> = self
            .rules
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let kind = if r.allow { "ALLOW" } else { " DENY" };
                format!(
                    "  [{i:>2}] {kind}  {}({})  — {}",
                    r.tool, r.pattern, r.description
                )
            })
            .collect();
        format!(
            "Permission Rules ({} total):\n{}",
            self.rules.len(),
            lines.join("\n")
        )
    }
}

/// Extract a human-readable summary string from tool input for pattern matching
pub fn tool_input_summary(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "bash" => input["command"].as_str().unwrap_or("").to_string(),
        "read_file" | "write_file" | "edit_file" | "create_file" | "delete_file" => {
            input["path"].as_str().unwrap_or("").to_string()
        }
        "list_directory" => input["path"].as_str().unwrap_or("").to_string(),
        "search_files" | "find_files" => input["path"].as_str().unwrap_or("").to_string(),
        "git_commit" => input["message"].as_str().unwrap_or("").to_string(),
        _ => serde_json::to_string(input).unwrap_or_default(),
    }
}

/// Derive the effective permission level considering trust mode and stored rules
pub fn effective_permission(
    tool_name: &str,
    input: &serde_json::Value,
    base_level: &PermissionLevel,
    trust_mode: bool,
    store: &PermissionStore,
) -> EffectivePermission {
    if trust_mode {
        return EffectivePermission::Allow;
    }

    // ReadOnly tools never need permission
    if *base_level == PermissionLevel::ReadOnly {
        return EffectivePermission::Allow;
    }

    let summary = tool_input_summary(tool_name, input);
    match store.check(tool_name, &summary) {
        Some(true) => EffectivePermission::Allow,
        Some(false) => EffectivePermission::Deny,
        None => EffectivePermission::Ask,
    }
}

#[derive(Debug, PartialEq)]
pub enum EffectivePermission {
    Allow,
    Deny,
    Ask,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match() {
        assert!(glob_match("git *", "git commit -m 'hello'"));
        assert!(glob_match("git *", "git push"));
        assert!(!glob_match("git *", "npm test"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("/src/*", "/src/main.rs"));
        assert!(!glob_match("/src/*", "/tests/main.rs"));
    }

    #[test]
    fn test_rule_matching() {
        let rule = PermissionRule::new_allow("bash", "git *");
        assert!(rule.matches("bash", "git commit -m 'fix'"));
        assert!(!rule.matches("bash", "npm install"));
        assert!(!rule.matches("read_file", "git status"));
    }

    #[test]
    fn test_store_check() {
        let mut store = PermissionStore::default();
        store.rules.push(PermissionRule::new_allow("bash", "git *"));
        store
            .rules
            .push(PermissionRule::new_deny("bash", "rm -rf *"));

        assert_eq!(store.check("bash", "git status"), Some(true));
        assert_eq!(store.check("bash", "rm -rf /tmp"), Some(false));
        assert_eq!(store.check("bash", "npm test"), None);
    }
}
