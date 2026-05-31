/// Integration tests for candor-sandbox.
use candor_sandbox::{SandboxPolicy, ToolSandbox};
use std::path::Path;

#[tokio::test]
async fn test_sandbox_shell_hello() {
    let sandbox = ToolSandbox::new().unwrap();
    let result = sandbox
        .execute_tool(
            "echo sandboxed",
            candor_sandbox::unified::ExecLanguage::Shell,
        )
        .await
        .unwrap();
    assert!(result.contains("sandboxed"));
}

#[tokio::test]
async fn test_sandbox_shell_multiline() {
    let sandbox = ToolSandbox::new().unwrap();
    let code = "echo line1 && echo line2";
    let result = sandbox
        .execute_tool(code, candor_sandbox::unified::ExecLanguage::Shell)
        .await
        .unwrap();
    assert!(result.contains("line1"));
    assert!(result.contains("line2"));
}

#[test]
fn test_policy_deny_network_by_default() {
    let policy = SandboxPolicy::default();
    assert!(!policy.network_allowed);
    assert!(!policy.env_allowed);
    assert_eq!(policy.timeout_secs, 15);
    assert_eq!(policy.memory_limit_mb, Some(256));
    assert_eq!(policy.fuel_limit, Some(1_000_000));
}

#[test]
fn test_policy_builder_with_read_write() {
    // Test the builder API through the SandboxPolicy struct directly
    let mut policy = SandboxPolicy::default();
    policy.read_allowed.push(Path::new("/tmp").to_path_buf());
    policy.write_allowed.push(Path::new("/tmp").to_path_buf());

    // Read-allowed should now have both the default scratchpad and /tmp
    assert_eq!(policy.read_allowed.len(), 2);
    // Write-allowed should also have both
    assert_eq!(policy.write_allowed.len(), 2);
}

#[test]
fn test_process_backend_construction() {
    // Verify ProcessBackend constructs via ToolSandbox
    let sandbox = ToolSandbox::new().unwrap();
    let native = sandbox.native_engine();
    // bwrap availability depends on host — just verify no panic
    let _ = native.is_bwrap_available();
}

#[test]
fn test_wasm_backend_construction() {
    let sandbox = ToolSandbox::new().unwrap();
    let wasm = sandbox.wasm_engine();
    let policy = wasm.policy();
    assert!(!policy.network_allowed);
}
