//! MCP client: high-level wrapper around a transport.
//!
//! Handles the `initialize` handshake and the `tools/list`, `tools/call`
//! methods. Each connected MCP server has exactly one McpClient.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::protocol::{
    CallToolResult, ClientInfo, ContentBlock, InitializeParams, InitializeResult, ListToolsResult,
    McpToolDescriptor, PROTOCOL_VERSION,
};
use super::transport::{StdioTransport, TransportError};

#[derive(Debug, Clone)]
pub struct ServerHandshakeInfo {
    pub server_name: String,
    pub server_version: String,
    pub protocol_version: String,
}

pub struct McpClient {
    pub transport: Arc<StdioTransport>,
    pub handshake: ServerHandshakeInfo,
    pub call_timeout: Duration,
}

impl McpClient {
    /// Spawn a stdio MCP server, perform the `initialize` handshake, and
    /// return a ready-to-use client.
    pub async fn connect_stdio(
        program: &str,
        args: &[String],
        env: &std::collections::HashMap<String, String>,
        cwd: Option<&std::path::Path>,
        connect_timeout: Duration,
    ) -> Result<Self, TransportError> {
        let transport = StdioTransport::spawn(program, args, env, cwd, connect_timeout).await?;
        let transport = Arc::new(transport);

        // Handshake.
        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION,
            capabilities: json!({
                "tools": {},
                "resources": {},
                "prompts": {}
            }),
            client_info: ClientInfo {
                name: "forge-osh",
                version: env!("CARGO_PKG_VERSION"),
            },
        };
        let value = serde_json::to_value(&params)
            .map_err(|e| TransportError::Encode(e.to_string()))?;
        let res = transport.request("initialize", Some(value)).await?;
        let init: InitializeResult = serde_json::from_value(res)
            .map_err(|e| TransportError::Decode(e.to_string()))?;

        // Send the `notifications/initialized` notification per spec.
        transport
            .notify("notifications/initialized", Some(json!({})))
            .await?;

        let info = init
            .server_info
            .map(|s| ServerHandshakeInfo {
                server_name: s.name,
                server_version: s.version,
                protocol_version: init.protocol_version.clone(),
            })
            .unwrap_or_else(|| ServerHandshakeInfo {
                server_name: "unknown".into(),
                server_version: String::new(),
                protocol_version: init.protocol_version.clone(),
            });

        Ok(Self {
            transport,
            handshake: info,
            call_timeout: connect_timeout,
        })
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, TransportError> {
        let res = self
            .transport
            .request("tools/list", Some(json!({})))
            .await?;
        let r: ListToolsResult = serde_json::from_value(res)
            .map_err(|e| TransportError::Decode(e.to_string()))?;
        Ok(r.tools)
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<(String, bool), TransportError> {
        let res = self
            .transport
            .request(
                "tools/call",
                Some(json!({
                    "name": name,
                    "arguments": arguments,
                })),
            )
            .await?;
        let r: CallToolResult = serde_json::from_value(res)
            .map_err(|e| TransportError::Decode(e.to_string()))?;
        Ok((ContentBlock::flatten(&r.content), r.is_error))
    }

    pub async fn shutdown(&self) {
        self.transport.shutdown().await;
    }
}
