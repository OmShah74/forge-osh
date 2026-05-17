//! MCP (Model Context Protocol) integration.
//!
//! Lets forge-osh consume tools from any standard MCP server. Public API:
//!
//! - [`McpManager`]    — owns server lifecycles
//! - [`catalog::CATALOG`] — built-in directory of known servers
//! - secrets are stored via the existing `KeyStore` under namespaced keys
//!
//! The TUI surfaces all of this through a `/mcp` modal.

pub mod catalog;
pub mod client;
pub mod manager;
pub mod protocol;
pub mod tool_adapter;
pub mod transport;

pub use manager::{McpManager, SecretSource, SecretStatus, ServerSnapshot, ServerStatus};
