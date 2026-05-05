//! LSP-based code intelligence for forge-osh.
//!
//! This module integrates Language Server Protocol clients into the agent's
//! tool surface. The architecture is:
//!
//!   AgentLoop / ToolRegistry → SharedLspManager → LspClient → spawned server
//!
//! - `protocol.rs`  — JSON-RPC framing and minimal LSP message types
//! - `client.rs`    — single-server client (process + req/resp routing)
//! - `manager.rs`   — multiplexer keyed by language; lazy spawn
//! - `config.rs`    — built-in server registry + path/URI helpers
//! - `tools.rs`     — `Tool` impls (lsp_diagnostics, lsp_definition, ...)
//!
//! Servers are spawned on first use only. If no language server is installed
//! for a file's language, the LSP tools degrade gracefully with a friendly
//! "not installed" message rather than failing the whole conversation.

pub mod client;
pub mod config;
pub mod manager;
pub mod protocol;
pub mod tools;

pub use manager::{LspManager, SharedLspManager};
