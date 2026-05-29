/// Integration tests for candor-graph — hooks wiring.
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_core::error::CoreError;
use candor_core::protocol::AgentAction;
use candor_core::state::AgentState;
use candor_graph::hooks::{
    AfterToolCallback, BeforeToolCallback, CompletionCallback, LifecycleHooks,
};
use candor_graph::node::AgentNode;
use candor_graph::runner::GraphRunner;

/// A node that just stamps its name in state.
struct StampNode {
    name: String,
}

#[async_trait::async_trait]
impl AgentNode for StampNode {
    fn name(&self) -> &str {
        &self.name
    }

    async fn execute(
        &self,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError> {
        let mut s = state.lock().await;
        s.log_event(&format!("stamped: {}", self.name));
        Ok(())
    }
}

/// A test hook that counts calls.
struct CountingHook {
    call_count: Arc<std::sync::atomic::AtomicU32>,
}

impl CountingHook {
    fn new() -> (Self, Arc<std::sync::atomic::AtomicU32>) {
        let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        (Self { call_count: Arc::clone(&count) }, count)
    }
}

#[async_trait::async_trait]
impl BeforeToolCallback for CountingHook {
    async fn before_tool(
        &self,
        _action: &AgentAction,
        _state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait::async_trait]
impl AfterToolCallback for CountingHook {
    async fn after_tool(
        &self,
        _action: &AgentAction,
        _result: &str,
        _state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait::async_trait]
impl CompletionCallback for CountingHook {
    async fn on_complete(
        &self,
        _state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn test_before_tool_hook_fires() {
    let (hook, count) = CountingHook::new();
    let (hook2, after_count) = CountingHook::new();

    let hooks = LifecycleHooks::default()
        .with_before_tool(Box::new(hook))
        .with_after_tool(Box::new(hook2));

    let mut runner = GraphRunner::new(100).with_hooks(hooks);
    let n1 = runner.insert_node("test-node", Box::new(StampNode { name: "a".into() }));
    let n2 = runner.insert_node("test-node-2", Box::new(StampNode { name: "b".into() }));
    runner.insert_edge(n1, n2, "next".into());

    runner.execute_graph(n1).await.unwrap();

    // before_tool should fire for each node execution (2 nodes)
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
    // after_tool should fire for each node execution (2 nodes)
    assert_eq!(after_count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_before_tool_hook_rejects_action() {
    struct RejectingHook;

    #[async_trait::async_trait]
    impl BeforeToolCallback for RejectingHook {
        async fn before_tool(
            &self,
            _action: &AgentAction,
            _state: Arc<Mutex<AgentState>>,
        ) -> Result<(), CoreError> {
            Err(CoreError::SentinelPolicyViolation("rejected".into()))
        }
    }

    let hooks = LifecycleHooks::default()
        .with_before_tool(Box::new(RejectingHook));

    let mut runner = GraphRunner::new(100).with_hooks(hooks);
    let n1 = runner.insert_node("test", Box::new(StampNode { name: "a".into() }));

    let result = runner.execute_graph(n1).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_node_labels_and_transition_log() {
    let mut runner = GraphRunner::new(100);
    let n1 = runner.insert_node("alpha", Box::new(StampNode { name: "a".into() }));
    let n2 = runner.insert_node("beta", Box::new(StampNode { name: "b".into() }));
    runner.insert_edge(n1, n2, "alpha->beta".into());

    runner.execute_graph(n1).await.unwrap();

    let log = runner.transition_log();
    assert_eq!(log.len(), 1);

    assert_eq!(runner.node_count(), 2);
}

#[tokio::test]
async fn test_completion_hook_fires() {
    let (hook, count) = CountingHook::new();
    let hooks = LifecycleHooks {
        on_complete: vec![Box::new(hook)],
        ..Default::default()
    };

    let mut runner = GraphRunner::new(100).with_hooks(hooks);
    let n1 = runner.insert_node("only", Box::new(StampNode { name: "a".into() }));

    runner.execute_graph(n1).await.unwrap();
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}
