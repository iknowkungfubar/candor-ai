/// Markdown-driven routing module.
///
/// Dynamically loads SYSTEM.md and ISA.md as agent prompts,
/// extracting operating doctrine, ISA criteria, constraints, and goal.
///
/// From the design doc: "Following the established standard for production
/// agent infrastructure, all operational context and routing logic must be
/// instantiated as simple markdown files."
use std::path::Path;
use tracing::info;

/// The structured context extracted from SYSTEM.md and ISA.md markdown files.
///
/// This struct is injectable into the agent's system prompt to provide
/// deterministic operating constraints and success criteria.
#[derive(Debug, Clone)]
pub struct MarkdownContext {
    /// Operating doctrines from SYSTEM.md (section 1 + No-Slop Guardrails).
    pub doctrine: String,
    /// ISA acceptance criteria descriptions (section 7 from ISA.md).
    pub criteria: Vec<String>,
    /// ISA constraints (section 5 from ISA.md).
    pub constraints: Vec<String>,
    /// ISA goal (section 6 from ISA.md).
    pub goal: String,
}

impl MarkdownContext {
    /// Format the full context as a system prompt preamble.
    ///
    /// Produces a structured string suitable for injection into
    /// the CognitiveEngine's system prompt.
    pub fn format_system_prompt(&self) -> String {
        let mut parts = Vec::new();

        if !self.doctrine.is_empty() {
            parts.push("=== OPERATING DOCTRINE ===".to_string());
            parts.push(self.doctrine.clone());
            parts.push(String::new());
        }

        if !self.goal.is_empty() {
            parts.push("=== PROJECT GOAL ===".to_string());
            parts.push(self.goal.clone());
            parts.push(String::new());
        }

        if !self.criteria.is_empty() {
            parts.push("=== ACCEPTANCE CRITERIA ===".to_string());
            for (i, c) in self.criteria.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, c));
            }
            parts.push(String::new());
        }

        if !self.constraints.is_empty() {
            parts.push("=== CONSTRAINTS ===".to_string());
            for (i, c) in self.constraints.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, c));
            }
        }

        parts.join("\n")
    }

    /// Return true if any context fields are populated.
    pub fn is_empty(&self) -> bool {
        self.doctrine.is_empty()
            && self.criteria.is_empty()
            && self.constraints.is_empty()
            && self.goal.is_empty()
    }
}

/// Load markdown prompts from SYSTEM.md and ISA.md in the given root directory.
///
/// # Arguments
///
/// * `root_dir` - Path to the project root containing SYSTEM.md and ISA.md.
///
/// # Returns
///
/// A `MarkdownContext` populated with structured data extracted from the
/// markdown files. Missing files result in empty fields (no error).
pub fn load_markdown_prompts(root_dir: &Path) -> MarkdownContext {
    let system_path = root_dir.join("SYSTEM.md");
    let isa_path = root_dir.join("ISA.md");

    let doctrine = load_and_parse_system(&system_path);
    let (goal, criteria, constraints) = load_and_parse_isa(&isa_path);

    let ctx = MarkdownContext {
        doctrine,
        criteria,
        constraints,
        goal,
    };

    info!(
        doctrine_len = ctx.doctrine.len(),
        criteria_count = ctx.criteria.len(),
        constraints_count = ctx.constraints.len(),
        "Markdown prompt context loaded"
    );

    ctx
}

/// Load and parse SYSTEM.md, extracting operating doctrine sections.
///
/// Extracts:
/// - "Core Operating Doctrine" and its numbered principles
/// - "No-Slop Guardrails" section
/// - "The Autonomous 7-Phase Execution Algorithm" section
fn load_and_parse_system(path: &Path) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            info!(error = %e, path = %path.display(), "SYSTEM.md not found or unreadable");
            return String::new();
        }
    };

    let mut doctrine_parts = Vec::new();
    let mut current_section = String::new();
    let mut capturing = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Section headers
        if trimmed.starts_with('#') {
            // Flush current section if we were capturing
            let capture = !current_section.trim().is_empty();
            if capture {
                doctrine_parts.push(current_section.trim().to_string());
                current_section = String::new();
            }

            let header = trimmed.to_lowercase();

            let is_doctrine = header.contains("core operating doctrine");
            let is_guardrails = header.contains("no-slop guardrails");
            let is_algorithm = header.contains("7-phase") || header.contains("execution algorithm");
            let is_captured = is_doctrine || is_guardrails || is_algorithm;

            if is_captured {
                // Include the header itself
                current_section.push_str(trimmed);
                current_section.push('\n');
            }
            capturing = is_captured;
            continue;
        }

        if capturing {
            current_section.push_str(trimmed);
            current_section.push('\n');
        }
    }

    // Flush remaining section
    if capturing && !current_section.trim().is_empty() {
        doctrine_parts.push(current_section.trim().to_string());
    }

    doctrine_parts.join("\n\n")
}

/// Load and parse ISA.md, extracting goal, criteria, and constraints.
///
/// Expected sections:
/// - `## Constraints` (section 5): bullet-pointed constraint descriptions
/// - `## Goal` or `# Goal` or `## 6. Goal` (section 6): goal statement
/// - `## Criteria` or `## 7. Criteria` or `## Acceptance Criteria` (section 7): checklist items
fn load_and_parse_isa(path: &Path) -> (String, Vec<String>, Vec<String>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            info!(error = %e, path = %path.display(), "ISA.md not found or unreadable");
            return (String::new(), Vec::new(), Vec::new());
        }
    };

    let mut goal = String::new();
    let mut criteria = Vec::new();
    let mut constraints = Vec::new();

    let mut section: Option<&str> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect section headers by matching goal/criteria/constraints headings
        if trimmed.starts_with('#') {
            let header = trimmed.to_lowercase();

            if header.contains("goal") {
                section = Some("goal");
                // Extract goal text from the header line itself (e.g., "# 6. Goal" content from "6. Goal")
                let goal_text = extract_header_content(trimmed, &["goal"]);
                if !goal_text.is_empty() {
                    if !goal.is_empty() {
                        goal.push(' ');
                    }
                    goal.push_str(&goal_text);
                }
                continue;
            }

            if header.contains("constraints") {
                section = Some("constraints");
                continue;
            }

            if header.contains("criteria") || header.contains("acceptance") {
                section = Some("criteria");
                continue;
            }
        }

        match section {
            Some("goal") => {
                // Skip empty lines, separator lines, and any new headers in goal
                if trimmed.is_empty() || trimmed.starts_with("---") || trimmed.starts_with('#') {
                    continue;
                }
                if !goal.is_empty() {
                    goal.push(' ');
                }
                goal.push_str(trimmed);
            }
            Some("criteria") => {
                if let Some(criterion) = extract_list_item(trimmed) {
                    criteria.push(criterion);
                }
            }
            Some("constraints") => {
                let cleaned = trimmed
                    .trim_start_matches(&['-', '*', ' '][..])
                    .trim()
                    .trim_start_matches(|c: char| c.is_ascii_digit())
                    .trim_start_matches('.')
                    .trim();
                if !cleaned.is_empty() {
                    constraints.push(cleaned.to_string());
                }
            }
            _ => {}
        }
    }

    (goal.trim().to_string(), criteria, constraints)
}

/// Extract content from a header line, removing the markdown heading markers
/// and optionally filtering out keywords.
fn extract_header_content(header: &str, skip_keywords: &[&str]) -> String {
    let mut parts: Vec<&str> = header
        .trim_start_matches('#')
        .split_whitespace()
        .filter(|word| {
            let lower = word
                .trim_matches(|c: char| c.is_ascii_punctuation())
                .to_lowercase();
            !skip_keywords.contains(&lower.as_str()) && !lower.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    parts.dedup();
    parts.join(" ")
}

/// Extract a list item (checklist or bullet) into its description string.
///
/// Handles formats:
/// - `- [ ] description`
/// - `- [x] description`
/// - `* [ ] description`
/// - `- description`
/// - `[ ] description`
fn extract_list_item(trimmed: &str) -> Option<String> {
    let cleaned = trimmed.trim_start_matches(&['-', '*', ' '][..]).trim();

    // Handle checkbox format: [ ] or [x]
    let without_checkbox = if cleaned.starts_with('[') {
        if let Some(end) = cleaned.find(']') {
            cleaned[end + 1..].trim()
        } else {
            return None;
        }
    } else {
        cleaned
    };

    if without_checkbox.is_empty() {
        return None;
    }

    // Skip purely numeric or short items
    if without_checkbox.len() < 5 {
        return None;
    }

    Some(without_checkbox.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_md(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.md");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "{}", content).unwrap();
        (dir, path)
    }

    #[test]
    fn test_load_markdown_prompts_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = load_markdown_prompts(dir.path());
        assert!(ctx.is_empty());
        assert!(ctx.doctrine.is_empty());
        assert!(ctx.criteria.is_empty());
        assert!(ctx.constraints.is_empty());
        assert!(ctx.goal.is_empty());
    }

    #[test]
    fn test_load_markdown_prompts_full() {
        let dir = tempfile::tempdir().unwrap();

        // Create SYSTEM.md
        let sys_content = r#"# Agent Execution Architecture: Lawful Good Operating Doctrine

## Core Operating Doctrine
1. Precision Over Persuasion
2. Systems Before Tools
3. Failure Is the Primary Use Case
4. Simplicity Is an Ethical Choice
5. Epistemic Honesty

## No-Slop Guardrails
- Verify-First: Read local files before acting.
- Scope-Lock: Do only what is defined in the ISA.
- No-Slop Code: Reject dead code and vague TODOs.
- Test-Then-Ship: All tests must pass before commit.
- Git-Discipline: Use feature branches and conventional commits.

## The Autonomous 7-Phase Execution Algorithm
OBSERVE → THINK → PLAN → BUILD → EXECUTE → VERIFY → LEARN
"#;
        let sys_path = dir.path().join("SYSTEM.md");
        std::fs::write(&sys_path, sys_content).unwrap();

        // Create ISA.md
        let isa_content = r#"# Master Project ISA: Agentic Operating System

## 5. Constraints
- Must compile on Linux, macOS, and Windows.
- Must maintain low memory footprint for consumer hardware.
- Must enforce the 7-phase algorithm for all state mutations.

## 6. Goal
Deliver a production-ready, highly reliable agentic harness.

## 7. Criteria
- [ ] The Rust workspace compiles cleanly with zero warnings.
- [ ] An Axum server binds to 127.0.0.1:31337.
- [ ] The CognitiveEngine routes between cloud and local models.
"#;
        let isa_path = dir.path().join("ISA.md");
        std::fs::write(&isa_path, isa_content).unwrap();

        let ctx = load_markdown_prompts(dir.path());

        assert!(!ctx.is_empty());
        assert!(ctx.doctrine.contains("Precision Over Persuasion"));
        assert!(ctx.doctrine.contains("Verify-First"));
        assert!(ctx.doctrine.contains("7-Phase Execution Algorithm"));

        assert_eq!(ctx.criteria.len(), 3);
        assert!(ctx.criteria[0].contains("compiles cleanly"));

        assert_eq!(ctx.constraints.len(), 3);
        assert!(ctx.constraints[0].contains("Linux, macOS, and Windows"));

        assert!(ctx.goal.contains("production-ready"));
    }

    #[test]
    fn test_format_system_prompt_empty() {
        let ctx = MarkdownContext {
            doctrine: String::new(),
            criteria: Vec::new(),
            constraints: Vec::new(),
            goal: String::new(),
        };
        let prompt = ctx.format_system_prompt();
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_format_system_prompt_full() {
        let ctx = MarkdownContext {
            doctrine: "Precision Over Persuasion".into(),
            criteria: vec!["Must compile".into(), "Must bind port".into()],
            constraints: vec!["No sudo required".into()],
            goal: "Build the agentic OS".into(),
        };
        let prompt = ctx.format_system_prompt();
        assert!(prompt.contains("=== OPERATING DOCTRINE ==="));
        assert!(prompt.contains("=== PROJECT GOAL ==="));
        assert!(prompt.contains("=== ACCEPTANCE CRITERIA ==="));
        assert!(prompt.contains("=== CONSTRAINTS ==="));
        assert!(prompt.contains("Precision Over Persuasion"));
        assert!(prompt.contains("Build the agentic OS"));
        assert!(prompt.contains("Must compile"));
        assert!(prompt.contains("No sudo required"));
    }

    #[test]
    fn test_extract_list_item_checkbox() {
        let item = extract_list_item("- [ ] The Rust workspace compiles cleanly.").unwrap();
        assert_eq!(item, "The Rust workspace compiles cleanly.");
    }

    #[test]
    fn test_extract_list_item_checked() {
        let item = extract_list_item("- [x] Tests pass.").unwrap();
        assert_eq!(item, "Tests pass.");
    }

    #[test]
    fn test_extract_list_item_bullet() {
        let item = extract_list_item("- Must compile cross-platform.").unwrap();
        assert_eq!(item, "Must compile cross-platform.");
    }

    #[test]
    fn test_system_doctrine_extraction() {
        let content = "# Main Header\n\n## Core Operating Doctrine\nPrecision Over Persuasion\nSystems Before Tools\n\n## No-Slop Guardrails\nVerify-First: Read files before acting.\n\n## The Autonomous 7-Phase Execution Algorithm\nOBSERVE → THINK → PLAN\n";
        let (_dir, path) = create_temp_md(content);
        let result = load_and_parse_system(&path);
        assert!(result.contains("Core Operating Doctrine"));
        assert!(result.contains("Precision Over Persuasion"));
        assert!(result.contains("No-Slop Guardrails"));
        assert!(result.contains("7-Phase Execution Algorithm"));
    }

    #[test]
    fn test_isa_goal_extraction() {
        let content = "# Project\n\n## 6. Goal\nDeliver a production-ready agentic harness.\n";
        let (_dir, path) = create_temp_md(content);
        let (goal, criteria, constraints) = load_and_parse_isa(&path);
        assert!(goal.contains("production-ready agentic harness"));
        assert!(criteria.is_empty());
        assert!(constraints.is_empty());
    }

    #[test]
    fn test_isa_constraints_extraction() {
        let content =
            "# Project\n\n## 5. Constraints\n- Must compile on Linux.\n- Must not require sudo.\n";
        let (_dir, path) = create_temp_md(content);
        let (goal, criteria, constraints) = load_and_parse_isa(&path);
        assert!(goal.is_empty());
        assert!(criteria.is_empty());
        assert_eq!(constraints.len(), 2);
        assert!(constraints[0].contains("Linux"));
    }

    #[test]
    fn test_load_nonexistent_dir() {
        let dir = Path::new("/nonexistent/path/that/does/not/exist");
        let ctx = load_markdown_prompts(dir);
        assert!(ctx.is_empty());
    }
}
