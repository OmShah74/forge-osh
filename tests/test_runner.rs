//! forge-osh Comprehensive Internal Test Suite
//!
//! This is a standalone integration test binary that exercises ALL features
//! of the forge-osh application without requiring API keys or external
//! services. It generates a detailed HTML + terminal report.
//!
//! Run: cargo test --test test_runner -- --nocapture
//! Or:  cargo test --test test_runner 2>&1 | tee tests/report.txt

// All test modules
mod test_agent_loop;
mod test_compaction;
mod test_config;
mod test_context;
mod test_coordinator;
mod test_edit_robust;
mod test_error;
mod test_evaluation_harness;
mod test_file_history;
mod test_graph_builder;
mod test_graph_parser;
mod test_graph_query;
mod test_graph_types;
mod test_hooks;
mod test_models;
mod test_permissions;
mod test_planner;
mod test_provider_router;
mod test_session;
mod test_system_prompt;
mod test_tools_agent;
mod test_tools_code;
mod test_tools_executor;
mod test_tools_fs;
mod test_tools_git;
mod test_tools_notebook;
mod test_tools_registry;
mod test_tools_search;
mod test_tools_shell;
mod test_tools_tasks;
mod test_tools_web;
mod test_tui_diff;
mod test_tui_input;
mod test_tui_picker;
mod test_tui_spinner;
mod test_tui_themes;
mod test_types;
