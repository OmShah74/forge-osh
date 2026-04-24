//! Tests for src/agent/permissions.rs

use forge_agent::agent::permissions::*;
use forge_agent::types::PermissionLevel;

#[test]
fn permission_rule_new_allow() {
    let r = PermissionRule::new_allow("bash", "git *");
    assert!(r.allow);
    assert_eq!(r.tool, "bash");
    assert_eq!(r.pattern, "git *");
}

#[test]
fn permission_rule_new_deny() {
    let r = PermissionRule::new_deny("bash", "rm -rf *");
    assert!(!r.allow);
    assert_eq!(r.tool, "bash");
}

#[test]
fn rule_matches_exact_tool() {
    let r = PermissionRule::new_allow("bash", "git *");
    assert!(r.matches("bash", "git commit -m 'fix'"));
    assert!(!r.matches("read_file", "git something"));
}

#[test]
fn rule_matches_wildcard_pattern() {
    let r = PermissionRule::new_allow("bash", "*");
    assert!(r.matches("bash", "anything at all"));
}

#[test]
fn rule_matches_prefix_glob() {
    let r = PermissionRule::new_allow("bash", "git *");
    assert!(r.matches("bash", "git push"));
    assert!(r.matches("bash", "git commit -m hello"));
    assert!(!r.matches("bash", "npm install"));
}

#[test]
fn rule_matches_suffix_glob() {
    let r = PermissionRule::new_allow("edit_file", "*.rs");
    assert!(r.matches("edit_file", "main.rs"));
    assert!(!r.matches("edit_file", "main.py"));
}

#[test]
fn rule_matches_wildcard_tool() {
    let r = PermissionRule::new_allow("*", "anything");
    assert!(r.matches("bash", "anything"));
    assert!(r.matches("read_file", "anything"));
}

#[test]
fn store_check_allow() {
    let mut store = PermissionStore::default();
    store.rules.push(PermissionRule::new_allow("bash", "git *"));
    assert_eq!(store.check("bash", "git status"), Some(true));
}

#[test]
fn store_check_deny() {
    let mut store = PermissionStore::default();
    store
        .rules
        .push(PermissionRule::new_deny("bash", "rm -rf *"));
    assert_eq!(store.check("bash", "rm -rf /tmp"), Some(false));
}

#[test]
fn store_check_no_match() {
    let store = PermissionStore::default();
    assert_eq!(store.check("bash", "npm test"), None);
}

#[test]
fn store_deny_takes_precedence() {
    let mut store = PermissionStore::default();
    store.rules.push(PermissionRule::new_allow("bash", "*"));
    store
        .rules
        .push(PermissionRule::new_deny("bash", "rm -rf *"));
    // Deny should win over allow
    assert_eq!(store.check("bash", "rm -rf /"), Some(false));
    // But allow should work for others
    assert_eq!(store.check("bash", "git status"), Some(true));
}

#[test]
fn store_add_allow_dedup() {
    let mut store = PermissionStore::default();
    store.rules.push(PermissionRule::new_allow("bash", "git *"));
    store.rules.push(PermissionRule::new_allow("bash", "git *")); // dup (via direct push)
    assert_eq!(store.rules.len(), 2); // direct push doesn't dedup
}

#[test]
fn store_remove_by_index() {
    let mut store = PermissionStore::default();
    store.rules.push(PermissionRule::new_allow("bash", "git *"));
    store.rules.push(PermissionRule::new_allow("bash", "npm *"));
    assert_eq!(store.rules.len(), 2);
    store.rules.remove(0);
    assert_eq!(store.rules.len(), 1);
    assert_eq!(store.rules[0].pattern, "npm *");
}

#[test]
fn store_display_empty() {
    let store = PermissionStore::default();
    let d = store.display();
    assert!(d.contains("No permission rules"));
}

#[test]
fn store_display_with_rules() {
    let mut store = PermissionStore::default();
    store.rules.push(PermissionRule::new_allow("bash", "git *"));
    let d = store.display();
    assert!(d.contains("ALLOW"));
    assert!(d.contains("bash"));
    assert!(d.contains("git *"));
}

#[test]
fn tool_input_summary_bash() {
    let input = serde_json::json!({"command": "git status"});
    assert_eq!(tool_input_summary("bash", &input), "git status");
}

#[test]
fn tool_input_summary_file() {
    let input = serde_json::json!({"path": "/src/main.rs"});
    assert_eq!(tool_input_summary("read_file", &input), "/src/main.rs");
}

#[test]
fn effective_permission_trust_mode_allows() {
    let store = PermissionStore::default();
    let result = effective_permission(
        "bash",
        &serde_json::json!({"command": "rm -rf /"}),
        &PermissionLevel::Destructive,
        true, // trust mode
        &store,
    );
    assert_eq!(result, EffectivePermission::Allow);
}

#[test]
fn effective_permission_readonly_auto_allows() {
    let store = PermissionStore::default();
    let result = effective_permission(
        "read_file",
        &serde_json::json!({"path": "/secret"}),
        &PermissionLevel::ReadOnly,
        false,
        &store,
    );
    assert_eq!(result, EffectivePermission::Allow);
}

#[test]
fn effective_permission_asks_when_no_rule() {
    let store = PermissionStore::default();
    let result = effective_permission(
        "bash",
        &serde_json::json!({"command": "npm install"}),
        &PermissionLevel::Shell,
        false,
        &store,
    );
    assert_eq!(result, EffectivePermission::Ask);
}
