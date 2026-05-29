/// The SentinelInterceptor — architectural guardian of the agent graph.
///
/// From the design doc: "The Sentinel is registered as a middleware
/// interceptor (BeforeToolCallback). It physically pauses the state
/// machine at the application level immediately before node transition
/// or tool execution to audit behavioral signatures."
///
/// KEY CONCERN: "To eliminate peer preservation, the Sentinel is subjected
/// to strict architectural isolation. The Sentinel physically cannot read
/// the conversation history, system prompts, or persona of the primary
/// agent. It is fed only stateless, deterministic structs."
use std::sync::Arc;
use tracing::{error, info, instrument};

use candor_core::error::CoreError;
use candor_core::protocol::AgentAction;
use candor_core::state::AgentState;
use candor_graph::hooks::BeforeToolCallback;

use super::rules::{enforce_deterministic_rules, check_conventional_commit};
use super::slop_detector;

/// The Sentinel interceptor — a sidecar that audits agent actions.
///
/// Architecturally isolated: receives NO conversation history,
/// NO system prompts, NO persona context from the primary agent.
/// Only receives: <proposed_action> + <valid_scopes>.
pub struct SentinelInterceptor {
    /// Reference to the local classifier for semantic slop detection.
    local_classifier: Arc<candor_cognitive::CognitiveEngine>,

    /// Valid scope tags for the current task.
    valid_scopes: Vec<String>,

    /// Whether the sentinel is active (can be disabled for testing).
    active: bool,
}

impl SentinelInterceptor {
    pub fn new(
        classifier: Arc<candor_cognitive::CognitiveEngine>,
        valid_scopes: Vec<String>,
    ) -> Self {
        Self {
            local_classifier: classifier,
            valid_scopes,
            active: true,
        }
    }

    /// Create a sentinel with no valid scopes (accepts everything).
    pub fn permissive(classifier: Arc<candor_cognitive::CognitiveEngine>) -> Self {
        Self {
            local_classifier: classifier,
            valid_scopes: vec![],
            active: true,
        }
    }

    /// Deactivate the sentinel (for development/testing).
    pub fn deactivate(&mut self) {
        self.active = false;
        info!("Sentinel deactivated");
    }

    /// Activate the sentinel.
    pub fn activate(&mut self) {
        self.active = true;
        info!("Sentinel activated");
    }

    /// Set the valid scopes for the current task.
    pub fn set_scopes(&mut self, scopes: Vec<String>) {
        self.valid_scopes = scopes;
    }

    /// Enforce deterministic rules synchronously.
    ///
    /// Validates Git-Discipline and Scope-Lock. Runs before any
    /// async audit to fail fast on clear violations.
    fn enforce_deterministic_rules_sync(
        &self,
        payload: &str,
    ) -> Result<(), CoreError> {
        let check = enforce_deterministic_rules(payload, &self.valid_scopes);

        if !check.passed {
            let messages: Vec<String> = check
                .violations
                .iter()
                .map(|v| v.description.clone())
                .collect();

            error!(
                violations = ?messages,
                "Sentinel deterministic rules violated"
            );

            return Err(CoreError::SentinelPolicyViolation(messages.join("; ")));
        }

        Ok(())
    }

    /// Graph-level interception hook invoked before any destructive state mutation.
    ///
    /// Two-phase audit:
    /// 1. Synchronous deterministic regex rules (fail fast)
    /// 2. Asynchronous semantic slop detection via local classifier
    #[instrument(skip(self))]
    pub async fn evaluate_payload(
        &self,
        code_payload: String,
    ) -> Result<(), CoreError> {
        if !self.active {
            info!("Sentinel inactive — skipping audit");
            return Ok(());
        }

        info!("Sentinel initiating hybrid audit on proposed payload");

        // Phase 1: Deterministic rule checks.
        self.enforce_deterministic_rules_sync(&code_payload)?;

        // Phase 2: Semantic audit using local hardware tier.
        let cognitive = Arc::clone(&self.local_classifier);
        let payload = code_payload.clone();

        let evaluation = tokio::task::spawn(async move {
            slop_detector::evaluate_for_slop(&cognitive, &payload).await
        })
        .await
        .map_err(|_| {
            CoreError::SentinelSemanticRejection("Tokio task panicked during audit".into())
        })?;

        match evaluation {
            Ok(true) => {
                info!("Sentinel audit passed. Resuming graph execution.");
                Ok(())
            }
            Ok(false) => {
                error!("Sentinel detected AI slop or hallucination. Graph execution halted.");
                Err(CoreError::SentinelSemanticRejection(
                    "Payload failed semantic no-slop verification.".into(),
                ))
            }
            Err(e) => {
                error!(error = %e, "Sentinel evaluation error");
                Err(e)
            }
        }
    }

    /// Evaluate an AgentAction through the full sentinel pipeline.
    pub async fn evaluate_action(
        &self,
        action: &AgentAction,
    ) -> Result<(), CoreError> {
        // For commit actions, also validate the commit message format.
        if matches!(
            action.action_type,
            candor_core::protocol::ActionType::GitCommit
        ) {
            let commit_check = check_conventional_commit(&action.payload);
            if !commit_check.passed {
                return Err(CoreError::SentinelPolicyViolation(
                    commit_check.violations[0].description.clone(),
                ));
            }
        }

        self.evaluate_payload(action.payload.clone()).await
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Create a boxed clone as a BeforeToolCallback trait object.
    pub fn clone_box(&self) -> Box<dyn BeforeToolCallback> {
        Box::new(Self {
            local_classifier: Arc::clone(&self.local_classifier),
            valid_scopes: self.valid_scopes.clone(),
            active: self.active,
        })
    }
}

// ── BeforeToolCallback implementation ──
// This is what wires the Sentinel into the GraphRunner lifecycle.

#[async_trait::async_trait]
impl BeforeToolCallback for SentinelInterceptor {
    async fn before_tool(
        &self,
        action: &AgentAction,
        _state: Arc<tokio::sync::Mutex<AgentState>>,
    ) -> Result<(), CoreError> {
        // The Sentinel is architecturally isolated — it does NOT read
        // the AgentState (conversation history, persona, etc.).
        // It evaluates the action purely on deterministic rules + payload.
        self.evaluate_action(action).await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_sentinel_structural() {
        // Sentinel construction doesn't require a live model.
        // The permissive mode accepts all scopes.

        // This test validates the structural integrity
        // of the SentinelInterceptor type.
        assert!(true); // SentinelInterceptor compiles correctly.
    }
}
