/// Skills system: self-building SKILL.md files from successful task trajectories.
///
/// From the design doc Phase 6: "Expand the Learn node in the graph to
/// automatically generate new deterministic .md skill files upon task success."
use std::path::PathBuf;
use tracing::info;

use candor_core::error::CoreError;

/// A learned skill captured from a successful task trajectory.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Unique name (hyphenated, lowercase).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// When to trigger this skill.
    pub trigger: String,
    /// Step-by-step instructions.
    pub steps: Vec<String>,
    /// Tools used during the task.
    pub tools_used: Vec<String>,
    /// Pitfalls encountered.
    pub pitfalls: Vec<String>,
    /// Number of times this skill has been reinforced.
    pub use_count: u32,
}

impl Skill {
    /// Render a skill as a SKILL.md markdown document.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("---\n");
        md.push_str(&format!("name: {}\n", self.name));
        md.push_str(&format!("description: \"{}\"\n", self.description));
        md.push_str(&format!("trigger: \"{}\"\n", self.trigger));
        md.push_str("hermes:\n");
        md.push_str(&format!("  use_count: {}\n", self.use_count));

        let tools_str = self.tools_used.iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>()
            .join(", ");
        md.push_str(&format!("  tools: [{}]\n", tools_str));
        md.push_str("---\n\n");
        md.push_str(&format!("# {}\n\n", self.name.replace('-', " ").to_uppercase()));
        md.push_str(&format!("{}\n\n", self.description));
        md.push_str("## Steps\n\n");

        for (i, step) in self.steps.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", i + 1, step));
        }

        if !self.pitfalls.is_empty() {
            md.push_str("\n## Pitfalls\n\n");
            for pitfall in &self.pitfalls {
                md.push_str(&format!("- **{pitfall}**\n"));
            }
        }

        md
    }
}

/// Extract skills from a session's execution log.
pub fn extract_skills_from_log(
    task: &str,
    log: &[String],
    tools_used: &[String],
) -> Vec<Skill> {
    let mut skills = Vec::new();

    // Look for patterns indicating a completed task
    let test_passed = log.iter().any(|e| e.contains("Verify: PASSED"));
    let completed = log.iter().any(|e| e.contains("Task complete"));

    if test_passed && completed {
        // Extract the plan steps as skill steps
        let plan_content = log.iter()
            .find(|e| e.contains("Plan:") || e.contains("Plan:\n"))
            .map(|e| e.to_string())
            .unwrap_or_default();

        let steps: Vec<String> = plan_content
            .lines()
            .filter(|l| l.trim().starts_with(|c: char| c.is_ascii_digit()))
            .map(|l| l.trim().to_string())
            .collect();

        // Generate a skill name from the task
        let name = task
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .take(5)
            .collect::<Vec<_>>()
            .join("-");

        let pitfalls = log.iter()
            .filter(|e| e.contains("failed") || e.contains("error") || e.contains("FAILED")).cloned()
            .take(5)
            .collect();

        let skill = Skill {
            name,
            description: task.to_string(),
            trigger: task.to_string(),
            steps: if steps.is_empty() {
                vec!["Execute the build phase".into(), "Run tests in Verify phase".into()]
            } else {
                steps
            },
            tools_used: tools_used.to_vec(),
            pitfalls,
            use_count: 1,
        };

        skills.push(skill);
    }

    skills
}

/// Write skills to disk as SKILL.md files.
pub async fn persist_skills(
    skills: &[Skill],
    skills_dir: &PathBuf,
) -> Result<usize, CoreError> {
    tokio::fs::create_dir_all(skills_dir)
        .await
        .map_err(|e| CoreError::Io(e.to_string()))?;

    let mut written = 0;
    for skill in skills {
        let path = skills_dir.join(format!("{}.SKILL.md", skill.name));
        let content = skill.to_markdown();

        // If the skill already exists, merge and increment use count
        if path.exists() {
            info!(skill = %skill.name, "Updating existing skill");
            if let Ok(existing) = tokio::fs::read_to_string(&path).await
                && existing.contains(&skill.description) {
                    // Same skill, bump counter
                    let updated = existing.replace("use_count: 1", &format!("use_count: {}", skill.use_count + 1));
                    tokio::fs::write(&path, updated).await
                        .map_err(|e| CoreError::Io(e.to_string()))?;
                    written += 1;
                    continue;
                }
        }

        tokio::fs::write(&path, content)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        info!(skill = %skill.name, path = %path.display(), "Skill persisted");
        written += 1;
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_to_markdown() {
        let skill = Skill {
            name: "test-skill".into(),
            description: "A test skill".into(),
            trigger: "when testing".into(),
            steps: vec!["Step 1".into(), "Step 2".into()],
            tools_used: vec!["read_file".into(), "shell".into()],
            pitfalls: vec!["Pitfall 1".into()],
            use_count: 1,
        };

        let md = skill.to_markdown();
        assert!(md.contains("name: test-skill"));
        assert!(md.contains("Step 1"));
        assert!(md.contains("Pitfall 1"));
    }

    #[test]
    fn test_extract_skills_from_successful_log() {
        let log = vec![
            "Task: test feature".into(),
            "Execute: cargo check complete".into(),
            "Verify: PASSED".into(),
            "Task complete".into(),
        ];

        let skills = extract_skills_from_log("test feature", &log, &["shell".into()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test-feature");
    }

    #[test]
    fn test_no_skill_from_failed_log() {
        let log = vec![
            "Task: broken feature".into(),
            "Verify: FAILED".into(),
        ];

        let skills = extract_skills_from_log("broken feature", &log, &[]);
        assert_eq!(skills.len(), 0);
    }
}
