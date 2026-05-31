/// Deterministic rule-based guardrails enforced by the Sentinel.
///
/// These rules run synchronously and are checked before any
/// semantic audit. From the design doc:
/// "Signature verification, payload syntax checking, Git command
/// inspection, and explicit regex blacklists run synchronously."
use regex::Regex;
use std::sync::LazyLock;

use candor_core::error::CoreError;

/// The result of a rules check.
#[derive(Debug, Clone)]
pub struct RulesCheck {
    /// Whether the payload passed all rules.
    pub passed: bool,
    /// Specific violations found.
    pub violations: Vec<RuleViolation>,
}

#[derive(Debug, Clone)]
pub struct RuleViolation {
    pub rule: String,
    pub description: String,
    pub severity: ViolationSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolationSeverity {
    /// Blocking — must be fixed before execution proceeds.
    Fatal,
    /// Warning — logged but not blocking.
    Warning,
}

// ── Compiled regexes for common slop patterns ──
static TODO_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)//\s*TODO|#\s*TODO|/\*.*?TODO").unwrap());

static NARRATION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(this\s+function|here\s+we|let's|now\s+we|first\s+we|next\s+we)\s+(create|define|implement|set\s+up|build|add|write)").unwrap()
});

static FORCE_PUSH_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+push\s+(-f|--force)").unwrap());

static RM_RF_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"rm\s+-rf\s+/").unwrap());

static DEAD_CODE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(if\s+false|while\s+false|unreachable!\(\s*"never"\s*\))"#).unwrap()
});

/// Run all deterministic rules against a payload.
pub fn enforce_deterministic_rules(payload: &str, valid_scopes: &[String]) -> RulesCheck {
    let mut violations = Vec::new();

    // ── Rule 1: Scope-Lock ──
    if !valid_scopes.is_empty() {
        let in_scope = valid_scopes.iter().any(|scope| payload.contains(scope));
        if !in_scope {
            violations.push(RuleViolation {
                rule: "scope-lock".into(),
                description:
                    "Payload does not match any valid scope — out-of-scope invocation blocked."
                        .into(),
                severity: ViolationSeverity::Fatal,
            });
        }
    }

    // ── Rule 2: No force-push ──
    if FORCE_PUSH_REGEX.is_match(payload) {
        violations.push(RuleViolation {
            rule: "git-discipline: force-push".into(),
            description: "Force pushing to remote is strictly prohibited.".into(),
            severity: ViolationSeverity::Fatal,
        });
    }

    // ── Rule 3: No rm -rf / ──
    if RM_RF_REGEX.is_match(payload) {
        violations.push(RuleViolation {
            rule: "git-discipline: destructive-rm".into(),
            description: "Destructive recursive removal of root is prohibited.".into(),
            severity: ViolationSeverity::Fatal,
        });
    }

    // ── Rule 4: No vague TODOs ──
    if TODO_REGEX.is_match(payload) {
        violations.push(RuleViolation {
            rule: "no-slop: vague-todo".into(),
            description: "Vague TODO detected — replace with specific issue reference or remove."
                .into(),
            severity: ViolationSeverity::Fatal,
        });
    }

    // ── Rule 5: No narration comments ──
    if NARRATION_REGEX.is_match(payload) {
        violations.push(RuleViolation {
            rule: "no-slop: narration".into(),
            description: "AI narration comment detected — these add noise without value.".into(),
            severity: ViolationSeverity::Fatal,
        });
    }

    // ── Rule 6: No dead code patterns ──
    if DEAD_CODE_REGEX.is_match(payload) {
        violations.push(RuleViolation {
            rule: "no-slop: dead-code".into(),
            description: "Dead code pattern detected (if false, while false, unreachable).".into(),
            severity: ViolationSeverity::Fatal,
        });
    }

    let passed = violations
        .iter()
        .all(|v| v.severity != ViolationSeverity::Fatal);

    RulesCheck { passed, violations }
}

/// Validate that a file exists and can be read (Verify-First rule).
pub async fn verify_file_exists(path: &str) -> Result<bool, CoreError> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| CoreError::Io(e.to_string()))?;
    Ok(meta.is_file() && meta.len() > 0)
}

/// Check that a proposed commit message follows conventional commits.
pub fn check_conventional_commit(message: &str) -> RulesCheck {
    let conventional_regex = Regex::new(
        r"^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?!?: .+",
    )
    .unwrap();

    if conventional_regex.is_match(message.trim()) {
        RulesCheck {
            passed: true,
            violations: vec![],
        }
    } else {
        RulesCheck {
            passed: false,
            violations: vec![RuleViolation {
                rule: "git-discipline: conventional-commit".into(),
                description: "Commit message does not follow conventional commits format.".into(),
                severity: ViolationSeverity::Fatal,
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_lock_violation() {
        let check = enforce_deterministic_rules(
            "delete the database",
            &["read_file".into(), "write_test".into()],
        );
        assert!(!check.passed);
        assert!(check.violations.iter().any(|v| v.rule == "scope-lock"));
    }

    #[test]
    fn test_force_push_blocked() {
        let check =
            enforce_deterministic_rules("git push --force origin main", &["git push".into()]);
        assert!(!check.passed);
        assert!(
            check
                .violations
                .iter()
                .any(|v| v.rule.contains("force-push"))
        );
    }

    #[test]
    fn test_todo_detected() {
        let check = enforce_deterministic_rules("// TODO: fix this later", &["code".into()]);
        assert!(!check.passed);
    }

    #[test]
    fn test_clean_payload_passes() {
        let check = enforce_deterministic_rules(
            "fn add(a: i32, b: i32) -> i32 { a + b }",
            &["fn add".into()],
        );
        assert!(check.passed);
    }

    #[test]
    fn test_conventional_commit_valid() {
        let check = check_conventional_commit("feat(sandbox): add WASM execution backend");
        assert!(check.passed);
    }

    #[test]
    fn test_conventional_commit_invalid() {
        let check = check_conventional_commit("fixed the bug");
        assert!(!check.passed);
    }
}
