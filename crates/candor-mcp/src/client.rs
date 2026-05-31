/// MCP client: JSON-RPC 2.0 client for Model Context Protocol servers.
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

use candor_core::error::CoreError;

use super::transport::Transport;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A connected MCP server client.
pub struct McpClient {
    transport: Box<dyn Transport>,
    name: String,
    _version: String,
    tools: Vec<McpTool>,
}

/// A tool discovered from an MCP server.
#[derive(Debug, Clone, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

impl McpClient {
    /// Connect to an MCP server and initialize the session.
    pub async fn connect(name: String, transport: Box<dyn Transport>) -> Result<Self, CoreError> {
        info!(server = %name, "Connecting to MCP server");

        // Initialize
        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": NEXT_ID.fetch_add(1, Ordering::SeqCst),
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "candor-ai",
                    "version": "0.1.0"
                }
            }
        });

        let init_resp = transport.send_request(init_req).await?;
        let version = init_resp["result"]["protocolVersion"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        info!(server = %name, version = %version, "MCP initialized");
        Ok(Self {
            transport,
            name,
            _version: version,
            tools: Vec::new(),
        })
    }

    /// Discover tools available on the server.
    pub async fn discover_tools(&mut self) -> Result<(), CoreError> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": NEXT_ID.fetch_add(1, Ordering::SeqCst),
            "method": "tools/list",
            "params": {}
        });

        let resp = self.transport.send_request(req).await?;

        if let Some(tools) = resp["result"]["tools"].as_array() {
            for tool in tools {
                if let Ok(mcp_tool) = serde_json::from_value::<McpTool>(tool.clone()) {
                    info!(tool = %mcp_tool.name, "Discovered MCP tool");
                    self.tools.push(mcp_tool);
                }
            }
        }

        info!(count = self.tools.len(), server = %self.name, "MCP tools discovered");
        Ok(())
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, CoreError> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": NEXT_ID.fetch_add(1, Ordering::SeqCst),
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        let resp = self.transport.send_request(req).await?;

        if let Some(err) = resp.get("error") {
            return Err(CoreError::Internal(format!(
                "MCP tool '{}' error: {}",
                tool_name,
                err["message"].as_str().unwrap_or("unknown")
            )));
        }

        // Extract content from the response
        let content = resp["result"]["content"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| format!("{}", resp["result"]));

        Ok(content)
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
