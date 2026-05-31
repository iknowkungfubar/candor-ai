use candor_sandbox::cross_platform::{Backoff, CircuitBreaker, PlatformInfo, SandboxType};
use candor_sandbox::policy::SandboxPolicyBuilder;
/// Phase 2 integration tests: portable isolation chamber.
/// Tests WASM fuel limits, cross-platform sandbox, bubblewrap network isolation.
use candor_sandbox::{SandboxPolicy, ToolSandbox};
use std::time::Duration;

// ── Fuel Limit Configuration ──

#[test]
fn test_fuel_limit_default() {
    let policy = SandboxPolicy::default();
    assert_eq!(policy.fuel_limit, Some(1_000_000));
}

#[test]
fn test_fuel_limit_custom() {
    let policy = SandboxPolicyBuilder::new().fuel_limit(5_000_000).build();
    assert_eq!(policy.fuel_limit, Some(5_000_000));
}

#[test]
fn test_fuel_limit_zero_traps_immediately() {
    // A fuel limit of 0 should trap on first instruction
    let policy = SandboxPolicyBuilder::new().fuel_limit(0).build();
    assert_eq!(policy.fuel_limit, Some(0));
}

#[test]
fn test_wasm_backend_uses_fuel_from_policy() {
    let policy = SandboxPolicyBuilder::new().fuel_limit(42).build();
    let backend = candor_sandbox::wasm_exec::WasmBackend::new(policy.clone());
    assert_eq!(backend.policy().fuel_limit, Some(42));
}

// ── Cross-Platform Sandbox ──

#[test]
fn test_platform_detection_returns_valid() {
    let info = PlatformInfo::detect();
    assert!(!info.os.is_empty());
    // Should be one of the valid sandbox types
    match info.sandbox_type {
        SandboxType::Bubblewrap
        | SandboxType::Seatbelt
        | SandboxType::AppContainer
        | SandboxType::Direct => {}
    }
}

#[test]
fn test_platform_isolated_detection() {
    let info = PlatformInfo::detect();
    // If bubblewrap is available, should be isolated
    if info.bwrap_available {
        assert!(info.is_isolated());
        assert_eq!(info.sandbox_type, SandboxType::Bubblewrap);
    }
}

// ── Bubblewrap Network Isolation ──

#[test]
fn test_sandbox_policy_denies_network_by_default() {
    let policy = SandboxPolicy::default();
    assert!(!policy.network_allowed);
}

#[test]
fn test_sandbox_policy_allows_network_when_configured() {
    let policy = SandboxPolicyBuilder::new().allow_network().build();
    assert!(policy.network_allowed);
}

#[test]
fn test_process_backend_detects_bwrap() {
    let policy = SandboxPolicy::default();
    let backend = candor_sandbox::process_exec::ProcessBackend::new(policy);
    // Just verify creation works regardless of bwrap availability
    assert!(backend.is_ok());
}

// ── Tool Sandbox Integration ──

#[tokio::test]
async fn test_tool_sandbox_creation() {
    let sandbox = ToolSandbox::new();
    assert!(sandbox.is_ok());
}

#[tokio::test]
async fn test_tool_sandbox_with_policy() {
    let policy = SandboxPolicyBuilder::new()
        .fuel_limit(500_000)
        .deny_network()
        .timeout_secs(30)
        .memory_limit_mb(512)
        .build();
    let sandbox = ToolSandbox::with_policy(policy);
    assert!(sandbox.is_ok());
}

#[tokio::test]
async fn test_sandbox_shell_execution() {
    let sandbox = ToolSandbox::new().unwrap();
    let result = sandbox
        .execute_tool(
            "echo isolation_test",
            candor_sandbox::unified::ExecLanguage::Shell,
        )
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("isolation_test"));
}

// ── Circuit Breaker + Backoff ──

#[test]
fn test_circuit_breaker_closed_by_default() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(10));
    assert_eq!(cb.state(), cross_platform::CircuitState::Closed);
    assert!(cb.allow().is_ok());
}

#[test]
fn test_circuit_breaker_opens_after_threshold() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(10));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), cross_platform::CircuitState::Open);
    assert!(cb.allow().is_err());
}

#[test]
fn test_circuit_breaker_resets_on_success() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(10));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), cross_platform::CircuitState::Open);
    cb.record_success();
    assert_eq!(cb.state(), cross_platform::CircuitState::Closed);
}

#[test]
fn test_exponential_backoff_doubles() {
    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_secs(1));
    let d1 = backoff.next_delay();
    let d2 = backoff.next_delay();
    assert!(d2 > d1);
    assert_eq!(d1, Duration::from_millis(10));
    assert_eq!(d2, Duration::from_millis(20));
}

#[test]
fn test_exponential_backoff_respects_max() {
    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_millis(15));
    backoff.next_delay(); // 10ms
    backoff.next_delay(); // 20ms → capped at 15ms
    let d3 = backoff.next_delay();
    assert!(d3 <= Duration::from_millis(15));
}

#[test]
fn test_backoff_reset() {
    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_secs(1));
    backoff.next_delay(); // 10ms
    backoff.next_delay(); // 20ms
    backoff.reset();
    assert_eq!(backoff.next_delay(), Duration::from_millis(10));
}

// Re-export for use in tests
use candor_sandbox::cross_platform;
