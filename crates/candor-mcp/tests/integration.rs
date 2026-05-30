/// Tests for MCP transport, client, and bridge modules.

// ── Transport tests ──

#[test]
fn test_parse_transport_http() {
    let transport = candor_mcp::transport::parse_transport("http://localhost:3000");
    assert!(transport.is_some());
}

#[test]
fn test_parse_transport_stdio() {
    let transport = candor_mcp::transport::parse_transport("stdio:echo hello");
    assert!(transport.is_some());
}

#[test]
fn test_parse_transport_invalid() {
    let transport = candor_mcp::transport::parse_transport("invalid:xyz");
    assert!(transport.is_none());
}

// ── Client tests (structural — don't require live MCP servers) ──

#[test]
fn test_mcp_tool_deserialization() {
    let json = serde_json::json!({
        "name": "test_tool",
        "description": "A test tool",
        "input_schema": {"type": "object"}
    });
    let tool: candor_mcp::client::McpTool = serde_json::from_value(json).unwrap();
    assert_eq!(tool.name, "test_tool");
    assert_eq!(tool.description, "A test tool");
}

#[test]
fn test_mcp_tool_default_description() {
    let json = serde_json::json!({
        "name": "minimal_tool",
        "input_schema": {}
    });
    let tool: candor_mcp::client::McpTool = serde_json::from_value(json).unwrap();
    assert_eq!(tool.name, "minimal_tool");
    assert_eq!(tool.description, "");
}

// ── Bridge tests ──

#[test]
fn test_parse_transport_multiple() {
    // Verify parsing doesn't crash on empty/multiple
    assert!(candor_mcp::transport::parse_transport("http://localhost:3000").is_some());
    assert!(candor_mcp::transport::parse_transport("").is_none());
    assert!(candor_mcp::transport::parse_transport("stdio:ls").is_some());
}

#[test]
fn test_http_transport_construction() {
    let _transport = candor_mcp::transport::HttpTransport::new("http://localhost:3000".into());
    // Just verify it doesn't panic
    assert!(true);
}

#[test]
fn test_stdio_transport_construction() {
    let _transport = candor_mcp::transport::StdioTransport::new(
        "echo".into(),
        vec!["test".into()],
    );
    assert!(true);
}
