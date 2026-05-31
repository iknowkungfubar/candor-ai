// candor-mcp: Model Context Protocol client.
// Connects to external MCP servers to extend the agent's capability set.
// Implements JSON-RPC 2.0 over stdio and HTTP transports.

pub mod bridge;
pub mod client;
pub mod transport;

pub use bridge::McpToolBridge;
pub use client::McpClient;
