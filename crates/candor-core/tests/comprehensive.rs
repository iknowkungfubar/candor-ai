/// Comprehensive tests for candor-core — all types, all edge cases.
use std::collections::HashMap;

use candor_core::error::CoreError;
use candor_core::ideal::{
    AcceptanceCriterion, ArtifactType, ConstraintEnforcement, IdealStateArtifact, VerificationMethod,
};
use candor_core::protocol::{ActionType, AgentAction};
use candor_core::state::AgentState;

// ── CoreError ──

#[test]
fn test_all_error_variants_display() {
    let errors = [
        CoreError::GraphExecution("test".into()),
        CoreError::SandboxTrap("trap".into()),
        CoreError::SandboxResourceExhausted,
        CoreError::Inference("fail".into()),
        CoreError::MemorySystem("mem".into()),
        CoreError::SentinelPolicyViolation("blocked".into()),
        CoreError::SentinelSemanticRejection("slop".into()),
        CoreError::IdealStateNotSatisfied("nope".into()),
        CoreError::MaxIterationsReached,
        CoreError::HumanApprovalDenied,
        CoreError::StateCorruption("bad".into()),
        CoreError::Config("wrong".into()),
        CoreError::Io("disk".into()),
        CoreError::Serialization("json".into()),
        CoreError::Internal("bug".into()),
    ];
    for err in &errors {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn test_error_clone() {
    let err = CoreError::GraphExecution("test".into());
    assert_eq!(err.to_string(), err.clone().to_string());
}

#[test]
fn test_error_from_io() {
    let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let core: CoreError = io.into();
    assert!(core.to_string().contains("IO error"));
}

#[test]
fn test_error_from_serde() {
    let json = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
    let core: CoreError = json.into();
    assert!(core.to_string().contains("Serialization"));
}

#[test]
fn test_error_debug() {
    let err = CoreError::MaxIterationsReached;
    assert!(format!("{:?}", err).contains("MaxIterationsReached"));
}

// ── AgentState ──

#[test]
fn test_state_default() {
    let s = AgentState::default();
    assert!(s.message_history.is_empty());
    assert!(s.active_task.is_empty());
    assert_eq!(s.iteration_count, 0);
    assert!(!s.sentinel_approved);
    assert!(!s.awaiting_approval);
    assert!(s.project_id.is_none());
    assert!(s.execution_log.is_empty());
    assert_eq!(s.estimated_token_count, 0);
    assert!(!s.compaction_required);
}

#[test]
fn test_state_append_message() {
    let mut s = AgentState::default();
    s.append_message("hello");
    assert_eq!(s.message_history.len(), 1);
    assert!(s.estimated_token_count > 0);
}

#[test]
fn test_state_append_multiple() {
    let mut s = AgentState::default();
    for i in 0..100 {
        s.append_message(&format!("msg {i}"));
    }
    assert_eq!(s.message_history.len(), 100);
}

#[test]
fn test_state_token_limit() {
    let mut s = AgentState::default();
    s.estimated_token_count = 135_000;
    assert!(s.is_over_token_limit());
    s.estimated_token_count = 134_999;
    assert!(!s.is_over_token_limit());
    s.estimated_token_count = 0;
    assert!(!s.is_over_token_limit());
}

#[test]
fn test_state_compact_context() {
    let mut s = AgentState::default();
    for i in 0..100 {
        s.append_message(&format!("message number {i} with some content"));
    }
    s.compact_context(200);
    assert!(s.message_history.len() < 100);
    assert!(!s.compaction_required);
}

#[test]
fn test_state_compact_empty() {
    let mut s = AgentState::default();
    s.compact_context(100);
    assert!(s.message_history.is_empty());
}

#[test]
fn test_state_compact_keeps_recent() {
    let mut s = AgentState::default();
    s.append_message("first");
    s.append_message("second");
    s.append_message("third");
    s.compact_context(100);
    // Should keep at least the last message
    assert!(!s.message_history.is_empty());
}

#[test]
fn test_state_log_event() {
    let mut s = AgentState::default();
    s.log_event("test");
    assert_eq!(s.execution_log.len(), 1);
    assert!(s.execution_log[0].contains("test"));
}

#[test]
fn test_state_log_multiple_events() {
    let mut s = AgentState::default();
    for i in 0..50 {
        s.log_event(&format!("event {i}"));
    }
    assert_eq!(s.execution_log.len(), 50);
}

#[test]
fn test_state_serialization_roundtrip() {
    let mut s = AgentState {
        active_task: "test task".into(),
        iteration_count: 42,
        project_id: Some("proj-1".into()),
        ..Default::default()
    };
    s.log_event("hello");

    let json = serde_json::to_string(&s).unwrap();
    let deserialized: AgentState = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.active_task, "test task");
    assert_eq!(deserialized.iteration_count, 42);
}

// ── IdealStateArtifact ──

#[test]
fn test_isa_all_criteria_satisfied() {
    let isa = make_isa();
    let mut results = HashMap::new();
    results.insert("c1".into(), true);
    results.insert("c2".into(), true);
    assert!(isa.is_satisfied(&results));
}

#[test]
fn test_isa_some_unmet() {
    let isa = make_isa();
    let mut results = HashMap::new();
    results.insert("c1".into(), true);
    results.insert("c2".into(), false);
    assert!(!isa.is_satisfied(&results));
}

#[test]
fn test_isa_unmet_criteria_returns_correct() {
    let isa = make_isa();
    let mut results = HashMap::new();
    results.insert("c1".into(), true);
    results.insert("c2".into(), false);
    let unmet = isa.unmet_criteria(&results);
    assert_eq!(unmet.len(), 1);
    assert_eq!(unmet[0].id, "c2");
}

#[test]
fn test_isa_missing_results_treated_as_false() {
    let isa = make_isa();
    let results = HashMap::new();
    assert!(!isa.is_satisfied(&results));
    assert_eq!(isa.unmet_criteria(&results).len(), 2);
}

#[test]
fn test_verification_method_serde() {
    let methods = [
        VerificationMethod::ShellCommand { command: "ls".into() },
        VerificationMethod::TestCase {
            test_name: "my_test".into(),
        },
        VerificationMethod::FileExists {
            path: "README.md".into(),
        },
        VerificationMethod::FileMatches {
            path: "Cargo.toml".into(),
            pattern: "name".into(),
        },
        VerificationMethod::LintCheck {
            command: "cargo fmt --check".into(),
        },
        VerificationMethod::HumanConfirmation { prompt: "ok?".into() },
    ];
    for m in &methods {
        let json = serde_json::to_string(m).unwrap();
        assert!(!json.is_empty());
    }
}

#[test]
fn test_artifact_type_variants() {
    let types = [
        ArtifactType::SourceFile,
        ArtifactType::TestFile,
        ArtifactType::MarkdownDocument,
        ArtifactType::Commit,
        ArtifactType::BinaryOutput,
        ArtifactType::Other { kind: "config".into() },
    ];
    for t in &types {
        let json = serde_json::to_string(t).unwrap();
        assert!(!json.is_empty());
    }
}

#[test]
fn test_constraint_enforcement() {
    let enforcements = [
        ConstraintEnforcement::PreExecution,
        ConstraintEnforcement::PostExecution,
        ConstraintEnforcement::PreCommit,
    ];
    for e in &enforcements {
        let json = serde_json::to_string(e).unwrap();
        assert!(!json.is_empty());
    }
}

fn make_isa() -> IdealStateArtifact {
    IdealStateArtifact {
        id: "test".into(),
        goal: "test goal".into(),
        acceptance_criteria: vec![
            AcceptanceCriterion {
                id: "c1".into(),
                description: "works".into(),
                verification_method: VerificationMethod::ShellCommand { command: "true".into() },
            },
            AcceptanceCriterion {
                id: "c2".into(),
                description: "passes tests".into(),
                verification_method: VerificationMethod::TestCase {
                    test_name: "all".into(),
                },
            },
        ],
        constraints: vec![],
        expected_artifacts: vec![],
        phase_requirements: Default::default(),
        fully_autonomous: true,
    }
}

// ── AgentAction ──

#[test]
fn test_is_destructive_force_push() {
    let a = AgentAction {
        id: "1".into(),
        action_type: ActionType::ForcePush,
        payload: "push".into(),
        target_path: None,
        is_reversible: false,
        scope_tags: vec![],
        phase: "execute".into(),
        sentinel_approved: false,
    };
    assert!(a.is_destructive());
}

#[test]
fn test_is_destructive_file_delete() {
    let a = AgentAction {
        id: "2".into(),
        action_type: ActionType::FileDelete,
        payload: "rm".into(),
        target_path: Some("x".into()),
        is_reversible: false,
        scope_tags: vec![],
        phase: "execute".into(),
        sentinel_approved: false,
    };
    assert!(a.is_destructive());
}

#[test]
fn test_is_destructive_shell() {
    let a = AgentAction {
        id: "3".into(),
        action_type: ActionType::ShellCommand,
        payload: "rm -rf /".into(),
        target_path: None,
        is_reversible: false,
        scope_tags: vec![],
        phase: "execute".into(),
        sentinel_approved: false,
    };
    assert!(a.is_destructive());
}

#[test]
fn test_is_destructive_db_write() {
    let a = AgentAction {
        id: "4".into(),
        action_type: ActionType::DatabaseWrite,
        payload: "INSERT".into(),
        target_path: None,
        is_reversible: false,
        scope_tags: vec![],
        phase: "execute".into(),
        sentinel_approved: false,
    };
    assert!(a.is_destructive());
}

#[test]
fn test_is_not_destructive_file_write() {
    let a = AgentAction {
        id: "5".into(),
        action_type: ActionType::FileWrite,
        payload: "write".into(),
        target_path: Some("x".into()),
        is_reversible: true,
        scope_tags: vec![],
        phase: "build".into(),
        sentinel_approved: false,
    };
    assert!(!a.is_destructive());
}

#[test]
fn test_is_not_destructive_generate_code() {
    let a = AgentAction {
        id: "6".into(),
        action_type: ActionType::GenerateCode,
        payload: "fn main() {}".into(),
        target_path: None,
        is_reversible: true,
        scope_tags: vec![],
        phase: "build".into(),
        sentinel_approved: false,
    };
    assert!(!a.is_destructive());
}

#[test]
fn test_action_type_serde_all() {
    let types = [
        ActionType::GenerateCode,
        ActionType::ShellCommand,
        ActionType::FileWrite,
        ActionType::FileDelete,
        ActionType::GitCommit,
        ActionType::GitPush,
        ActionType::ForcePush,
        ActionType::HttpRequest,
        ActionType::DatabaseRead,
        ActionType::DatabaseWrite,
        ActionType::SandboxExecution,
        ActionType::ApprovalRequest,
        ActionType::MemoryStore,
        ActionType::MemoryRetrieve,
        ActionType::SentinelAudit,
    ];
    for t in &types {
        let json = serde_json::to_string(t).unwrap();
        assert!(!json.is_empty());
    }
}
