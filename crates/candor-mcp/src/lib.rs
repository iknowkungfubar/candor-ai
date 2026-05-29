// candor-mcp: Model Context Protocol client.
// Connects to external MCP servers to extend the agent's capability set.
// Implements JSON-RPC 2.0 over stdio and HTTP transports.

pub mod client;
pub mod transport;
pub mod bridge;

pub use client::McpClient;
pub use bridge::McpToolBridge;
