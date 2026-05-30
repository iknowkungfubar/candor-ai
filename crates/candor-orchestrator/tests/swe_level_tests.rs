/// Professional SWE-level tests: property-based, edge cases, integration.
/// Tests the full agent pipeline with mock LLM end-to-end.
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_cognitive::CognitiveEngine;
use candor_core::error::CoreError;
use candor_core::ideal::{IdealStateArtifact, AcceptanceCriterion, VerificationMethod};
use candor_core::state::AgentState;
use candor_core::protocol::{ActionType, AgentAction};
use candor_graph::runner::GraphRunner;
use candor_graph::node::AgentNode;
use candor_graph::hooks::LifecycleHooks;
use candor_memory::store::MemorySystem;
use candor_orchestrator::OrchestratorEngine;
use candor_tools::registry::{Tool, ToolContext, ToolOutput, ToolRegistry};

// ── Property-based: AgentState invariants ──

#[test]
fn property_state_token_estimate_monotonic() {
    let mut state = AgentState::default();
    let mut last = 0;
    for i in 0..100 {
        state.append_message(&format!("msg {i} with padding to make it realistic"));
        assert!(state.estimated_token_count >= last, "token count must be monotonic");
        last = state.estimated_token_count;
    }
}

#[test]
fn property_state_compaction_reduces_size() {
    let mut state = AgentState::default();
    for i in 0..200 {
        state.append_message(&format!("this is message number {i} with substantial content for testing"));
    }
    let before = state.message_history.len();
    assert!(before >= 199, "should have at least 199 messages");

    state.compact_context(100);
    assert!(state.message_history.len() < before, "compaction must reduce message count");
}

#[test]
fn property_state_compaction_idempotent() {
    let mut state = AgentState::default();
    for _i in 0..50 {
        state.append_message("test message content for idempotency check");
    }
    state.compact_context(200);
    let after_first = state.message_history.len();
    state.compact_context(200);
    assert_eq!(state.message_history.len(), after_first, "double compaction should be idempotent");
}

#[test]
fn property_error_to_string_not_empty() {
    // Every error variant should produce non-empty display
    let errors = vec![
        CoreError::GraphExecution("x".into()),
        CoreError::SandboxTrap("x".into()),
        CoreError::SandboxResourceExhausted,
        CoreError::Inference("x".into()),
        CoreError::MemorySystem("x".into()),
        CoreError::SentinelPolicyViolation("x".into()),
        CoreError::SentinelSemanticRejection("x".into()),
        CoreError::IdealStateNotSatisfied("x".into()),
        CoreError::MaxIterationsReached,
        CoreError::HumanApprovalDenied,
        CoreError::StateCorruption("x".into()),
        CoreError::Config("x".into()),
        CoreError::Io("x".into()),
        CoreError::Serialization("x".into()),
        CoreError::Internal("x".into()),
    ];
    for err in &errors {
        assert!(!err.to_string().is_empty());
    }
}

// ── Edge case: empty state ──

#[tokio::test]
async fn edge_empty_state_compaction_safe() {
    let state = Arc::new(Mutex::new(AgentState::default()));
    {
        let mut s = state.lock().await;
        s.compact_context(100);
    }
    let s = state.lock().await;
    assert!(s.message_history.is_empty());
}

#[tokio::test]
async fn edge_max_iterations_zero() {
    let mut runner = GraphRunner::new(0);
    let node = Box::new(TestNode { name: "test".into(), should_fail: false });
    let idx = runner.insert_node("test", node);
    let result = runner.execute_graph(idx).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), CoreError::MaxIterationsReached));
}

// ── Edge case: single node graph ──

#[tokio::test]
async fn edge_single_node_graph() {
    let mut runner = GraphRunner::new(100);
    let node = Box::new(TestNode { name: "only".into(), should_fail: false });
    let idx = runner.insert_node("only", node);
    let result = runner.execute_graph(idx).await;
    assert!(result.is_ok());

    let state_arc = runner.state();
    let s = state_arc.lock().await;
    assert_eq!(s.execution_log.len(), 1);
}

// ── Edge case: node failure propagation ──

#[tokio::test]
async fn edge_node_failure_stops_graph() {
    let mut runner = GraphRunner::new(100);
    let n1 = runner.insert_node("good", Box::new(TestNode { name: "good".into(), should_fail: false }));
    let n2 = runner.insert_node("bad", Box::new(TestNode { name: "bad".into(), should_fail: true }));
    runner.insert_edge(n1, n2, "next".into());

    let result = runner.execute_graph(n1).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("intentional"));
}

// ── Edge case: empty tool registry ──

#[test]
fn edge_empty_tool_registry() {
    let registry = ToolRegistry::new();
    assert_eq!(registry.tool_count(), 0);
    assert!(registry.find("anything").is_none());
    assert!(registry.descriptions_for_llm().is_empty());
}

#[test]
fn edge_tool_registry_duplicate_registration() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(DummyTool { name: "dup".into() }));
    registry.register(Arc::new(DummyTool { name: "dup".into() }));
    // Should allow duplicates (last wins on find)
    assert_eq!(registry.tool_count(), 2);
}

#[test]
fn edge_destructive_action_all_types() {
    // All destructive types should return true
    let destructive = [
        ActionType::ForcePush,
        ActionType::FileDelete,
        ActionType::ShellCommand,
        ActionType::DatabaseWrite,
    ];
    let non_destructive = [
        ActionType::GenerateCode,
        ActionType::FileWrite,
        ActionType::GitCommit,
        ActionType::DatabaseRead,
        ActionType::SandboxExecution,
        ActionType::MemoryStore,
        ActionType::MemoryRetrieve,
    ];

    for action_type in &destructive {
        let a = AgentAction {
            id: "t".into(), action_type: action_type.clone(),
            payload: "".into(), target_path: None, is_reversible: false,
            scope_tags: vec![], phase: "".into(), sentinel_approved: false,
        };
        assert!(a.is_destructive(), "{:?} should be destructive", action_type);
    }
    for action_type in &non_destructive {
        let a = AgentAction {
            id: "t".into(), action_type: action_type.clone(),
            payload: "".into(), target_path: None, is_reversible: true,
            scope_tags: vec![], phase: "".into(), sentinel_approved: false,
        };
        assert!(!a.is_destructive(), "{:?} should NOT be destructive", action_type);
    }
}

// ── Integration: full agent pipeline with mock LLM ──

#[tokio::test]
async fn integration_full_agent_pipeline_mock() {
    // SAFETY: Single-threaded test — prevent RunTestsTool from recursively
    // calling `cargo test` and deadlocking the test suite.
    unsafe { std::env::set_var("CANDOR_SKIP_TEST_EXECUTION", "1"); }

    let cognitive = Arc::new(CognitiveEngine::new(None, None).await.unwrap());
    let memory = Arc::new(MemorySystem::new(384).await.unwrap());
    let mut agent = OrchestratorEngine::new(cognitive, memory, 100).await.unwrap();

    // Deactivate sentinel for mock testing
    agent.sentinel.deactivate();
    let hooks = LifecycleHooks::default().with_before_tool(agent.sentinel.clone_box());
    agent.graph_runner = GraphRunner::new(100).with_hooks(hooks);

    let isa = IdealStateArtifact {
        id: "integration-test".into(),
        goal: "scan project and list files".into(),
        acceptance_criteria: vec![
            AcceptanceCriterion {
                id: "list-output".into(),
                description: "list_dir produces output".into(),
                verification_method: VerificationMethod::ShellCommand {
                    command: "ls".into(),
                },
            },
        ],
        constraints: vec![],
        expected_artifacts: vec![],
        phase_requirements: Default::default(),
        fully_autonomous: true,
    };

    let result = agent.run_task("integration: scan project and list files", &isa, None).await;
    assert!(result.is_ok(), "full agent pipeline should complete");

    // Verify all 7 phases ran
    let state_arc = agent.graph_runner.state();
    let s = state_arc.lock().await;
    assert!(s.execution_log.iter().any(|e| e.contains("Observe")));
    assert!(s.execution_log.iter().any(|e| e.contains("Think")));
    assert!(s.execution_log.iter().any(|e| e.contains("Plan")));
    assert!(s.execution_log.iter().any(|e| e.contains("Build")));
    assert!(s.execution_log.iter().any(|e| e.contains("Execute")));
    assert!(s.execution_log.iter().any(|e| e.contains("Verify")));
    assert!(s.execution_log.iter().any(|e| e.contains("Learn")));
}

#[tokio::test]
async fn integration_sentinel_blocks_destructive_in_pipeline() {
    let cognitive = Arc::new(CognitiveEngine::new(None, None).await.unwrap());
    let memory = Arc::new(MemorySystem::new(384).await.unwrap());
    let mut agent = OrchestratorEngine::new(cognitive, memory, 100).await.unwrap();

    // Sentinel active — should block force push payloads
    assert!(agent.sentinel.is_active());

    let hooks = LifecycleHooks::default().with_before_tool(agent.sentinel.clone_box());
    agent.graph_runner = GraphRunner::new(100).with_hooks(hooks);

    // Test that sentinel blocks force push in a node payload
    let result = agent.sentinel.evaluate_payload("git push --force origin main".into()).await;
    assert!(result.is_err());
}

// ── Helpers ──

struct TestNode {
    name: String,
    should_fail: bool,
}

#[async_trait::async_trait]
impl AgentNode for TestNode {
    fn name(&self) -> &str { &self.name }

    async fn execute(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        if self.should_fail {
            return Err(CoreError::Internal("intentional test failure".into()));
        }
        let mut s = state.lock().await;
        s.log_event(&format!("node: {}", self.name));
        s.iteration_count += 1;
        Ok(())
    }
}

struct DummyTool { name: String }

#[async_trait::async_trait]
impl Tool for DummyTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { "dummy" }
    async fn execute(&self, _ctx: &ToolContext, _args: &[String]) -> Result<ToolOutput, CoreError> {
        Ok(ToolOutput::ok("ok"))
    }
}
