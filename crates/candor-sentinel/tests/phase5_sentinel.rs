/// Phase 5 integration tests: sentinel + no-slop guardrails.
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_core::protocol::{ActionType, AgentAction};
use candor_core::state::AgentState;
use candor_sentinel::SentinelInterceptor;
use candor_sentinel::rules::{
    enforce_deterministic_rules, check_conventional_commit,
};
use candor_sentinel::doctrine::enforce_doctrine;

// ── Criterion 1: Sentinel is a BeforeToolCallback ──

#[tokio::test]
async fn test_sentinel_is_before_tool_callback() {
    let cog = Arc::new(candor_cognitive::CognitiveEngine::new(None, None).await.unwrap());
    let mut sentinel = SentinelInterceptor::new(cog, vec![]);
    sentinel.deactivate(); // semantic audit needs real LLM — testing trait impl only
    let action = AgentAction {
        id: "t".into(), action_type: ActionType::GenerateCode,
        payload: "fn main() {}".into(), target_path: None,
        is_reversible: true, scope_tags: vec![], phase: "build".into(),
        sentinel_approved: false,
    };
    let state = Arc::new(Mutex::new(AgentState::default()));
    // Call via the BeforeToolCallback trait
    let result = candor_graph::hooks::BeforeToolCallback::before_tool(
        &sentinel, &action, state,
    ).await;
    assert!(result.is_ok());
}

// ── Criterion 2: Sentinel halts on dangerous payloads ──

#[test]
fn test_blocks_force_push() {
    let check = enforce_deterministic_rules(
        "git push --force origin main", &["git".into()],
    );
    assert!(!check.passed);
    assert!(check.violations.iter().any(|v| v.rule.contains("force-push")));
}

#[test]
fn test_blocks_todo() {
    let check = enforce_deterministic_rules("// TODO: fix later", &["code".into()]);
    assert!(!check.passed);
}

#[test]
fn test_blocks_narration() {
    let check = enforce_deterministic_rules(
        "// now we create the function that handles user input",
        &["code".into()],
    );
    assert!(!check.passed);
}

#[test]
fn test_blocks_rm_rf() {
    let check = enforce_deterministic_rules("rm -rf /", &["shell".into()]);
    assert!(!check.passed);
}

#[test]
fn test_blocks_dead_code() {
    let check = enforce_deterministic_rules(
        "if false { unreachable!(\"never\"); }",
        &["code".into()],
    );
    assert!(!check.passed);
}

#[test]
fn test_passes_clean_code() {
    // Scope-lock checks that valid_scopes appear in the payload
    let check = enforce_deterministic_rules(
        "fn add_code(a: i32, b: i32) -> i32 { a + b }",
        &["add_code".into()],
    );
    assert!(check.passed);
}

#[test]
fn test_scope_lock_enforced() {
    let check = enforce_deterministic_rules(
        "delete database", &["read_file".into()],
    );
    assert!(!check.passed);
    assert!(check.violations.iter().any(|v| v.rule == "scope-lock"));
}

#[test]
fn test_scope_lock_passes_when_in_scope() {
    let check = enforce_deterministic_rules(
        "read_file test.txt", &["read_file".into()],
    );
    assert!(check.passed);
}

// ── Criterion 3: Conventional commit validation ──

#[test]
fn test_conventional_commit_valid() {
    assert!(check_conventional_commit("feat(sandbox): add WASM backend").passed);
    assert!(check_conventional_commit("fix: resolve race condition").passed);
    assert!(check_conventional_commit("docs: update README").passed);
    assert!(check_conventional_commit("chore: bump version").passed);
    assert!(check_conventional_commit("refactor(api)!: breaking change").passed);
}

#[test]
fn test_conventional_commit_invalid() {
    assert!(!check_conventional_commit("fixed the bug").passed);
    assert!(!check_conventional_commit("WIP stuff").passed);
    assert!(!check_conventional_commit("update").passed);
}

// ── Criterion 4: Test-then-ship — commit rejection on test failure ──

#[test]
fn test_doctrine_prevents_destructive() {
    let action = AgentAction {
        id: "d".into(), action_type: ActionType::ForcePush,
        payload: "push -f".into(), target_path: None, is_reversible: false,
        scope_tags: vec![], phase: "execute".into(), sentinel_approved: false,
    };
    let check = enforce_doctrine(&action, "force push to main");
    assert!(!check.passed);
}

#[test]
fn test_doctrine_vague_claim_rejected() {
    let action = AgentAction {
        id: "v".into(), action_type: ActionType::GenerateCode,
        payload: "this should work probably".into(), target_path: None,
        is_reversible: true, scope_tags: vec![], phase: "build".into(),
        sentinel_approved: false,
    };
    let check = enforce_doctrine(&action, "this should work fine");
    assert!(!check.passed);
}

#[test]
fn test_doctrine_marketing_rejected() {
    let action = AgentAction {
        id: "m".into(), action_type: ActionType::GenerateCode,
        payload: "revolutionary new feature".into(), target_path: None,
        is_reversible: true, scope_tags: vec![], phase: "build".into(),
        sentinel_approved: false,
    };
    let check = enforce_doctrine(&action, "this revolutionary approach");
    assert!(!check.passed);
}

#[test]
fn test_doctrine_sustainability_limit() {
    let action = AgentAction {
        id: "s".into(), action_type: ActionType::GenerateCode,
        payload: "x".repeat(100_001),
        target_path: None, is_reversible: true,
        scope_tags: vec![], phase: "build".into(), sentinel_approved: false,
    };
    let check = enforce_doctrine(&action, "normal context");
    assert!(!check.passed);
}
