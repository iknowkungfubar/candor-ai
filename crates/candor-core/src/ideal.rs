/// The Ideal State Artifact (ISA) defines exact success criteria
/// for a given agent task. Nodes must satisfy all ISA fields before
/// the graph can transition from Build to Execute.
///
/// From the design doc: "Replace ambiguous prompts. Every software
/// engineering task requires an explicit ISA.md defining the success
/// criteria before the Execute graph node triggers."
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdealStateArtifact {
    /// Unique identifier for this artifact.
    pub id: String,

    /// Human-readable summary of the desired outcome.
    pub goal: String,

    /// Ordered list of acceptance criteria that must all pass.
    pub acceptance_criteria: Vec<AcceptanceCriterion>,

    /// Constraints that must not be violated during execution.
    pub constraints: Vec<Constraint>,

    /// The expected output artifacts (files, commits, etc.).
    pub expected_artifacts: Vec<ExpectedArtifact>,

    /// Phase-specific requirements mapped by phase name.
    pub phase_requirements: HashMap<String, Vec<String>>,

    /// Whether this ISA can be satisfied without human approval.
    pub fully_autonomous: bool,
}

impl IdealStateArtifact {
    /// Return all unmet criteria for the given set of verification results.
    pub fn unmet_criteria(&self, results: &HashMap<String, bool>) -> Vec<&AcceptanceCriterion> {
        self.acceptance_criteria
            .iter()
            .filter(|c| !results.get(&c.id).copied().unwrap_or(false))
            .collect()
    }

    /// Check if every criterion is satisfied.
    pub fn is_satisfied(&self, results: &HashMap<String, bool>) -> bool {
        self.acceptance_criteria
            .iter()
            .all(|c| results.get(&c.id).copied().unwrap_or(false))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    /// Unique identifier (used for result mapping).
    pub id: String,
    /// Human-readable description of what must be true.
    pub description: String,
    /// How to verify this criterion (shell command, test name, etc.).
    pub verification_method: VerificationMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VerificationMethod {
    /// Run a shell command and check exit code.
    ShellCommand { command: String },
    /// Run a specific test by name/path.
    TestCase { test_name: String },
    /// Check that a file exists at the given path.
    FileExists { path: String },
    /// Check that a file's content matches a regex.
    FileMatches { path: String, pattern: String },
    /// Run linting/type-checking.
    LintCheck { command: String },
    /// Require explicit human confirmation.
    HumanConfirmation { prompt: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub id: String,
    pub description: String,
    pub enforcement: ConstraintEnforcement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConstraintEnforcement {
    /// Blocked at the sentinel level before tool execution.
    PreExecution,
    /// Checked during verification phase.
    PostExecution,
    /// Required before commit/merge.
    PreCommit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedArtifact {
    pub path: String,
    pub description: String,
    pub artifact_type: ArtifactType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArtifactType {
    SourceFile,
    TestFile,
    MarkdownDocument,
    Commit,
    BinaryOutput,
    Other { kind: String },
}
