//! JSON-RPC 2.0 envelope types and MCP-specific request/response shapes.
//!
//! MCP uses JSON-RPC 2.0 (https://www.jsonrpc.org/specification) over a chosen
//! transport (stdio, http+sse, streamable-http). This module models *only*
//! what we send/receive — wire framing lives in `transport.rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC_VERSION: &str = "2.0";
pub const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Any frame received from the server: response, notification, or request.
#[derive(Debug, Clone)]
pub enum InboundFrame {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    /// Server-initiated request — we currently acknowledge with a method-not-found error.
    Request(JsonRpcRequest),
}

impl InboundFrame {
    pub fn parse(s: &str) -> Result<Self, String> {
        let v: Value =
            serde_json::from_str(s).map_err(|e| format!("invalid JSON: {e}: {}", truncate(s)))?;
        if v.get("method").is_some() {
            if v.get("id").is_some() {
                let req: JsonRpcRequest =
                    serde_json::from_value(v).map_err(|e| format!("bad request: {e}"))?;
                Ok(InboundFrame::Request(req))
            } else {
                let n: JsonRpcNotification =
                    serde_json::from_value(v).map_err(|e| format!("bad notification: {e}"))?;
                Ok(InboundFrame::Notification(n))
            }
        } else {
            let r: JsonRpcResponse =
                serde_json::from_value(v).map_err(|e| format!("bad response: {e}"))?;
            Ok(InboundFrame::Response(r))
        }
    }
}

fn truncate(s: &str) -> String {
    if s.len() > 200 {
        format!("{}...", &s[..200])
    } else {
        s.to_string()
    }
}

// ── MCP method-specific param/result shapes ────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct InitializeParams<'a> {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: &'a str,
    pub capabilities: Value,
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo<'a> {
    pub name: &'a str,
    pub version: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion", default)]
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(rename = "serverInfo", default)]
    pub server_info: Option<ServerInfo>,
    #[serde(default)]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpToolDescriptor {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListToolsResult {
    #[serde(default)]
    pub tools: Vec<McpToolDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CallToolResult {
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        #[serde(default)]
        data: String,
        #[serde(rename = "mimeType", default)]
        mime_type: String,
    },
    #[serde(rename = "resource")]
    Resource {
        #[serde(default)]
        resource: Value,
    },
    #[serde(other)]
    Unknown,
}

impl ContentBlock {
    pub fn flatten(blocks: &[ContentBlock]) -> String {
        let mut out = String::new();
        for b in blocks {
            match b {
                ContentBlock::Text { text } => {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(text);
                }
                ContentBlock::Image { mime_type, data } => {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&format!(
                        "[image: {} ({} bytes base64)]",
                        if mime_type.is_empty() {
                            "image/*"
                        } else {
                            mime_type
                        },
                        data.len()
                    ));
                }
                ContentBlock::Resource { resource } => {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&format!("[resource] {}", resource));
                }
                ContentBlock::Unknown => {}
            }
        }
        out
    }
}
