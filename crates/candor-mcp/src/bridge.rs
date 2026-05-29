/// Bridge between MCP tools and Candor's Tool trait.
use std::sync::Arc;
use tracing::info;

use candor_core::error::CoreError;
use candor_tools::registry::{Tool, ToolContext, ToolOutput};

use super::client::McpClient;
use super::transport;

/// Wraps an MCP server connection as a Candor tool.
/// Each MCP tool becomes accessible through the Tool trait.
pub struct McpToolBridge {
    client: Arc<tokio::sync::Mutex<McpClient>>,
}

impl McpToolBridge {
    /// Connect to one or more MCP servers specified in a connection string.
    /// Format: comma-separated transport strings.
    /// Example: "http://localhost:3000,stdio:mcp-server --port 9000"
    pub async fn connect_all(
        conn_str: &str,
    ) -> Result<Vec<Arc<tokio::sync::Mutex<McpClient>>>, CoreError> {
        let mut clients = Vec::new();

        for part in conn_str.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            let transport = transport::parse_transport(part)
                .ok_or_else(|| CoreError::Internal(format!("Invalid MCP transport: {part}")))?;

            let mut client = McpClient::connect(
                format!("mcp-{}", part),
                transport,
            )
            .await?;

            client.discover_tools().await?;
            info!(
                server = %client.name(),
                tools = client.tools().len(),
                "MCP tools registered"
            );

            clients.push(Arc::new(tokio::sync::Mutex::new(client)));
        }

        Ok(clients)
    }
}

/// An MCP tool registered as a Candor Tool.
pub struct McpWrappedTool {
    tool_name: String,
    description: String,
    client: Arc<tokio::sync::Mutex<McpClient>>,
}

impl McpWrappedTool {
    pub fn new(
        tool_name: String,
        description: String,
        client: Arc<tokio::sync::Mutex<McpClient>>,
    ) -> Self {
        Self { tool_name, description, client }
    }
}

#[async_trait::async_trait]
impl Tool for McpWrappedTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn execute(
        &self,
        _ctx: &ToolContext,
        args: &[String],
    ) -> Result<ToolOutput, CoreError> {
        let client = self.client.lock().await;
        let arguments = if args.is_empty() {
            serde_json::json!({})
        } else if args.len() == 1 {
            // Try parsing as JSON
            serde_json::from_str(&args[0])
                .unwrap_or_else(|_| serde_json::json!({"input": args[0]}))
        } else {
            serde_json::json!({"args": args})
        };

        match client.call_tool(&self.tool_name, arguments).await {
            Ok(output) => Ok(ToolOutput::ok(output)),
            Err(e) => Ok(ToolOutput::err(format!("MCP call failed: {e}"))),
        }
    }
}
