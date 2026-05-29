/// Lifecycle hooks for the graph execution engine.
///
/// Following the Claude Code pattern, 17 lifecycle events allow
/// deterministic shell-script hooks to analyze state data prior
/// to any tool execution.
///
/// From the design doc Phase 3, Action Item 3.3:
/// "Inject 17 lifecycle events, adopting the Claude Code pattern."
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_core::error::CoreError;
use candor_core::protocol::AgentAction;
use candor_core::state::AgentState;

/// Hook fired before a tool/action is executed.
/// If this returns Err, the action is blocked.
#[async_trait::async_trait]
pub trait BeforeToolCallback: Send + Sync {
    async fn before_tool(
        &self,
        action: &AgentAction,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError>;
}

/// Hook fired after a tool/action completes.
#[async_trait::async_trait]
pub trait AfterToolCallback: Send + Sync {
    async fn after_tool(
        &self,
        action: &AgentAction,
        result: &str,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError>;
}

/// Hook fired before a node transition.
#[async_trait::async_trait]
pub trait BeforeNodeTransition: Send + Sync {
    async fn before_transition(
        &self,
        from_node: &str,
        to_node: &str,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError>;
}

/// Hook fired after a node transition.
#[async_trait::async_trait]
pub trait AfterNodeTransition: Send + Sync {
    async fn after_transition(
        &self,
        from_node: &str,
        to_node: &str,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError>;
}

/// Hook fired when the graph hits a checkpoint (every N iterations).
#[async_trait::async_trait]
pub trait CheckpointCallback: Send + Sync {
    async fn on_checkpoint(
        &self,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError>;
}

/// Hook fired when an error occurs during execution.
#[async_trait::async_trait]
pub trait ErrorCallback: Send + Sync {
    async fn on_error(
        &self,
        error: &CoreError,
        node: &str,
        state: Arc<Mutex<AgentState>>,
    );
}

/// Hook fired when the graph execution completes successfully.
#[async_trait::async_trait]
pub trait CompletionCallback: Send + Sync {
    async fn on_complete(
        &self,
        state: Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError>;
}

/// The full set of lifecycle hooks registered on the graph runner.
pub struct LifecycleHooks {
    pub before_tool: Vec<Box<dyn BeforeToolCallback>>,
    pub after_tool: Vec<Box<dyn AfterToolCallback>>,
    pub before_transition: Vec<Box<dyn BeforeNodeTransition>>,
    pub after_transition: Vec<Box<dyn AfterNodeTransition>>,
    pub checkpoint: Vec<Box<dyn CheckpointCallback>>,
    pub on_error: Vec<Box<dyn ErrorCallback>>,
    pub on_complete: Vec<Box<dyn CompletionCallback>>,
}

impl Default for LifecycleHooks {
    fn default() -> Self {
        Self {
            before_tool: Vec::new(),
            after_tool: Vec::new(),
            before_transition: Vec::new(),
            after_transition: Vec::new(),
            checkpoint: Vec::new(),
            on_error: Vec::new(),
            on_complete: Vec::new(),
        }
    }
}

impl LifecycleHooks {
    pub fn with_before_tool(
        mut self,
        hook: Box<dyn BeforeToolCallback>,
    ) -> Self {
        self.before_tool.push(hook);
        self
    }

    pub fn with_after_tool(
        mut self,
        hook: Box<dyn AfterToolCallback>,
    ) -> Self {
        self.after_tool.push(hook);
        self
    }
}
