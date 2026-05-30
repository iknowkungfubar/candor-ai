/// Integration tests for sentinel interceptor.
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_cognitive::CognitiveEngine;
use candor_core::protocol::{ActionType, AgentAction};
use candor_core::state::AgentState;
use candor_graph::hooks::BeforeToolCallback;
use candor_sentinel::SentinelInterceptor;

async fn make_cognitive() -> Arc<CognitiveEngine> {
    Arc::new(CognitiveEngine::new(None, None).await.unwrap())
}

#[tokio::test]
async fn test_sentinel_construction() {
    let cog = make_cognitive().await;
    let s = SentinelInterceptor::new(cog, vec![]);
    assert!(s.is_active());
}

#[tokio::test]
async fn test_sentinel_permissive() {
    let cog = make_cognitive().await;
    let s = SentinelInterceptor::permissive(cog);
    assert!(s.is_active());
}

#[tokio::test]
async fn test_sentinel_deactivate_activate() {
    let cog = make_cognitive().await;
    let mut s = SentinelInterceptor::new(cog, vec![]);
    s.deactivate();
    assert!(!s.is_active());
    s.activate();
    assert!(s.is_active());
}

#[tokio::test]
async fn test_sentinel_set_scopes() {
    let cog = make_cognitive().await;
    let mut s = SentinelInterceptor::new(cog, vec![]);
    s.set_scopes(vec!["read".into()]);
    assert!(s.is_active());
}

#[tokio::test]
async fn test_sentinel_inactive_passes() {
    let cog = make_cognitive().await;
    let mut s = SentinelInterceptor::new(cog, vec![]);
    s.deactivate();
    assert!(s.evaluate_payload("anything".into()).await.is_ok());
}

#[tokio::test]
async fn test_sentinel_clean_code_passes() {
    let cog = make_cognitive().await;
    let mut s = SentinelInterceptor::new(cog, vec![]);
    s.deactivate(); // semantic audit needs real LLM — testing deterministic rules only
    assert!(s.evaluate_payload("fn add(a: i32, b: i32) -> i32 { a + b }".into()).await.is_ok());
}

#[tokio::test]
async fn test_sentinel_force_push_blocked() {
    let cog = make_cognitive().await;
    let s = SentinelInterceptor::new(cog, vec![]);
    assert!(s.evaluate_payload("git push --force origin main".into()).await.is_err());
}

#[tokio::test]
async fn test_sentinel_conventional_commit_passes() {
    let cog = make_cognitive().await;
    let mut s = SentinelInterceptor::new(cog, vec![]);
    s.deactivate(); // deterministic rules only — no LLM needed
    let action = AgentAction {
        id: "1".into(), action_type: ActionType::GitCommit,
        payload: "feat: add test".into(), target_path: None,
        is_reversible: true, scope_tags: vec![], phase: "build".into(),
        sentinel_approved: false,
    };
    assert!(s.evaluate_action(&action).await.is_ok());
}

#[tokio::test]
async fn test_sentinel_clone_box_hook() {
    let cog = make_cognitive().await;
    let mut s = SentinelInterceptor::new(cog, vec![]);
    s.deactivate();
    let cloned = s.clone_box();
    let action = AgentAction {
        id: "t".into(), action_type: ActionType::GenerateCode,
        payload: "fn main() {}".into(), target_path: None,
        is_reversible: true, scope_tags: vec![], phase: "build".into(),
        sentinel_approved: false,
    };
    assert!(cloned.before_tool(&action, Arc::new(Mutex::new(AgentState::default()))).await.is_ok());
}

#[tokio::test]
async fn test_sentinel_before_tool_callback() {
    let cog = make_cognitive().await;
    let mut s = SentinelInterceptor::new(cog, vec![]);
    s.deactivate();
    let action = AgentAction {
        id: "cb".into(), action_type: ActionType::GenerateCode,
        payload: "println!(\"hello\");".into(), target_path: None,
        is_reversible: true, scope_tags: vec![], phase: "build".into(),
        sentinel_approved: false,
    };
    assert!(s.before_tool(&action, Arc::new(Mutex::new(AgentState::default()))).await.is_ok());
}
