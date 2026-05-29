/// Full 17 lifecycle events per the Claude Code pattern.
///
/// From design doc Phase 3, Action Item 3.3:
/// "Inject 17 lifecycle events, adopting the Claude Code pattern,
/// allowing deterministic shell-script hooks to analyze state data
/// prior to any tool execution."
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_core::error::CoreError;
use candor_core::protocol::AgentAction;
use candor_core::state::AgentState;

// ── 17 Lifecycle Hook Traits ──

/// 1. Before any tool execution
#[async_trait::async_trait]
pub trait BeforeToolCallback: Send + Sync {
    async fn before_tool(&self, action: &AgentAction, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 2. After any tool execution
#[async_trait::async_trait]
pub trait AfterToolCallback: Send + Sync {
    async fn after_tool(&self, action: &AgentAction, result: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 3. Before node transition
#[async_trait::async_trait]
pub trait BeforeNodeTransition: Send + Sync {
    async fn before_transition(&self, from_node: &str, to_node: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 4. After node transition
#[async_trait::async_trait]
pub trait AfterNodeTransition: Send + Sync {
    async fn after_transition(&self, from_node: &str, to_node: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 5. Checkpoint (every N iterations)
#[async_trait::async_trait]
pub trait CheckpointCallback: Send + Sync {
    async fn on_checkpoint(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 6. On error
#[async_trait::async_trait]
pub trait ErrorCallback: Send + Sync {
    async fn on_error(&self, error: &CoreError, node: &str, state: Arc<Mutex<AgentState>>);
}

/// 7. On completion
#[async_trait::async_trait]
pub trait CompletionCallback: Send + Sync {
    async fn on_complete(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 8. Before phase entry
#[async_trait::async_trait]
pub trait BeforePhaseEntry: Send + Sync {
    async fn before_phase(&self, phase: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 9. After phase exit
#[async_trait::async_trait]
pub trait AfterPhaseExit: Send + Sync {
    async fn after_phase(&self, phase: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 10. Before file read
#[async_trait::async_trait]
pub trait BeforeFileRead: Send + Sync {
    async fn before_read(&self, path: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 11. After file write
#[async_trait::async_trait]
pub trait AfterFileWrite: Send + Sync {
    async fn after_write(&self, path: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 12. Before git operation
#[async_trait::async_trait]
pub trait BeforeGitOp: Send + Sync {
    async fn before_git(&self, operation: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 13. After git operation
#[async_trait::async_trait]
pub trait AfterGitOp: Send + Sync {
    async fn after_git(&self, operation: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 14. Before sandbox execution
#[async_trait::async_trait]
pub trait BeforeSandboxExec: Send + Sync {
    async fn before_sandbox(&self, code: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 15. After sandbox execution
#[async_trait::async_trait]
pub trait AfterSandboxExec: Send + Sync {
    async fn after_sandbox(&self, result: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 16. Before embedding generation
#[async_trait::async_trait]
pub trait BeforeEmbedding: Send + Sync {
    async fn before_embed(&self, text: &str, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

/// 17. On loop iteration (every cycle of the execution loop)
#[async_trait::async_trait]
pub trait OnLoopIteration: Send + Sync {
    async fn on_iteration(&self, count: u32, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}

// ── Full Lifecycle Hooks Registry ──

#[derive(Default)]
pub struct FullLifecycleHooks {
    pub before_tool: Vec<Box<dyn BeforeToolCallback>>,
    pub after_tool: Vec<Box<dyn AfterToolCallback>>,
    pub before_transition: Vec<Box<dyn BeforeNodeTransition>>,
    pub after_transition: Vec<Box<dyn AfterNodeTransition>>,
    pub checkpoint: Vec<Box<dyn CheckpointCallback>>,
    pub on_error: Vec<Box<dyn ErrorCallback>>,
    pub on_complete: Vec<Box<dyn CompletionCallback>>,
    pub before_phase: Vec<Box<dyn BeforePhaseEntry>>,
    pub after_phase: Vec<Box<dyn AfterPhaseExit>>,
    pub before_read: Vec<Box<dyn BeforeFileRead>>,
    pub after_write: Vec<Box<dyn AfterFileWrite>>,
    pub before_git: Vec<Box<dyn BeforeGitOp>>,
    pub after_git: Vec<Box<dyn AfterGitOp>>,
    pub before_sandbox: Vec<Box<dyn BeforeSandboxExec>>,
    pub after_sandbox: Vec<Box<dyn AfterSandboxExec>>,
    pub before_embed: Vec<Box<dyn BeforeEmbedding>>,
    pub on_iteration: Vec<Box<dyn OnLoopIteration>>,
}


impl FullLifecycleHooks {
    pub fn with_before_tool(mut self, hook: Box<dyn BeforeToolCallback>) -> Self {
        self.before_tool.push(hook);
        self
    }

    pub fn with_after_tool(mut self, hook: Box<dyn AfterToolCallback>) -> Self {
        self.after_tool.push(hook);
        self
    }

    pub fn with_before_phase(mut self, hook: Box<dyn BeforePhaseEntry>) -> Self {
        self.before_phase.push(hook);
        self
    }

    pub fn with_after_phase(mut self, hook: Box<dyn AfterPhaseExit>) -> Self {
        self.after_phase.push(hook);
        self
    }

    pub fn with_before_git(mut self, hook: Box<dyn BeforeGitOp>) -> Self {
        self.before_git.push(hook);
        self
    }

    pub fn with_before_sandbox(mut self, hook: Box<dyn BeforeSandboxExec>) -> Self {
        self.before_sandbox.push(hook);
        self
    }

    pub fn with_on_iteration(mut self, hook: Box<dyn OnLoopIteration>) -> Self {
        self.on_iteration.push(hook);
        self
    }
}

// Re-export the original hooks for backward compatibility
pub use super::LifecycleHooks;
