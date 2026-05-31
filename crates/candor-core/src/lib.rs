// candor-core: foundational types, errors, and traits shared across all crates.

pub mod error;
pub mod ideal;
pub mod protocol;
pub mod state;

pub use error::CoreError;
pub use ideal::IdealStateArtifact;
pub use protocol::AgentAction;
pub use state::AgentState;
