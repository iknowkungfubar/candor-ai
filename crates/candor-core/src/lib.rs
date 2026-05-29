// candor-core: foundational types, errors, and traits shared across all crates.

pub mod error;
pub mod state;
pub mod ideal;
pub mod protocol;

pub use error::CoreError;
pub use ideal::IdealStateArtifact;
pub use protocol::AgentAction;
pub use state::AgentState;
