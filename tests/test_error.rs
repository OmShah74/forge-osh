//! Tests for src/error.rs — ForgeError types and conversions

use forge_agent::error::*;

#[test]
fn error_provider_display() {
    let err = ForgeError::provider("connection refused");
    assert!(err.to_string().contains("connection refused"));
}

#[test]
fn error_api_display() {
    let err = ForgeError::api(429, "rate limited");
    let s = err.to_string();
    assert!(s.contains("429"));
    assert!(s.contains("rate limited"));
}

#[test]
fn error_tool_display() {
    let err = ForgeError::tool("unknown tool: foobar");
    assert!(err.to_string().contains("foobar"));
}

#[test]
fn error_config_display() {
    let err = ForgeError::config("missing API key");
    assert!(err.to_string().contains("missing API key"));
}

#[test]
fn error_token_limit_exceeded_message() {
    let err = ForgeError::TokenLimitExceeded {
        used: 100000,
        limit: 128000,
    };
    let msg = err.user_message();
    assert!(msg.contains("100000"));
    assert!(msg.contains("128000"));
    assert!(msg.contains("Context window"));
}

#[test]
fn error_permission_denied_message() {
    let err = ForgeError::PermissionDenied("bash(rm -rf /)".into());
    let msg = err.user_message();
    assert!(msg.contains("Permission denied"));
    assert!(msg.contains("bash"));
}

#[test]
fn error_interrupted_message() {
    let err = ForgeError::Interrupted;
    assert!(err.user_message().contains("interrupted"));
}

#[test]
fn error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
    let forge_err: ForgeError = io_err.into();
    assert!(matches!(forge_err, ForgeError::Io(_)));
    assert!(forge_err.to_string().contains("not found"));
}

#[test]
fn error_from_serde_json() {
    let bad_json = "not valid json{{{";
    let result: std::result::Result<serde_json::Value, _> = serde_json::from_str(bad_json);
    let serde_err = result.unwrap_err();
    let forge_err: ForgeError = serde_err.into();
    assert!(matches!(forge_err, ForgeError::Serde(_)));
}

#[test]
fn error_timeout_display() {
    let err = ForgeError::Timeout(30);
    assert!(err.to_string().contains("30"));
}

#[test]
fn error_session_display() {
    let err = ForgeError::Session("corrupt checkpoint".into());
    assert!(err.to_string().contains("corrupt checkpoint"));
}

#[test]
fn error_git_display() {
    let err = ForgeError::Git("not a git repository".into());
    assert!(err.to_string().contains("not a git repository"));
}

#[test]
fn error_other_display() {
    let err = ForgeError::Other("something unexpected".into());
    let msg = err.user_message();
    assert!(msg.contains("something unexpected"));
}
