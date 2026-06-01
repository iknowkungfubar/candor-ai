// candor-orchestrator: the 7-phase algorithmic state machine.
//
// From the design doc: "The core orchestration logic abandons open-ended
// loops in favor of the Algorithm v6.3.0. This enforces a strict seven-phase
// state machine: Observe, Think, Plan, Build, Execute, Verify, and Learn."
//
// "Following the Antigravity paradigm, the verification phase mandates the
// generation of an Ideal State Artifact (ISA). This artifact defines exact
// success criteria programmatically. The system hill-climbs toward this
// state, replacing raw tool execution logs with tangible deliverables."

pub mod approval_gate;
pub mod engine;
pub mod isa_parser;
pub mod markdown_router;
pub mod memory_nudge;
pub mod phases;
pub mod skills;
pub mod trajectory;

pub use engine::OrchestratorEngine;
