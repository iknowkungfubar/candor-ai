/// Exhaustive error states for the candor-ai platform.
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum CoreError {
    #[error("Graph execution error: {0}")]
    GraphExecution(String),

    #[error("Sandbox execution trap: {0}")]
    SandboxTrap(String),

    #[error("Sandbox resource exhausted")]
    SandboxResourceExhausted,

    #[error("Inference error: {0}")]
    Inference(String),

    #[error("Memory system error: {0}")]
    MemorySystem(String),

    #[error("Sentinel policy violation: {0}")]
    SentinelPolicyViolation(String),

    #[error("Sentinel semantic rejection: {0}")]
    SentinelSemanticRejection(String),

    #[error("Ideal state artifact not satisfied: {0}")]
    IdealStateNotSatisfied(String),

    #[error("Maximum iteration limit reached")]
    MaxIterationsReached,

    #[error("Human approval denied for tool execution")]
    HumanApprovalDenied,

    #[error("State corruption detected: {0}")]
    StateCorruption(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        CoreError::Serialization(e.to_string())
    }
}

/// Convert any CoreError into the GraphExecution variant so graph-runner
/// errors propagate cleanly through the recovery loop.
impl CoreError {
    pub fn into_graph_execution(self) -> Self {
        CoreError::GraphExecution(self.to_string())
    }
}
