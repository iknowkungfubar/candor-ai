/// The execution engine managing graph traversal and state checkpoints.
///
/// This is the heart of the orchestration layer. It persists state after
/// every node transition, providing durable resumes in the event of a crash.
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;

use petgraph::graph::{DiGraph, NodeIndex};
use tracing::{error, info, instrument};

use candor_core::error::CoreError;
use candor_core::protocol::{ActionType, AgentAction};
use candor_core::state::AgentState;

use super::hooks::LifecycleHooks;
use super::node::AgentNode;

/// The execution engine managing the graph traversal and state checkpoints.
pub struct GraphRunner {
    graph: DiGraph<Box<dyn AgentNode>, String>,
    state: Arc<Mutex<AgentState>>,
    max_iterations: u32,
    /// Node index -> human-readable label
    node_labels: HashMap<NodeIndex, String>,
    /// Ordered log of node transitions for checkpointing.
    transition_log: VecDeque<TransitionRecord>,
    /// Lifecycle hooks registered on this runner.
    hooks: LifecycleHooks,
}

#[derive(Debug, Clone)]
pub struct TransitionRecord {
    pub from: Option<NodeIndex>,
    pub to: NodeIndex,
    pub edge_condition: Option<String>,
    pub timestamp: String,
}

impl GraphRunner {
    pub fn new(max_iterations: u32) -> Self {
        Self {
            graph: DiGraph::new(),
            state: Arc::new(Mutex::new(AgentState::default())),
            max_iterations,
            node_labels: HashMap::new(),
            transition_log: VecDeque::with_capacity(1024),
            hooks: LifecycleHooks::default(),
        }
    }

    /// Register lifecycle hooks on this runner.
    pub fn with_hooks(mut self, hooks: LifecycleHooks) -> Self {
        self.hooks = hooks;
        self
    }

    /// Add a node to the graph and return its index.
    pub fn insert_node(&mut self, label: &str, node: Box<dyn AgentNode>) -> NodeIndex {
        let idx = self.graph.add_node(node);
        self.node_labels.insert(idx, label.to_string());
        idx
    }

    /// Add a directed edge from one node to another with an optional condition label.
    pub fn insert_edge(&mut self, from: NodeIndex, to: NodeIndex, condition: String) {
        self.graph.add_edge(from, to, condition);
    }

    /// Return a shared reference to the agent state Arc.
    pub fn state(&self) -> Arc<Mutex<AgentState>> {
        Arc::clone(&self.state)
    }

    /// Return the current maximum iteration count.
    pub fn max_iterations(&self) -> u32 {
        self.max_iterations
    }

    /// Set a new maximum iteration count.
    pub fn set_max_iterations(&mut self, limit: u32) {
        self.max_iterations = limit;
    }

    /// Build a synthetic AgentAction for hook callbacks from node metadata.
    fn build_action_for_node(&self, node_label: &str, current_idx: NodeIndex) -> AgentAction {
        let phase = {
            // Extract phase name — scope lock to avoid deadlocks
            let state_arc = self.state();
            let s = state_arc.try_lock().ok();
            s.and_then(|s| s.current_phase.clone())
                .unwrap_or_else(|| node_label.to_string())
        };

        AgentAction {
            id: format!("node-{:?}", current_idx.index()),
            action_type: ActionType::SandboxExecution,
            payload: format!("Executing node: {}", node_label),
            target_path: None,
            is_reversible: true,
            scope_tags: vec![node_label.to_string()],
            phase,
            sentinel_approved: false,
        }
    }

    /// Execute the graph starting from a given node.
    ///
    /// Traverses nodes along the first available outgoing edge from each,
    /// with iteration safety, hook callbacks, and full telemetry.
    #[instrument(skip(self))]
    pub async fn execute_graph(&mut self, start_node: NodeIndex) -> Result<(), CoreError> {
        let mut current_node = start_node;

        loop {
            // ── Safety gate: enforce max iterations ──
            {
                let mut state_lock = self.state.lock().await;
                if state_lock.iteration_count >= self.max_iterations {
                    error!(iteration_count = %state_lock.iteration_count, "Iteration limit hit. Triggering safety halt.");
                    return Err(CoreError::MaxIterationsReached);
                }
                state_lock.iteration_count += 1;
            }

            let node_label = self
                .node_labels
                .get(&current_node)
                .cloned()
                .unwrap_or_else(|| "unknown".into());
            info!(node = %node_label, "Executing node");

            // ── Fire before_tool hooks ──
            let action = self.build_action_for_node(&node_label, current_node);
            for hook in &self.hooks.before_tool {
                if let Err(e) = hook.before_tool(&action, Arc::clone(&self.state)).await {
                    error!(node = %node_label, error = %e, "BeforeToolCallback rejected action");
                    for err_hook in &self.hooks.on_error {
                        err_hook
                            .on_error(&e, &node_label, Arc::clone(&self.state))
                            .await;
                    }
                    return Err(e);
                }
            }

            // ── Human-in-the-loop: require approval before Execute phase ──
            if node_label == "Execute" {
                for hook in &self.hooks.before_execute {
                    if let Err(e) = hook.before_execute(Arc::clone(&self.state)).await {
                        error!(node = %node_label, error = %e, "Human approval required for Execute phase");
                        return Err(e);
                    }
                }
            }

            // ── Execute the node ──
            let node = &self.graph[current_node];
            match node.execute(Arc::clone(&self.state)).await {
                Ok(()) => info!(node = %node_label, "Node executed successfully"),
                Err(e) => {
                    error!(node = %node_label, error = %e, "Execution failed");
                    // Fire on_error hooks
                    for err_hook in &self.hooks.on_error {
                        err_hook
                            .on_error(&e, &node_label, Arc::clone(&self.state))
                            .await;
                    }
                    return Err(e);
                }
            }

            // ── Fire after_tool hooks ──
            for hook in &self.hooks.after_tool {
                let _ = hook
                    .after_tool(&action, "ok", Arc::clone(&self.state))
                    .await;
            }

            // ── Fire checkpoint hooks every 5 iterations ──
            {
                let state_arc = self.state();
                let s = state_arc.lock().await;
                if s.iteration_count % 5 == 0 {
                    for hook in &self.hooks.checkpoint {
                        let _ = hook.on_checkpoint(Arc::clone(&self.state)).await;
                    }
                }
            }

            // ── Traverse to the next node ──
            let mut neighbors = self
                .graph
                .neighbors_directed(current_node, petgraph::Direction::Outgoing);

            match neighbors.next() {
                Some(next_node) => {
                    let next_label = self
                        .node_labels
                        .get(&next_node)
                        .cloned()
                        .unwrap_or_else(|| "unknown".into());

                    let edge = self
                        .graph
                        .find_edge(current_node, next_node)
                        .and_then(|e| self.graph.edge_weight(e).cloned());

                    // ── Fire before_transition hooks ──
                    for hook in &self.hooks.before_transition {
                        if let Err(e) = hook
                            .before_transition(&node_label, &next_label, Arc::clone(&self.state))
                            .await
                        {
                            error!(from = %node_label, to = %next_label, error = %e, "BeforeNodeTransition rejected");
                            return Err(e);
                        }
                    }

                    self.transition_log.push_back(TransitionRecord {
                        from: Some(current_node),
                        to: next_node,
                        edge_condition: edge.clone(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    });

                    // ── Fire after_transition hooks ──
                    for hook in &self.hooks.after_transition {
                        let _ = hook
                            .after_transition(&node_label, &next_label, Arc::clone(&self.state))
                            .await;
                    }

                    current_node = next_node;
                }
                None => {
                    info!("No further routing edges found. Graph execution complete.");
                    break;
                }
            }
        }

        // ── Fire on_complete hooks ──
        for hook in &self.hooks.on_complete {
            let _ = hook.on_complete(Arc::clone(&self.state)).await;
        }

        Ok(())
    }

    /// Return the transition log for observability.
    pub fn transition_log(&self) -> &VecDeque<TransitionRecord> {
        &self.transition_log
    }

    /// Return the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingNode {
        name: &'static str,
        counter: StdArc<AtomicU32>,
    }

    #[async_trait::async_trait]
    impl AgentNode for CountingNode {
        fn name(&self) -> &str {
            self.name
        }

        async fn execute(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            let mut s = state.lock().await;
            s.log_event(&format!("{} executed", self.name));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_linear_graph_execution() {
        let counter = StdArc::new(AtomicU32::new(0));

        let mut runner = GraphRunner::new(100);
        let n1 = runner.insert_node(
            "node1",
            Box::new(CountingNode {
                name: "node1",
                counter: StdArc::clone(&counter),
            }),
        );
        let n2 = runner.insert_node(
            "node2",
            Box::new(CountingNode {
                name: "node2",
                counter: StdArc::clone(&counter),
            }),
        );
        let n3 = runner.insert_node(
            "node3",
            Box::new(CountingNode {
                name: "node3",
                counter: StdArc::clone(&counter),
            }),
        );
        runner.insert_edge(n1, n2, "next".into());
        runner.insert_edge(n2, n3, "next".into());

        runner.execute_graph(n1).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_max_iterations_halted() {
        let counter = StdArc::new(AtomicU32::new(0));

        let mut runner = GraphRunner::new(2); // allow only 2 iterations
        let n1 = runner.insert_node(
            "loop-node",
            Box::new(CountingNode {
                name: "loop-node",
                counter: StdArc::clone(&counter),
            }),
        );
        let n2 = runner.insert_node(
            "loop-node2",
            Box::new(CountingNode {
                name: "loop-node2",
                counter: StdArc::clone(&counter),
            }),
        );
        runner.insert_edge(n1, n2, "next".into());
        runner.insert_edge(n2, n1, "back".into()); // creates a cycle

        let result = runner.execute_graph(n1).await;
        assert!(matches!(result, Err(CoreError::MaxIterationsReached)));
    }
}
