/// Parser for Ideal State Artifact (ISA) markdown files.
///
/// From the design doc: "The Ideal State Artifact (ISA.md) replaces
/// ambiguous prompts. Every software engineering task requires an
/// explicit ISA.md defining the success criteria before the Execute
/// graph node triggers."
use std::path::Path;

use candor_core::error::CoreError;
use candor_core::ideal::{
    AcceptanceCriterion, ArtifactType, Constraint, ConstraintEnforcement, ExpectedArtifact,
    IdealStateArtifact, VerificationMethod,
};

/// Parse an ISA from a markdown string.
///
/// Expected format:
/// ```markdown
/// # Goal
/// <goal description>
///
/// ## Acceptance Criteria
/// - [id] description (verification: command/test/file/human)
///
/// ## Constraints
/// - [id] description
///
/// ## Expected Artifacts
/// - path/to/file: description
/// ```
pub fn parse_isa_from_markdown(id: &str, markdown: &str) -> Result<IdealStateArtifact, CoreError> {
    let mut goal = String::new();
    let mut criteria = Vec::new();
    let mut constraints = Vec::new();
    let mut artifacts = Vec::new();
    let mut fully_autonomous = true;

    let mut section: Option<&str> = None;

    for line in markdown.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("# ") && !trimmed.starts_with("## ") {
            section = Some("goal");
            goal = trimmed[2..].trim().to_string();
            continue;
        }

        if trimmed.contains("## Acceptance Criteria") || trimmed.contains("## Criteria") {
            section = Some("criteria");
            continue;
        }

        if trimmed.contains("## Constraints") {
            section = Some("constraints");
            continue;
        }

        if trimmed.contains("## Expected Artifacts") || trimmed.contains("## Artifacts") {
            section = Some("artifacts");
            continue;
        }

        if trimmed.contains("## Autonomous") || trimmed.contains("## Fully Autonomous") {
            section = Some("autonomous");
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with("---") {
            continue;
        }

        match section {
            Some("goal") => {
                if !goal.is_empty() {
                    goal.push('\n');
                }
                goal.push_str(trimmed);
            }
            Some("criteria") => {
                if let Some(criterion) = parse_criterion_line(trimmed) {
                    criteria.push(criterion);
                }
            }
            Some("constraints") => {
                if let Some(constraint) = parse_constraint_line(trimmed) {
                    constraints.push(constraint);
                }
            }
            Some("artifacts") => {
                if let Some(artifact) = parse_artifact_line(trimmed) {
                    artifacts.push(artifact);
                }
            }
            Some("autonomous")
                if (trimmed.to_lowercase().contains("false")
                    || trimmed.to_lowercase().contains("no")) =>
            {
                fully_autonomous = false;
            }
            _ => {}
        }
    }

    Ok(IdealStateArtifact {
        id: id.to_string(),
        goal,
        acceptance_criteria: criteria,
        constraints,
        expected_artifacts: artifacts,
        phase_requirements: Default::default(),
        fully_autonomous,
    })
}

/// Load and parse an ISA from a markdown file on disk.
pub async fn load_isa_from_file(path: &Path) -> Result<IdealStateArtifact, CoreError> {
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let markdown = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| CoreError::Io(e.to_string()))?;

    parse_isa_from_markdown(&id, &markdown)
}

fn parse_criterion_line(line: &str) -> Option<AcceptanceCriterion> {
    // Expected: - [id] description (verification: type)
    let cleaned = line.trim_start_matches(&['-', '*', ' '][..]).trim();

    if cleaned.is_empty() || !cleaned.starts_with('[') {
        return None;
    }

    let id_end = cleaned.find(']')?;
    let id = cleaned[1..id_end].to_string();
    let rest = cleaned[id_end + 1..].trim();

    let (description, method) = if let Some(paren_idx) = rest.rfind('(') {
        let desc = rest[..paren_idx].trim().to_string();
        let method_str = rest[paren_idx + 1..].trim_end_matches(')').trim();

        let method = if let Some(cmd) = method_str.strip_prefix("shell:") {
            VerificationMethod::ShellCommand {
                command: cmd.trim().to_string(),
            }
        } else if let Some(test_name) = method_str.strip_prefix("test:") {
            VerificationMethod::TestCase {
                test_name: test_name.trim().to_string(),
            }
        } else if let Some(path) = method_str.strip_prefix("file:") {
            VerificationMethod::FileExists {
                path: path.trim().to_string(),
            }
        } else if method_str.starts_with("human") {
            VerificationMethod::HumanConfirmation {
                prompt: desc.clone(),
            }
        } else if let Some(cmd) = method_str.strip_prefix("lint:") {
            VerificationMethod::LintCheck {
                command: cmd.trim().to_string(),
            }
        } else {
            VerificationMethod::ShellCommand {
                command: method_str.to_string(),
            }
        };

        (desc, method)
    } else {
        (
            rest.to_string(),
            VerificationMethod::HumanConfirmation {
                prompt: rest.to_string(),
            },
        )
    };

    Some(AcceptanceCriterion {
        id,
        description,
        verification_method: method,
    })
}

fn parse_constraint_line(line: &str) -> Option<Constraint> {
    let cleaned = line.trim_start_matches(&['-', '*', ' '][..]).trim();

    if cleaned.is_empty() || !cleaned.starts_with('[') {
        return None;
    }

    let id_end = cleaned.find(']')?;
    let id = cleaned[1..id_end].to_string();
    let description = cleaned[id_end + 1..].trim().to_string();

    Some(Constraint {
        id,
        description,
        enforcement: ConstraintEnforcement::PreExecution,
    })
}

fn parse_artifact_line(line: &str) -> Option<ExpectedArtifact> {
    let cleaned = line.trim_start_matches(&['-', '*', ' '][..]).trim();

    if cleaned.is_empty() {
        return None;
    }

    let (path, description) = if let Some(colon_idx) = cleaned.find(':') {
        let p = cleaned[..colon_idx].trim().to_string();
        let d = cleaned[colon_idx + 1..].trim().to_string();
        (p, d)
    } else {
        (cleaned.to_string(), String::new())
    };

    let artifact_type = if path.ends_with(".rs") {
        ArtifactType::SourceFile
    } else if path.contains("test") {
        ArtifactType::TestFile
    } else if path.ends_with(".md") {
        ArtifactType::MarkdownDocument
    } else {
        ArtifactType::Other {
            kind: "unknown".into(),
        }
    };

    Some(ExpectedArtifact {
        path,
        description,
        artifact_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ISA: &str = r#"
# Implement secure sandbox
Build a dual-engine sandbox with WASM and bubblewrap.

## Acceptance Criteria
- [sandbox-wasm] WASM executor works (shell: cargo test sandbox)
- [sandbox-bwrap] bubblewrap integration exists (test: test_bwrap_detection)
- [no-network] Network is denied by default (shell: grep 'network_allowed: false')

## Constraints
- [no-sudo] Must not require sudo
- [cross-plat] Must compile on Linux, macOS, Windows

## Expected Artifacts
- crates/candor-sandbox/src/wasm_exec.rs: WASM execution backend
- crates/candor-sandbox/src/process_exec.rs: Process sandbox backend
- crates/candor-sandbox/src/unified.rs: Unified sandbox engine
"#;

    #[test]
    fn test_parse_sample_isa() {
        let isa = parse_isa_from_markdown("test-isa", SAMPLE_ISA).unwrap();

        assert_eq!(isa.id, "test-isa");
        assert!(isa.goal.contains("dual-engine sandbox"));
        assert_eq!(isa.acceptance_criteria.len(), 3);
        assert_eq!(isa.constraints.len(), 2);
        assert_eq!(isa.expected_artifacts.len(), 3);

        assert_eq!(isa.acceptance_criteria[0].id, "sandbox-wasm");
        assert!(matches!(
            isa.acceptance_criteria[0].verification_method,
            VerificationMethod::ShellCommand { .. }
        ));
    }
}
