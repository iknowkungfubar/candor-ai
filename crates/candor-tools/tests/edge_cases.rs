use candor_sandbox::SandboxPolicy;
use candor_sandbox::cross_platform::{Backoff, CircuitBreaker};
use candor_sandbox::policy::SandboxPolicyBuilder;
/// SWE-level edge case tests for tools and sandbox.
use candor_tools::registry::{Tool, ToolContext, ToolOutput, ToolRegistry};
use candor_tools::{ListDirTool, ReadFileTool, WriteFileTool};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn ctx() -> ToolContext {
    ToolContext {
        workdir: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        project_id: "test".into(),
    }
}

// ── ReadFile edge cases ──

#[tokio::test]
async fn read_file_nonexistent() {
    let tool = ReadFileTool;
    let result = tool.execute(&ctx(), &["/nonexistent/file.rs".into()]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn read_file_no_args() {
    let tool = ReadFileTool;
    let result = tool.execute(&ctx(), &[]).await;
    assert!(result.is_err());
}

// ── WriteFile edge cases ──

#[tokio::test]
async fn write_file_no_args() {
    let tool = WriteFileTool;
    let result = tool.execute(&ctx(), &[]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn write_file_empty_content() {
    let dir = std::env::temp_dir().join(format!("candor-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = ToolContext {
        workdir: dir.to_string_lossy().to_string(),
        project_id: "test".into(),
    };
    let tool = WriteFileTool;
    let result = tool
        .execute(&ctx, &["empty.txt".into(), "".into()])
        .await
        .unwrap();
    assert!(result.success);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── ListDir edge cases ──

#[tokio::test]
async fn list_dir_empty() {
    let dir = std::env::temp_dir().join(format!("candor-empty-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = ToolContext {
        workdir: dir.to_string_lossy().to_string(),
        project_id: "test".into(),
    };
    let tool = ListDirTool;
    let result = tool.execute(&ctx, &[]).await.unwrap();
    assert!(result.success);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Circuit breaker reset timeout ──

#[test]
fn circuit_breaker_half_open_resets() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(1));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(
        cb.state(),
        candor_sandbox::cross_platform::CircuitState::Open
    );
    // Wait for reset timeout (short enough for test)
    std::thread::sleep(Duration::from_millis(5));
    // After timeout, should transition to half-open on next allow()
    let _ = cb.allow();
    // After half-open, a success resets to closed
    cb.record_success();
    assert_eq!(
        cb.state(),
        candor_sandbox::cross_platform::CircuitState::Closed
    );
}

#[test]
fn circuit_breaker_allow_succeeds() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(10));
    assert!(cb.allow().is_ok());
}

#[test]
fn circuit_breaker_single_failure_stays_closed() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(10));
    cb.record_failure();
    assert_eq!(
        cb.state(),
        candor_sandbox::cross_platform::CircuitState::Closed
    );
    assert!(cb.allow().is_ok());
}

// ── Backoff edge cases ──

#[test]
fn backoff_initial_equals_first_delay() {
    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_secs(1));
    assert_eq!(backoff.next_delay(), Duration::from_millis(10));
}

#[test]
fn backoff_respects_max() {
    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_millis(15));
    backoff.next_delay(); // 10
    let d = backoff.next_delay(); // 20 capped at 15
    assert!(d <= Duration::from_millis(15));
}

#[test]
fn backoff_reset_returns_to_initial() {
    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_secs(1));
    backoff.next_delay();
    backoff.next_delay();
    backoff.reset();
    assert_eq!(backoff.next_delay(), Duration::from_millis(10));
}

#[test]
fn backoff_multiple_resets() {
    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_secs(1));
    for _ in 0..5 {
        backoff.next_delay();
        backoff.next_delay();
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(10));
        backoff.reset();
    }
}

// ── Sandbox policy edge cases ──

#[test]
fn sandbox_policy_default_values() {
    let p = SandboxPolicy::default();
    assert_eq!(p.timeout_secs, 15);
    assert_eq!(p.memory_limit_mb, Some(256));
    assert_eq!(p.fuel_limit, Some(1_000_000));
    assert!(!p.network_allowed);
    assert!(!p.env_allowed);
}

#[test]
fn sandbox_policy_builder_chaining() {
    let p = SandboxPolicyBuilder::new()
        .allow_network()
        .deny_network()
        .timeout_secs(30)
        .memory_limit_mb(512)
        .fuel_limit(5_000_000)
        .allow_read(Path::new("/tmp"))
        .allow_write(Path::new("/tmp"))
        .build();

    assert!(!p.network_allowed); // last call was deny_network
    assert_eq!(p.timeout_secs, 30);
    assert_eq!(p.memory_limit_mb, Some(512));
    assert_eq!(p.fuel_limit, Some(5_000_000));
}

// ── Tool registry edge cases ──

#[test]
fn tool_registry_find_nonexistent() {
    let registry = ToolRegistry::new();
    assert!(registry.find("nonexistent").is_none());
}

#[test]
fn tool_registry_list_all_empty() {
    let registry = ToolRegistry::new();
    assert!(registry.list_all().is_empty());
}

#[test]
fn tool_registry_descriptions_for_llm_sorted() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(candor_tools::GitBranchTool));
    registry.register(Arc::new(candor_tools::ReadFileTool));
    let desc = registry.descriptions_for_llm();
    assert!(desc.contains("git_branch"));
    assert!(desc.contains("read_file"));
}

// ── Tool output edge cases ──

#[test]
fn tool_output_ok_with_data() {
    let data = serde_json::json!({"key": "value"});
    let output = ToolOutput::ok_with_data("success", data.clone());
    assert!(output.success);
    assert_eq!(output.output, "success");
    assert_eq!(output.data, Some(data));
    assert!(output.error.is_none());
}

#[test]
fn tool_output_err() {
    let output = ToolOutput::err("failure");
    assert!(!output.success);
    assert!(output.output.is_empty());
    assert_eq!(output.error, Some("failure".into()));
}
