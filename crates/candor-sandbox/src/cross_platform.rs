/// Cross-platform sandbox detection and resilience patterns.
///
/// From design doc Phase 2, Action Item 2.3:
/// "Verify the abstraction triggers bubblewrap on Linux, Seatbelt on macOS,
/// and AppContainer on Windows when legacy binary execution is requested."
use std::time::Duration;
use tracing::{info, warn};

use candor_core::error::CoreError;

/// Platform-specific sandbox information.
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    pub os: String,
    pub sandbox_type: SandboxType,
    pub bwrap_available: bool,
    pub seatbelt_available: bool,
    pub appcontainer_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxType {
    Bubblewrap,
    Seatbelt,
    AppContainer,
    Direct,
}

impl PlatformInfo {
    pub fn detect() -> Self {
        let os = std::env::consts::OS.to_string();

        let (sandbox_type, bwrap, seatbelt, appcontainer) = if cfg!(target_os = "linux") {
            let b = is_bwrap_available();
            if b {
                info!("Linux: bubblewrap detected");
                (SandboxType::Bubblewrap, true, false, false)
            } else {
                warn!("Linux: bubblewrap not found — using direct execution");
                (SandboxType::Direct, false, false, false)
            }
        } else if cfg!(target_os = "macos") {
            let s = is_seatbelt_available();
            if s {
                info!("macOS: Seatbelt sandbox detected");
                (SandboxType::Seatbelt, false, true, false)
            } else {
                warn!("macOS: Seatbelt not available — using direct execution");
                (SandboxType::Direct, false, false, false)
            }
        } else if cfg!(target_os = "windows") {
            info!("Windows: using AppContainer isolation");
            (SandboxType::AppContainer, false, false, true)
        } else {
            (SandboxType::Direct, false, false, false)
        };

        Self {
            os,
            sandbox_type,
            bwrap_available: bwrap,
            seatbelt_available: seatbelt,
            appcontainer_available: appcontainer,
        }
    }

    pub fn is_isolated(&self) -> bool {
        self.sandbox_type != SandboxType::Direct
    }
}

fn is_bwrap_available() -> bool {
    std::process::Command::new("bwrap")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_seatbelt_available() -> bool {
    std::process::Command::new("sandbox-exec")
        .arg("-h")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── Circuit Breaker ──

/// Circuit breaker pattern for external API calls.
/// After `failure_threshold` consecutive failures, the circuit opens
/// and all calls fail fast until the `reset_timeout` elapses.
pub struct CircuitBreaker {
    failure_count: std::sync::atomic::AtomicU32,
    state: std::sync::atomic::AtomicU8, // 0=closed, 1=open, 2=half-open
    failure_threshold: u32,
    reset_timeout: Duration,
    last_failure: std::sync::Mutex<Option<std::time::Instant>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            failure_count: std::sync::atomic::AtomicU32::new(0),
            state: std::sync::atomic::AtomicU8::new(0),
            failure_threshold,
            reset_timeout,
            last_failure: std::sync::Mutex::new(None),
        }
    }

    pub fn state(&self) -> CircuitState {
        match self.state.load(std::sync::atomic::Ordering::SeqCst) {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            _ => CircuitState::HalfOpen,
        }
    }

    /// Check if a call is allowed. Returns Ok(()) if allowed, Err if circuit is open.
    pub fn allow(&self) -> Result<(), CoreError> {
        let current_state = self.state();

        match current_state {
            CircuitState::Closed => Ok(()),
            CircuitState::HalfOpen => Ok(()),
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                if let Ok(guard) = self.last_failure.lock() {
                    if let Some(last) = *guard {
                        if last.elapsed() >= self.reset_timeout {
                            // Transition to half-open
                            self.state.store(2, std::sync::atomic::Ordering::SeqCst);
                            info!("Circuit breaker: open → half-open");
                            return Ok(());
                        }
                    }
                }
                Err(CoreError::Internal(
                    "Circuit breaker is open — API calls suspended".into(),
                ))
            }
        }
    }

    /// Record a successful call.
    pub fn record_success(&self) {
        self.failure_count.store(0, std::sync::atomic::Ordering::SeqCst);
        self.state.store(0, std::sync::atomic::Ordering::SeqCst);
    }

    /// Record a failed call.
    pub fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if let Ok(mut guard) = self.last_failure.lock() {
            *guard = Some(std::time::Instant::now());
        }

        if count >= self.failure_threshold {
            self.state.store(1, std::sync::atomic::Ordering::SeqCst);
            warn!(
                count = count,
                threshold = self.failure_threshold,
                "Circuit breaker: closed → open"
            );
        }
    }
}

// ── Exponential Backoff ──

/// Exponential backoff with jitter for retryable operations.
pub struct Backoff {
    initial: Duration,
    max: Duration,
    multiplier: f64,
    current: Duration,
}

impl Backoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max,
            multiplier: 2.0,
            current: initial,
        }
    }

    /// Get the next delay and advance the backoff.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = std::cmp::min(
            Duration::from_secs_f64(
                self.current.as_secs_f64() * self.multiplier,
            ),
            self.max,
        );
        delay
    }

    /// Reset the backoff to initial.
    pub fn reset(&mut self) {
        self.current = self.initial;
    }

    /// Sleep for the current delay and advance.
    pub async fn wait(&mut self) {
        tokio::time::sleep(self.next_delay()).await;
    }
}

// ── Retry with Backoff ──

/// Execute an async operation with exponential backoff retry.
pub async fn with_retry<F, Fut, T>(
    max_attempts: u32,
    backoff: &mut Backoff,
    mut f: F,
) -> Result<T, CoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, CoreError>>,
{
    let mut last_error = None;

    for attempt in 0..max_attempts {
        match f().await {
            Ok(result) => {
                backoff.reset();
                return Ok(result);
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < max_attempts - 1 {
                    warn!(attempt = attempt + 1, max = max_attempts, "Retrying...");
                    backoff.wait().await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        CoreError::Internal("Retry exhausted with no error".into())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let info = PlatformInfo::detect();
        assert!(!info.os.is_empty());
    }

    #[test]
    fn test_circuit_breaker_closed_by_default() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow().is_ok());
    }

    #[test]
    fn test_circuit_breaker_opens_after_failures() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_resets_after_success() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_backoff_exponential() {
        let mut backoff = Backoff::new(
            Duration::from_millis(10),
            Duration::from_secs(1),
        );
        let d1 = backoff.next_delay();
        let d2 = backoff.next_delay();
        assert!(d2 > d1);
        backoff.reset();
        assert_eq!(backoff.next_delay(), d1);
    }
}
