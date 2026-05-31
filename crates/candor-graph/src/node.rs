/// A discrete unit of work within the agent graph.
/// Every node implements this trait and is stored in the DiGraph.
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_core::error::CoreError;
use candor_core::state::AgentState;

#[async_trait::async_trait]
pub trait AgentNode: Send + Sync {
    /// Human-readable name for telemetry and logging.
    fn name(&self) -> &str;

    /// Execute this node's work, reading from and writing to shared state.
    ///
    /// IMPORTANT: Implementations MUST scope mutex locks to the minimum
    /// necessary lines. Holding a lock across an await point (network I/O,
    /// subprocess spawn) causes Tokio executor deadlocks. See design doc
    /// Troubleshooting Protocol: Graph Deadlocks.
    async fn execute(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError>;
}
