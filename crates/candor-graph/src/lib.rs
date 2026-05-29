// candor-graph: petgraph-based deterministic orchestration engine.
//
// Models the agent's reasoning as a strict state machine using DiGraph.
// Nodes = tasks. Edges = conditional routing. Tokio runs nodes concurrently.
//
// From the design doc: "The primary bottleneck is state contention.
// Scoping the mutex lock strictly avoids deadlocks across await boundaries."

pub mod runner;
pub mod node;
pub mod hooks;
pub mod checkpoint;
pub mod recovery;
pub mod full_hooks;

pub use full_hooks::FullLifecycleHooks;
pub use hooks::LifecycleHooks;
pub use node::AgentNode;
pub use recovery::{RecoveryNode, RecoveryStrategy, analyze_error};
pub use runner::GraphRunner;
