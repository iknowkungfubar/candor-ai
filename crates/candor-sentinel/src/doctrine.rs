/// Operational Doctrine — the 10 Lawful Good principles as runtime guardrails.
///
/// From the design doc's "Operating Doctrine and Lawful Good Philosophy":
/// These principles are encoded as checkable rules that the Sentinel enforces
/// at runtime, not just documentation.
use candor_core::protocol::AgentAction;

/// Result of a doctrine check.
#[derive(Debug, Clone)]
pub struct DoctrineCheck {
    pub passed: bool,
    pub violations: Vec<String>,
    pub warnings: Vec<String>,
}

/// Encode all 10 operational guidelines as runtime guardrails.
pub fn enforce_doctrine(action: &AgentAction, context: &str) -> DoctrineCheck {
    let mut violations = Vec::new();
    let mut warnings = Vec::new();

    // 1. Precision Over Persuasion — claims must survive adversarial reading
    if contains_vague_claim(context) {
        violations.push(
            "Precision Over Persuasion: Vague claim detected. Claims must be specific and verifiable."
                .into(),
        );
    }

    // 2. Systems Before Tools — tools are replaceable, architecture is not
    // (Enforced structurally by the crate architecture — not a runtime check)

    // 3. AI Is Infrastructure, Not Authority — hard constraints on real-world actions
    if action.is_destructive() && !action.sentinel_approved {
        violations.push(
            "AI Is Infrastructure, Not Authority: Destructive action requires sentinel approval."
                .into(),
        );
    }

    // 4. Failure Is the Primary Use Case — design for failure first
    if !action.is_reversible && matches!(action.action_type, candor_core::protocol::ActionType::FileWrite | candor_core::protocol::ActionType::FileDelete) {
        warnings.push(
            "Failure Is the Primary Use Case: Irreversible action — ensure rollback is possible."
                .into(),
        );
    }

    // 5. Marketing Is Not Evidence — claims require implementation proof
    if contains_marketing_language(context) {
        violations.push(
            "Marketing Is Not Evidence: Remove marketing language. Claims require sandbox-verified proof."
                .into(),
        );
    }

    // 6. Autonomy Requires Control — control over data, compute, execution
    // (Enforced by local-first execution and sandbox denial-by-default)

    // 7. Sustainability Is a Hard Constraint — human limits are design inputs
    if action.payload.len() > 100_000 {
        violations.push(
            "Sustainability Is a Hard Constraint: Payload exceeds cognitive limit. Break into smaller steps."
                .into(),
        );
    }

    // 8. Simplicity Is an Ethical Choice — visible, observable logic
    if contains_over_abstraction(context) {
        violations.push(
            "Simplicity Is an Ethical Choice: Over-abstraction detected. Use visible, simple logic."
                .into(),
        );
    }

    // 9. Prevention Is the Highest Form of Competence — prevent failure
    // (Enforced by sentinel deterministic rules — force-push, rm -rf, etc.)

    // 10. Reversibility Matters More Than Speed — undo is cheap
    if matches!(action.action_type, candor_core::protocol::ActionType::ForcePush) {
        violations.push(
            "Reversibility Matters More Than Speed: Force push is irreversible. Blocked."
                .into(),
        );
    }

    let passed = violations.is_empty();
    DoctrineCheck { passed, violations, warnings }
}

fn contains_vague_claim(text: &str) -> bool {
    let vague_patterns = [
        "should work",
        "probably fine",
        "might be okay",
        "seems to work",
        "looks good",
        "I think",
        "I believe",
    ];
    let lower = text.to_lowercase();
    vague_patterns.iter().any(|p| lower.contains(p))
}

fn contains_marketing_language(text: &str) -> bool {
    let marketing_patterns = [
        "revolutionary",
        "game-changing",
        "best-in-class",
        "world-class",
        "cutting-edge",
        "state-of-the-art",
        "unprecedented",
    ];
    let lower = text.to_lowercase();
    marketing_patterns.iter().any(|p| lower.contains(p))
}

fn contains_over_abstraction(text: &str) -> bool {
    let abstraction_patterns = [
        "AbstractSingletonProxyFactoryBean",
        "EnterpriseQuality",
        "ManagerFactory",
        "ProviderManager",
    ];
    text.contains("FactoryFactory") || abstraction_patterns.iter().any(|p| text.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vague_claim_detected() {
        assert!(contains_vague_claim("This should work fine"));
        assert!(!contains_vague_claim("The function returns Ok(()) when given valid input"));
    }

    #[test]
    fn test_marketing_language_detected() {
        assert!(contains_marketing_language("This revolutionary approach"));
        assert!(!contains_marketing_language("The test passes with valid inputs"));
    }

    #[test]
    fn test_doctrine_check_destructive() {
        let action = AgentAction {
            id: "1".into(),
            action_type: candor_core::protocol::ActionType::FileDelete,
            payload: "rm important".into(),
            target_path: Some("/tmp/test".into()),
            is_reversible: false,
            scope_tags: vec![],
            phase: "execute".into(),
            sentinel_approved: false,
        };

        let check = enforce_doctrine(&action, "delete this file");
        assert!(!check.passed);
        assert!(check.warnings.iter().any(|w| w.contains("Failure Is the Primary Use Case")));
    }
}
