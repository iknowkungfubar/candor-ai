/// The seven phases of the Algorithm v6.3.0 state machine.
///
/// Each phase is a named node in the petgraph orchestration.
/// The graph enforces strict sequential progression with no
/// backward edges that could create infinite loops.

use serde::{Deserialize, Serialize};

/// The canonical seven phases.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum Phase {
    /// Gather all relevant context: files, docs, environment state.
    Observe,
    /// Reason about the gathered context and identify the problem.
    Think,
    /// Produce a concrete implementation plan.
    Plan,
    /// Generate the output artifacts (code, docs, configs).
    Build,
    /// Execute/build the generated artifacts in the sandbox.
    Execute,
    /// Verify outputs against the Ideal State Artifact.
    Verify,
    /// Extract lessons, update skills, store memory.
    Learn,
}

impl Phase {
    /// Return the phases in their canonical order.
    pub fn ordered() -> [Phase; 7] {
        [
            Phase::Observe,
            Phase::Think,
            Phase::Plan,
            Phase::Build,
            Phase::Execute,
            Phase::Verify,
            Phase::Learn,
        ]
    }

    /// Human-readable name.
    pub fn name(&self) -> &str {
        match self {
            Phase::Observe => "Observe",
            Phase::Think => "Think",
            Phase::Plan => "Plan",
            Phase::Build => "Build",
            Phase::Execute => "Execute",
            Phase::Verify => "Verify",
            Phase::Learn => "Learn",
        }
    }

    /// The next phase in the sequence (None if this is the last).
    pub fn next(&self) -> Option<Phase> {
        match self {
            Phase::Observe => Some(Phase::Think),
            Phase::Think => Some(Phase::Plan),
            Phase::Plan => Some(Phase::Build),
            Phase::Build => Some(Phase::Execute),
            Phase::Execute => Some(Phase::Verify),
            Phase::Verify => Some(Phase::Learn),
            Phase::Learn => None,
        }
    }

    /// Whether this phase requires the ISA to be satisfied before advancing.
    pub fn requires_isa_satisfied(&self) -> bool {
        matches!(self, Phase::Verify)
    }

    /// Whether this phase requires sentinel approval.
    pub fn requires_sentinel(&self) -> bool {
        matches!(
            self,
            Phase::Build | Phase::Execute | Phase::Verify
        )
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_order_length() {
        assert_eq!(Phase::ordered().len(), 7);
    }

    #[test]
    fn test_phase_chain() {
        let mut phase = Some(Phase::Observe);
        let mut count = 0;
        while let Some(p) = phase {
            count += 1;
            phase = p.next();
        }
        assert_eq!(count, 7);
    }
}
