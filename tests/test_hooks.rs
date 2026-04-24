//! Tests for src/agent/hooks.rs

use forge_agent::agent::hooks::*;

#[test]
fn hooks_config_default_is_empty() {
    let config = HooksConfig::default();
    assert!(config.is_empty());
    assert!(config.pre_tool_use.is_empty());
    assert!(config.post_tool_use.is_empty());
    assert!(config.stop.is_empty());
    assert!(config.notification.is_empty());
}

#[test]
fn hooks_config_not_empty_with_entries() {
    let mut config = HooksConfig::default();
    config.pre_tool_use.push(HookEntry {
        matcher: "*".into(),
        command: "echo hello".into(),
        timeout_seconds: 10,
        blocking: false,
    });
    assert!(!config.is_empty());
}

#[test]
fn hook_entry_default_timeout() {
    let json = r#"{"command": "echo test"}"#;
    let entry: HookEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.timeout_seconds, 10);
    assert_eq!(entry.matcher, "*");
}

#[test]
fn hook_entry_custom_timeout() {
    let json = r#"{"matcher": "bash", "command": "echo test", "timeout_seconds": 30}"#;
    let entry: HookEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.timeout_seconds, 30);
    assert_eq!(entry.matcher, "bash");
}

#[test]
fn hooks_config_serialization_roundtrip() {
    let mut config = HooksConfig::default();
    config.pre_tool_use.push(HookEntry {
        matcher: "bash".into(),
        command: "echo pre".into(),
        timeout_seconds: 5,
        blocking: true,
    });
    config.post_tool_use.push(HookEntry {
        matcher: "*".into(),
        command: "echo post".into(),
        timeout_seconds: 10,
        blocking: false,
    });
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: HooksConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.pre_tool_use.len(), 1);
    assert_eq!(deserialized.post_tool_use.len(), 1);
    assert_eq!(deserialized.pre_tool_use[0].matcher, "bash");
}
