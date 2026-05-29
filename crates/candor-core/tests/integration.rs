/// Rust integration tests for candor-core.
use candor_core::error::CoreError;
use candor_core::ideal::{
    AcceptanceCriterion, ArtifactType, Constraint, ConstraintEnforcement,
    ExpectedArtifact, IdealStateArtifact, VerificationMethod,
};
use candor_core::protocol::{ActionType, AgentAction};
use candor_core::state::AgentState;

// ── Error Tests ──

#[test]
fn test_core_error_display() {
    assert_eq!(
        CoreError::MaxIterationsReached.to_string(),
        "Maximum iteration limit reached"
    );
    assert_eq!(
        CoreError::HumanApprovalDenied.to_string(),
        "Human approval denied for tool execution"
    );
}

#[test]
fn test_core_error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let core_err: CoreError = io_err.into();
    assert!(core_err.to_string().contains("IO error"));
}

#[test]
fn test_core_error_from_serde_json() {
    let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
    let core_err: CoreError = json_err.into();
    assert!(core_err.to_string().contains("Serialization error"));
}

#[test]
fn test_core_error_clone() {
    let err = CoreError::MaxIterationsReached;
    let cloned = err.clone();
    assert_eq!(err.to_string(), cloned.to_string());
}

// ── AgentState Tests ──

#[test]
fn test_agent_state_default() {
    let state = AgentState::default();
    assert!(state.message_history.is_empty());
    assert_eq!(state.iteration_count, 0);
    assert!(!state.sentinel_approved);
}

#[test]
fn test_agent_state_append_message() {
    let mut state = AgentState::default();
    state.append_message("hello world");
    assert_eq!(state.message_history.len(), 1);
    assert!(state.estimated_token_count > 0);
}

#[test]
fn test_agent_state_token_limit() {
    let mut state = AgentState::default();
    // Each message is ~100 chars, needs ~540 to hit 135K tokens (with ~4 chars/token → 33,750 messages)
    // Use append_message which adds len/4 to token count
    state.estimated_token_count = 135_000;
    assert!(state.is_over_token_limit());

    state.estimated_token_count = 100_000;
    assert!(!state.is_over_token_limit());
}

#[test]
fn test_agent_state_compact_context() {
    let mut state = AgentState::default();
    state.append_message("First message");
    state.append_message("Second message");
    state.append_message("Third message");
    // Keep only ~10 chars of context
    state.compact_context(10);
    // Should have compacted to fewer messages
    assert!(state.message_history.len() < 3);
}

#[test]
fn test_agent_state_compact_empty() {
    let mut state = AgentState::default();
    state.compact_context(100);
    assert!(state.message_history.is_empty());
}

#[test]
fn test_agent_state_log_event() {
    let mut state = AgentState::default();
    state.log_event("test event");
    assert_eq!(state.execution_log.len(), 1);
    assert!(state.execution_log[0].contains("test event"));
}

// ── IdealStateArtifact Tests ──

#[test]
fn test_isa_unmet_criteria() {
    let isa = IdealStateArtifact {
        id: "test".into(),
        goal: "test goal".into(),
        acceptance_criteria: vec![
            AcceptanceCriterion {
                id: "c1".into(),
                description: "test criterion".into(),
                verification_method: VerificationMethod::ShellCommand {
                    command: "true".into(),
                },
            },
            AcceptanceCriterion {
                id: "c2".into(),
                description: "unmet criterion".into(),
                verification_method: VerificationMethod::HumanConfirmation {
                    prompt: "confirm".into(),
                },
            },
        ],
        constraints: vec![],
        expected_artifacts: vec![],
        phase_requirements: Default::default(),
        fully_autonomous: true,
    };

    let mut results = std::collections::HashMap::new();
    results.insert("c1".to_string(), true);
    results.insert("c2".to_string(), false);

    let unmet = isa.unmet_criteria(&results);
    assert_eq!(unmet.len(), 1);
    assert_eq!(unmet[0].id, "c2");

    assert!(!isa.is_satisfied(&results));

    results.insert("c2".to_string(), true);
    assert!(isa.is_satisfied(&results));
}

#[test]
fn test_constraint_enforcement() {
    let constraint = Constraint {
        id: "no-sudo".into(),
        description: "Must not require sudo".into(),
        enforcement: ConstraintEnforcement::PreExecution,
    };
    assert_eq!(constraint.id, "no-sudo");
}

#[test]
fn test_expected_artifact() {
    let artifact = ExpectedArtifact {
        path: "src/main.rs".into(),
        description: "main file".into(),
        artifact_type: ArtifactType::SourceFile,
    };
    assert_eq!(artifact.path, "src/main.rs");
}

// ── AgentAction Tests ──

#[test]
fn test_is_destructive_force_push() {
    let action = AgentAction {
        id: "1".into(),
        action_type: ActionType::ForcePush,
        payload: "git push --force".into(),
        target_path: None,
        is_reversible: false,
        scope_tags: vec![],
        phase: "execute".into(),
        sentinel_approved: false,
    };
    assert!(action.is_destructive());
}

#[test]
fn test_is_destructive_file_write() {
    let action = AgentAction {
        id: "2".into(),
        action_type: ActionType::FileWrite,
        payload: "write file".into(),
        target_path: Some("test.txt".into()),
        is_reversible: true,
        scope_tags: vec![],
        phase: "build".into(),
        sentinel_approved: false,
    };
    // FileWrite is NOT in the destructive match arm
    assert!(!action.is_destructive());
}

#[test]
fn test_is_destructive_file_delete() {
    let action = AgentAction {
        id: "3".into(),
        action_type: ActionType::FileDelete,
        payload: "rm file".into(),
        target_path: Some("test.txt".into()),
        is_reversible: false,
        scope_tags: vec![],
        phase: "execute".into(),
        sentinel_approved: false,
    };
    assert!(action.is_destructive());
}

#[test]
fn test_is_destructive_db_write() {
    let action = AgentAction {
        id: "4".into(),
        action_type: ActionType::DatabaseWrite,
        payload: "INSERT INTO...".into(),
        target_path: None,
        is_reversible: false,
        scope_tags: vec![],
        phase: "execute".into(),
        sentinel_approved: false,
    };
    assert!(action.is_destructive());
}

// ── VerificationMethod Tests ──

#[test]
fn test_verification_methods() {
    let shell = VerificationMethod::ShellCommand {
        command: "ls".into(),
    };
    let test = VerificationMethod::TestCase {
        test_name: "my_test".into(),
    };
    let file = VerificationMethod::FileExists {
        path: "README.md".into(),
    };
    let human = VerificationMethod::HumanConfirmation {
        prompt: "ok?".into(),
    };
    let lint = VerificationMethod::LintCheck {
        command: "cargo clippy".into(),
    };

    // Ensure all variants construct without error
    assert!(serde_json::to_string(&shell).is_ok());
    assert!(serde_json::to_string(&test).is_ok());
    assert!(serde_json::to_string(&file).is_ok());
    assert!(serde_json::to_string(&human).is_ok());
    assert!(serde_json::to_string(&lint).is_ok());
}
