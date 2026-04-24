//! Tests for src/agent/planner.rs

use forge_agent::agent::planner::Planner;

#[test]
fn planner_detects_refactor() {
    assert!(Planner::is_complex_task(
        "Refactor the auth module to use JWT"
    ));
}

#[test]
fn planner_detects_build() {
    assert!(Planner::is_complex_task(
        "Build a new REST API with CRUD operations"
    ));
}

#[test]
fn planner_detects_migrate() {
    assert!(Planner::is_complex_task(
        "Migrate the database from MySQL to PostgreSQL"
    ));
}

#[test]
fn planner_detects_implement() {
    assert!(Planner::is_complex_task(
        "Implement a caching layer for the API"
    ));
}

#[test]
fn planner_detects_setup() {
    assert!(Planner::is_complex_task(
        "Set up CI/CD pipeline with GitHub Actions"
    ));
}

#[test]
fn planner_detects_rewrite() {
    assert!(Planner::is_complex_task(
        "Rewrite the parser using a recursive descent approach"
    ));
}

#[test]
fn planner_detects_overhaul() {
    assert!(Planner::is_complex_task(
        "Overhaul the error handling throughout the project"
    ));
}

#[test]
fn planner_detects_long_messages() {
    let long_msg = "I want you to look at the current implementation of the authentication module and then check how the session tokens are being validated and after that make sure the refresh token rotation is happening correctly and also verify the CORS headers are properly set up for the frontend";
    assert!(Planner::is_complex_task(long_msg));
}

#[test]
fn planner_simple_question_not_complex() {
    assert!(!Planner::is_complex_task("What does this function do?"));
}

#[test]
fn planner_simple_fix_not_complex() {
    assert!(!Planner::is_complex_task("Fix the typo"));
}

#[test]
fn planner_short_query_not_complex() {
    assert!(!Planner::is_complex_task("Show me line 5"));
}

#[test]
fn planner_case_insensitive() {
    assert!(Planner::is_complex_task("REFACTOR everything"));
    assert!(Planner::is_complex_task("BUILD a new system"));
}

#[test]
fn planning_prompt_contains_user_message() {
    let prompt = Planner::planning_prompt("refactor auth");
    assert!(prompt.contains("refactor auth"));
    assert!(prompt.contains("plan"));
}
