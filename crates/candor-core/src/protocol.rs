/// Represents an action the agent intends to execute.
/// This is the payload inspected by the SentinelInterceptor
/// before any destructive state mutation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    /// Unique identifier for this action.
    pub id: String,

    /// The type of action being proposed.
    pub action_type: ActionType,

    /// The raw payload (code, command, message) to inspect.
    pub payload: String,

    /// Optional target path affected by this action.
    pub target_path: Option<String>,

    /// Whether this action is reversible.
    pub is_reversible: bool,

    /// The scope tags this action falls under.
    pub scope_tags: Vec<String>,

    /// The phase of the 7-phase machine this action belongs to.
    pub phase: String,

    /// Whether the sentinel has approved this action.
    pub sentinel_approved: bool,
}

impl AgentAction {
    pub fn is_destructive(&self) -> bool {
        matches!(
            self.action_type,
            ActionType::FileDelete
                | ActionType::ForcePush
                | ActionType::ShellCommand
                | ActionType::DatabaseWrite
        ) || !self.is_reversible
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionType {
    /// Generate code or text.
    GenerateCode,
    /// Execute a shell command.
    ShellCommand,
    /// Write to a file.
    FileWrite,
    /// Delete a file.
    FileDelete,
    /// Git operations.
    GitCommit,
    GitPush,
    ForcePush,
    /// Network operations.
    HttpRequest,
    /// Database operations.
    DatabaseRead,
    DatabaseWrite,
    /// Tool execution inside sandbox.
    SandboxExecution,
    /// Human approval request.
    ApprovalRequest,
    /// Memory operation.
    MemoryStore,
    MemoryRetrieve,
    /// Sentinel-only: audit result.
    SentinelAudit,
}
