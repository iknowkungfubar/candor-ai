use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use candor_core::error::CoreError;
use candor_core::ideal::AcceptanceCriterion;
use candor_core::ideal::VerificationMethod;
use candor_core::state::AgentState;
use candor_tools::registry::ToolContext;

use super::PhaseContext;

impl PhaseContext {
    /// Validate the Ideal State Artifact before executing.
    ///
    /// Checks:
    /// 1. At least one acceptance criterion is defined
    /// 2. Each criterion has a non-empty, well-formed verification method
    /// 3. The active task aligns with the ISA's goal
    pub(super) async fn validate_isa_for_execution(
        isa: &candor_core::ideal::IdealStateArtifact,
        state: &Arc<Mutex<AgentState>>,
    ) -> Result<(), CoreError> {
        let active_task = state.lock().await.active_task.clone();

        // Check 1: at least one acceptance criterion
        if isa.acceptance_criteria.is_empty() {
            return Err(CoreError::IdealStateNotSatisfied(format!(
                "ISA '{}' has no acceptance criteria defined. \
                     Goal: '{}'. Task: '{}'. At least one acceptance criterion \
                     is required before the Build->Execute transition.",
                isa.id, isa.goal, active_task,
            )));
        }

        // Check 2: each criterion must have a valid verification method
        let mut invalid_criteria = Vec::new();
        for criterion in &isa.acceptance_criteria {
            let is_valid = match &criterion.verification_method {
                VerificationMethod::ShellCommand { command } | VerificationMethod::LintCheck { command } => {
                    !command.trim().is_empty()
                }
                VerificationMethod::TestCase { test_name } => !test_name.trim().is_empty(),
                VerificationMethod::FileExists { path } => !path.trim().is_empty(),
                VerificationMethod::FileMatches { path, pattern } => {
                    !path.trim().is_empty() && !pattern.trim().is_empty()
                }
                VerificationMethod::HumanConfirmation { prompt } => !prompt.trim().is_empty(),
            };
            if !is_valid {
                invalid_criteria.push(criterion.id.clone());
            }
        }

        if !invalid_criteria.is_empty() {
            return Err(CoreError::IdealStateNotSatisfied(format!(
                "ISA '{}' has criteria with invalid/empty verification methods: [{}]. \
                     Each acceptance criterion must specify a verification method with \
                     a non-empty value.",
                isa.id,
                invalid_criteria.join(", "),
            )));
        }

        // Check 3: active task should reference the ISA's goal
        let goal_lower = isa.goal.to_lowercase();
        let task_lower = active_task.to_lowercase();
        if !task_lower.contains(&goal_lower) && !goal_lower.contains(&task_lower) {
            return Err(CoreError::IdealStateNotSatisfied(format!(
                "Active task does not align with ISA goal. \
                     Task: '{}'. ISA Goal: '{}'.",
                active_task, isa.goal,
            )));
        }

        info!(
            isa_id = %isa.id,
            criteria = isa.acceptance_criteria.len(),
            "ISA validation passed -- proceeding to Execute phase"
        );
        Ok(())
    }

    /// Verify a single acceptance criterion against its verification method.
    pub(super) async fn verify_criterion(
        &self,
        criterion: &AcceptanceCriterion,
        ctx: &ToolContext,
        state: &Arc<Mutex<AgentState>>,
    ) -> bool {
        match &criterion.verification_method {
            VerificationMethod::ShellCommand { command } => self.verify_shell_command(command, ctx, state).await,
            VerificationMethod::TestCase { test_name } => self.verify_test_case(test_name, ctx, state).await,
            VerificationMethod::FileExists { path } => self.verify_file_exists(path, state).await,
            VerificationMethod::FileMatches { path, pattern } => self.verify_file_matches(path, pattern, state).await,
            VerificationMethod::LintCheck { command } => self.verify_shell_command(command, ctx, state).await,
            VerificationMethod::HumanConfirmation { prompt } => {
                // Human confirmation criteria are verified via the BeforeExecuteConfirmation hook.
                // In the Verify phase, we mark them as requiring approval.
                state.lock().await.log_event(&format!(
                    "Verify: criterion '{}' requires human confirmation: {}",
                    criterion.id, prompt
                ));
                // Don't pass/fail here -- the ApprovalGate hook handles this
                true
            }
        }
    }

    /// Verify a shell command criterion: run the command and check exit code.
    async fn verify_shell_command(&self, command: &str, ctx: &ToolContext, _state: &Arc<Mutex<AgentState>>) -> bool {
        if command.trim().is_empty() {
            return false;
        }
        // Try using the shell tool first
        if let Some(tool) = self.tools.find("shell") {
            match tool.execute(ctx, &[command.to_string()]).await {
                Ok(out) => out.success,
                Err(_) => {
                    // Fallback: run directly
                    let output = tokio::process::Command::new("sh").arg("-c").arg(command).output().await;
                    matches!(output, Ok(o) if o.status.success())
                }
            }
        } else {
            // No shell tool -- run directly
            let output = tokio::process::Command::new("sh").arg("-c").arg(command).output().await;
            matches!(output, Ok(o) if o.status.success())
        }
    }

    /// Verify a test case criterion: run a specific test.
    async fn verify_test_case(&self, test_name: &str, ctx: &ToolContext, _state: &Arc<Mutex<AgentState>>) -> bool {
        if let Some(tool) = self.tools.find("run_tests") {
            let args = if test_name.trim().is_empty() {
                vec![]
            } else {
                vec![test_name.to_string()]
            };
            match tool.execute(ctx, &args).await {
                Ok(out) => out.success,
                Err(_) => false,
            }
        } else {
            // Fallback: run cargo test directly
            let mut cmd = tokio::process::Command::new("cargo");
            cmd.arg("test");
            if !test_name.trim().is_empty() {
                cmd.arg("--").arg(test_name);
            }
            let output = cmd.output().await;
            matches!(output, Ok(o) if o.status.success())
        }
    }

    /// Verify a file existence criterion.
    async fn verify_file_exists(&self, path: &str, _state: &Arc<Mutex<AgentState>>) -> bool {
        if path.trim().is_empty() {
            return false;
        }
        tokio::fs::metadata(path).await.is_ok()
    }

    /// Verify a file content matches a pattern criterion.
    async fn verify_file_matches(&self, path: &str, pattern: &str, _state: &Arc<Mutex<AgentState>>) -> bool {
        if path.trim().is_empty() || pattern.trim().is_empty() {
            return false;
        }
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return false,
        };
        // Simple substring check -- sufficient for ISA verification patterns
        content.contains(pattern)
    }
}
