/// Recovery system: failure recovery nodes for the graph runner.
///
/// From the design doc: "Failure Is the Primary Use Case. The graph is
/// designed to handle failure before success. The system models failure
/// modes and ensures robust recovery."
///
/// "The adk-graph utilizes checkpointing so that when a tool traps or
/// an API timeouts, the graph restores state and triggers the
/// deterministic recovery node."
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use candor_core::error::CoreError;
use candor_core::state::AgentState;

use super::node::AgentNode;

/// A recovery node that attempts to recover from specific failure modes.
pub struct RecoveryNode {
    pub name: String,
    /// Maximum number of retry attempts before giving up.
    pub max_retries: u32,
    /// How many times this node has been tried.
    attempt: Arc<std::sync::atomic::AtomicU32>,
}

impl RecoveryNode {
    pub fn new(name: impl Into<String>, max_retries: u32) -> Self {
        Self {
            name: name.into(),
            max_retries,
            attempt: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    pub fn attempt_count(&self) -> u32 {
        self.attempt.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl AgentNode for RecoveryNode {
    fn name(&self) -> &str {
        &self.name
    }

    async fn execute(
        &self,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError> {
        let attempt = self.attempt.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;

        if attempt > self.max_retries {
            error!(
                recovery = %self.name,
                attempts = attempt,
                "Recovery node exhausted retries — escalating failure"
            );
            return Err(CoreError::Internal(format!(
                "Recovery '{}' failed after {} attempts",
                self.name, attempt
            )));
        }

        info!(
            recovery = %self.name,
            attempt = attempt,
            "Recovery node executing"
        );

        // Recovery actions based on state
        {
            let mut s = state.lock().await;

            // Reset sentinel approval for retry
            s.sentinel_approved = false;

            // Log the recovery attempt
            s.log_event(&format!(
                "Recovery: {} (attempt {}/{})",
                self.name,
                attempt,
                self.max_retries
            ));

            // If we've been looping, reset compaction flag
            if s.compaction_required {
                s.compaction_required = false;
                s.log_event("Recovery: resetting compaction flag for retry");
            }

            // Step back to the previous phase if possible
            let phase_clone = s.current_phase.clone();
            drop(s);
            if let Some(ref phase) = phase_clone {
                let mut s2 = state.lock().await;
                s2.log_event(&format!(
                    "Recovery: retrying from phase '{}'",
                    phase
                ));
            }
        }

        Ok(())
    }
}

/// Analyzes an error and determines the appropriate recovery strategy.
pub fn analyze_error(error: &CoreError) -> RecoveryStrategy {
    match error {
        CoreError::MaxIterationsReached => RecoveryStrategy {
            retry: false,
            reason: "Max iterations reached — cannot recover".into(),
            escalate: true,
        },
        CoreError::SandboxTrap(_) => RecoveryStrategy {
            retry: false,
            reason: "Sandbox trap — execution cannot be retried without changes".into(),
            escalate: true,
        },
        CoreError::SandboxResourceExhausted => RecoveryStrategy {
            retry: true,
            reason: "Resource exhausted — retry with increased limits".into(),
            escalate: false,
        },
        CoreError::Inference(_) => RecoveryStrategy {
            retry: true,
            reason: "Inference error — retry with fallback model".into(),
            escalate: false,
        },
        CoreError::MemorySystem(_) => RecoveryStrategy {
            retry: false,
            reason: "Memory system error — cannot recover without storage".into(),
            escalate: true,
        },
        CoreError::SentinelPolicyViolation(_) => RecoveryStrategy {
            retry: false,
            reason: "Sentinel policy violation — must fix the violation first".into(),
            escalate: true,
        },
        CoreError::SentinelSemanticRejection(_) => RecoveryStrategy {
            retry: false,
            reason: "Semantic rejection — payload must be reworked".into(),
            escalate: true,
        },
        CoreError::IdealStateNotSatisfied(_) => RecoveryStrategy {
            retry: true,
            reason: "ISA not satisfied — retry with revised approach".into(),
            escalate: false,
        },
        CoreError::HumanApprovalDenied => RecoveryStrategy {
            retry: false,
            reason: "Human approval denied — cannot proceed".into(),
            escalate: true,
        },
        CoreError::StateCorruption(_) => RecoveryStrategy {
            retry: false,
            reason: "State corruption — must restore from checkpoint".into(),
            escalate: true,
        },
        CoreError::Config(_) => RecoveryStrategy {
            retry: false,
            reason: "Configuration error — must fix config".into(),
            escalate: true,
        },
        CoreError::Io(_) => RecoveryStrategy {
            retry: true,
            reason: "IO error — retry with backoff".into(),
            escalate: false,
        },
        CoreError::Serialization(_) => RecoveryStrategy {
            retry: false,
            reason: "Serialization error — data corruption".into(),
            escalate: true,
        },
        CoreError::Internal(_) => RecoveryStrategy {
            retry: true,
            reason: "Internal error — retry once".into(),
            escalate: false,
        },
        _ => RecoveryStrategy {
            retry: false,
            reason: "Unknown error — escalating".into(),
            escalate: true,
        },
    }
}

/// The recommended recovery strategy for an error.
#[derive(Debug, Clone)]
pub struct RecoveryStrategy {
    /// Whether retrying is recommended.
    pub retry: bool,
    /// Human-readable reason for the strategy.
    pub reason: String,
    /// Whether this error should be escalated (terminate the task).
    pub escalate: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_recovery_node_within_limits() {
        let node = RecoveryNode::new("test-recovery", 3);
        let state = Arc::new(Mutex::new(AgentState::default()));

        // First attempt should succeed
        let result = node.execute(Arc::clone(&state)).await;
        assert!(result.is_ok());
        assert_eq!(node.attempt_count(), 1);
    }

    #[tokio::test]
    async fn test_recovery_node_exhausted() {
        let node = RecoveryNode::new("test-recovery", 2);
        let state = Arc::new(Mutex::new(AgentState::default()));

        // Use up retries
        let _ = node.execute(Arc::clone(&state)).await;
        let _ = node.execute(Arc::clone(&state)).await;

        // Third attempt should fail
        let result = node.execute(Arc::clone(&state)).await;
        assert!(result.is_err());
        assert_eq!(node.attempt_count(), 3);
    }

    #[test]
    fn test_analyze_error_retry_for_inference() {
        let strategy = analyze_error(&CoreError::Inference("timeout".into()));
        assert!(strategy.retry);
        assert!(!strategy.escalate);
    }

    #[test]
    fn test_analyze_error_escalate_for_policy_violation() {
        let strategy = analyze_error(&CoreError::SentinelPolicyViolation("blocked".into()));
        assert!(!strategy.retry);
        assert!(strategy.escalate);
    }

    #[test]
    fn test_analyze_error_retry_for_io() {
        let strategy = analyze_error(&CoreError::Io("connection refused".into()));
        assert!(strategy.retry);
        assert!(!strategy.escalate);
    }

    #[test]
    fn test_analyze_error_escalate_for_max_iterations() {
        let strategy = analyze_error(&CoreError::MaxIterationsReached);
        assert!(!strategy.retry);
        assert!(strategy.escalate);
    }
}
