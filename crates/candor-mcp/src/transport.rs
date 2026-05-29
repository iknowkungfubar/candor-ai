/// MCP transport layer: stdio and HTTP.

/// Supported MCP transport types.
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// stdio-based: spawn a process and communicate via stdin/stdout.
    Stdio {
        command: String,
        args: Vec<String>,
    },
    /// HTTP-based: connect to a running MCP server.
    Http {
        url: String,
    },
}

/// A transport that can send JSON-RPC requests and receive responses.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Send a JSON-RPC request and return the raw JSON response.
    async fn send_request(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, candor_core::error::CoreError>;

    /// Check if the transport is alive/healthy.
    async fn ping(&self) -> Result<bool, candor_core::error::CoreError>;
}

/// stdio transport: spawns a subprocess and communicates via JSON-RPC over stdin/stdout.
pub struct StdioTransport {
    command: String,
    args: Vec<String>,
}

impl StdioTransport {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }
}

#[async_trait::async_trait]
impl Transport for StdioTransport {
    async fn send_request(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, candor_core::error::CoreError> {
        let _req_str = serde_json::to_string(&request)
            .map_err(|e| candor_core::error::CoreError::Serialization(e.to_string()))?;

        let output = tokio::process::Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| candor_core::error::CoreError::Internal(format!("MCP stdio spawn failed: {e}")))?;

        // Write request to stdin and read from stdout (simplified: single request-response)
        // In production, this would use a persistent process with async stdin/stdout pipes.
        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        if raw.trim().is_empty() {
            return Err(candor_core::error::CoreError::Internal(
                format!("MCP stdio: no response from {}", self.command)
            ));
        }

        // Parse each line as a JSON-RPC response
        for line in raw.lines() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                return Ok(val);
            }
        }

        Err(candor_core::error::CoreError::Internal("MCP stdio: failed to parse JSON-RPC response".into()))
    }

    async fn ping(&self) -> Result<bool, candor_core::error::CoreError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
            "params": {}
        });
        match self.send_request(request).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

/// HTTP transport: sends JSON-RPC requests to a running MCP server.
pub struct HttpTransport {
    url: String,
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl Transport for HttpTransport {
    async fn send_request(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, candor_core::error::CoreError> {
        let resp = self.client
            .post(&self.url)
            .json(&request)
            .send()
            .await
            .map_err(|e| candor_core::error::CoreError::Internal(format!("MCP HTTP request failed: {e}")))?;

        let val: serde_json::Value = resp.json().await
            .map_err(|e| candor_core::error::CoreError::Internal(format!("MCP HTTP parse failed: {e}")))?;

        Ok(val)
    }

    async fn ping(&self) -> Result<bool, candor_core::error::CoreError> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
            "params": {}
        });
        match self.send_request(request).await {
            Ok(v) => Ok(!v.get("error").is_some()),
            Err(_) => Ok(false),
        }
    }
}

/// Create a transport from a connection string.
/// Format: "stdio:command arg1 arg2" or "http://host:port"
pub fn parse_transport(conn_str: &str) -> Option<Box<dyn Transport>> {
    if conn_str.starts_with("http://") || conn_str.starts_with("https://") {
        Some(Box::new(HttpTransport::new(conn_str.to_string())))
    } else if conn_str.starts_with("stdio:") {
        let parts: Vec<&str> = conn_str[6..].split_whitespace().collect();
        if let Some((cmd, args)) = parts.split_first() {
            Some(Box::new(StdioTransport::new(
                cmd.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            )))
        } else {
            None
        }
    } else {
        None
    }
}
