//! Tests for src/config/ — Config loading, defaults, model database

use forge_agent::config::Config;

#[test]
fn config_default_loads() {
    let cfg = Config::default();
    assert!(!cfg.general.theme.is_empty());
    assert!(cfg.agent.max_tokens > 0);
    assert!(cfg.agent.max_tool_iterations > 0);
}

#[test]
fn config_default_theme() {
    let cfg = Config::default();
    assert_eq!(cfg.general.theme, "dark");
}

#[test]
fn config_default_trust_mode_off() {
    let cfg = Config::default();
    assert!(!cfg.general.trust_mode);
}

#[test]
fn config_default_max_tokens() {
    let cfg = Config::default();
    assert!(cfg.agent.max_tokens >= 4096);
}

#[test]
fn config_default_temperature() {
    let cfg = Config::default();
    assert!((cfg.agent.temperature - 0.7).abs() < 0.01);
}

#[test]
fn config_default_max_iterations() {
    let cfg = Config::default();
    assert!(cfg.agent.max_tool_iterations >= 10);
}

#[test]
fn config_default_planning_mode_on() {
    let cfg = Config::default();
    assert!(cfg.agent.planning_mode);
}

#[test]
fn config_default_auto_summarize() {
    let cfg = Config::default();
    assert!(cfg.agent.auto_summarize_at > 0.0);
    assert!(cfg.agent.auto_summarize_at <= 1.0);
}

#[test]
fn config_bash_has_blocked_commands() {
    let cfg = Config::default();
    assert!(!cfg.tools.bash.blocked_commands.is_empty());
    // rm -rf / should be blocked
    assert!(cfg
        .tools
        .bash
        .blocked_commands
        .iter()
        .any(|c| c.contains("rm -rf")));
}

#[test]
fn config_bash_timeout_positive() {
    let cfg = Config::default();
    assert!(cfg.tools.bash.timeout_seconds > 0);
}

#[test]
fn config_web_enabled() {
    let cfg = Config::default();
    assert!(cfg.tools.web.enabled);
}
