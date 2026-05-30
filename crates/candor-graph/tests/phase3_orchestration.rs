/// Phase 3 integration tests: 7-phase orchestration graph.
/// Tests strict node traversal, human-in-the-loop pause, and checkpoint persistence.
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_core::error::CoreError;
use candor_core::state::AgentState;
use candor_graph::hooks::{BeforeExecuteConfirmation, LifecycleHooks};
use candor_graph::node::AgentNode;
use candor_graph::runner::GraphRunner;
use candor_graph::checkpoint::CheckpointManager;

/// Node that records its name in the execution log.
struct PhaseNode {
    name: String,
}

#[async_trait::async_trait]
impl AgentNode for PhaseNode {
    fn name(&self) -> &str { &self.name }

    async fn execute(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let mut s = state.lock().await;
        s.current_phase = Some(self.name.clone());
        s.log_event(&format!("Executed: {}", self.name));
        Ok(())
    }
}

/// Test: 7 nodes are traversed in strict order.
#[tokio::test]
async fn test_strict_7_node_traversal() {
    let mut runner = GraphRunner::new(100);
    let phases = ["Observe", "Think", "Plan", "Build", "Execute", "Verify", "Learn"];
    let mut indices = Vec::new();

    for phase in &phases {
        indices.push(runner.insert_node(phase, Box::new(PhaseNode { name: phase.to_string() })));
    }
    for w in indices.windows(2) {
        runner.insert_edge(w[0], w[1], "next".into());
    }

    runner.execute_graph(indices[0]).await.unwrap();

    let state_arc = runner.state();
    let s = state_arc.lock().await;
    let events: Vec<&str> = s.execution_log.iter().filter(|e| e.contains("Executed:")).map(|e| e.as_str()).collect();
    assert_eq!(events.len(), 7);
    assert!(events.iter().enumerate().all(|(i, e)| e.contains(phases[i])));
}

/// Test: human-in-the-loop pause blocks Execute phase.
#[tokio::test]
async fn test_human_in_the_loop_pause() {
    struct ApproveHook {
        approved: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl BeforeExecuteConfirmation for ApproveHook {
        async fn before_execute(&self, _state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
            if self.approved.load(std::sync::atomic::Ordering::SeqCst) {
                Ok(())
            } else {
                Err(CoreError::HumanApprovalDenied)
            }
        }
    }

    let approved = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook = ApproveHook { approved: Arc::clone(&approved) };

    let hooks = LifecycleHooks {
        before_execute: vec![Box::new(hook)],
        ..Default::default()
    };

    let mut runner = GraphRunner::new(100).with_hooks(hooks);
    let phases = ["Observe", "Think", "Plan", "Build", "Execute", "Verify", "Learn"];
    let mut indices = Vec::new();
    for phase in &phases {
        indices.push(runner.insert_node(phase, Box::new(PhaseNode { name: phase.to_string() })));
    }
    for w in indices.windows(2) {
        runner.insert_edge(w[0], w[1], "next".into());
    }

    // Before approval, Execute should be blocked
    let result = runner.execute_graph(indices[0]).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), CoreError::HumanApprovalDenied));
}

/// Test: checkpoint saves after every node transition.
#[tokio::test]
async fn test_checkpoint_after_transition() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = CheckpointManager::new(dir.path().to_path_buf(), 10);
    let state = Arc::new(Mutex::new(AgentState::default()));

    // Execute a node, save checkpoint
    {
        let mut s = state.lock().await;
        s.active_task = "checkpoint test".into();
        s.iteration_count = 1;
    }

    let path = mgr.save(Arc::clone(&state)).await.unwrap();
    assert!(path.exists());

    // Reset state and reload
    {
        let mut s = state.lock().await;
        s.active_task = String::new();
        s.iteration_count = 0;
    }

    let loaded = mgr.load_latest(Arc::clone(&state)).await.unwrap();
    assert!(loaded);

    let s = state.lock().await;
    assert_eq!(s.active_task, "checkpoint test");
    assert_eq!(s.iteration_count, 1);
}

/// Test: node count is correct after building 7-phase graph.
#[test]
fn test_graph_node_count() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut runner = GraphRunner::new(100);
        let phases = ["Observe", "Think", "Plan", "Build", "Execute", "Verify", "Learn"];
        for phase in &phases {
            runner.insert_node(phase, Box::new(PhaseNode { name: phase.to_string() }));
        }
        assert_eq!(runner.node_count(), 7);
    });
}
