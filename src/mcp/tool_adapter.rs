//! Adapter that wraps a remote MCP tool as a local `crate::tools::Tool`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::client::McpClient;
use crate::tools::Tool;
use crate::types::{PermissionLevel, ToolContext, ToolOutput};

pub struct McpTool {
    /// Local prefixed name: `mcp__<server>__<remote_name>`.
    pub local_name: String,
    pub server_id: String,
    pub remote_name: String,
    pub description_text: String,
    pub schema: Value,
    pub default_permission: PermissionLevel,
    pub client: Arc<McpClient>,
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.local_name
    }

    fn description(&self) -> &str {
        &self.description_text
    }

    fn parameters_schema(&self) -> Value {
        // Ensure we always return an object schema. If the server gave us
        // something weird, fall back to a permissive empty object.
        if self.schema.is_object()
            && self.schema.get("type").and_then(|t| t.as_str()) == Some("object")
        {
            self.schema.clone()
        } else if self.schema.is_object() {
            // Wrap.
            let mut o = serde_json::Map::new();
            o.insert("type".into(), Value::String("object".into()));
            o.insert("properties".into(), self.schema.clone());
            Value::Object(o)
        } else {
            serde_json::json!({ "type": "object", "properties": {} })
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        self.default_permission.clone()
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolOutput {
        match self.client.call_tool(&self.remote_name, input).await {
            Ok((text, is_err)) => {
                if is_err {
                    ToolOutput::error(text)
                } else {
                    ToolOutput::success(text)
                }
            }
            Err(e) => ToolOutput::error(format!("MCP call failed: {e}")),
        }
    }
}
