use std::collections::HashMap;
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
    pub(super) async fn observe(&self, ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let mut s = state.lock().await;
        s.log_event("Observe: scanning project");

        // List project structure
        if let Some(tool) = self.tools.find("list_dir")
            && let Ok(out) = tool.execute(ctx, &[]).await
        {
            s.append_message(&format!("Project files:\n{}", out.output));
        }
        // Find key files
        if let Some(tool) = self.tools.find("search_files")
            && let Ok(out) = tool.execute(ctx, &["*.rs".to_string()]).await
        {
            s.append_message(&format!("\nRust sources:\n{}", out.output));
        }
        // Read Cargo.toml
        if let Some(tool) = self.tools.find("read_file")
            && let Ok(out) = tool.execute(ctx, &["Cargo.toml".into()]).await
        {
            s.append_message(&format!("\nCargo.toml:\n{}", out.output));
        }
        s.log_event("Observe: complete");
        Ok(())
    }

    pub(super) async fn think(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let context = {
            let s = state.lock().await;
            s.message_history
                .iter()
                .rev()
                .take(15)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        };

        let base = format!(
            "You are a software engineering agent. Analyze the project context and identify what needs to be done.\n\nContext:\n{context}\n\nOutput a brief, specific analysis.",
        );
        let prompt = self.build_prompt_with_context(&base);

        match self.cognitive.generate_fast(&prompt).await {
            Ok(analysis) => {
                let mut s = state.lock().await;
                s.append_message(&format!("Analysis: {analysis}"));
                s.log_event("Think: complete");
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.log_event(&format!("Think: LLM unavailable ({e})"));
            }
        }
        Ok(())
    }

    pub(super) async fn plan(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let context = {
            let s = state.lock().await;
            s.message_history
                .iter()
                .rev()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        };

        let tools_desc = self.tools.descriptions_for_llm();
        let base = format!(
            "You are a software engineering agent. Generate a numbered plan.\n\nAvailable tools:\n{tools_desc}\n\nContext:\n{context}\n\nOutput numbered, actionable steps.",
        );
        let prompt = self.build_prompt_with_context(&base);

        match self.cognitive.generate_fast(&prompt).await {
            Ok(plan) => {
                let mut s = state.lock().await;
                s.append_message(&format!("Plan:\n{plan}"));
                s.log_event("Plan: complete");
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.append_message("Plan: 1) Implement changes 2) Run tests 3) Commit");
                s.log_event(&format!("Plan: fallback ({e})"));
            }
        }
        Ok(())
    }

    pub(super) async fn build(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let context = {
            let s = state.lock().await;
            s.message_history
                .iter()
                .rev()
                .take(25)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        };

        let base = format!(
            "You are a software engineering agent. Write code changes.\n\nContext:\n{context}\n\nFormat each file as:\n### FILE: path\n```\ncode\n```\n\nWrite COMPLETE, compilable code files.",
        );
        let prompt = self.build_prompt_with_context(&base);

        match self.cognitive.generate_fast(&prompt).await {
            Ok(code) => {
                let written = super::write_code_files(&code, &self.workdir).await;
                let mut s = state.lock().await;
                s.append_message(&format!("Generated code ({written} files):\n{code}"));
                s.log_event(&format!("Build: {written} files written"));
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.log_event(&format!("Build: LLM unavailable ({e})"));
            }
        }
        Ok(())
    }

    pub(super) async fn exec(&self, ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        // ── ISA validation gate (Build->Execute transition) ──
        // The Ideal State Artifact must define acceptance criteria before
        // execution can proceed. This enforces the design contract that
        // every task has measurable success criteria.
        let isa = {
            let s = state.lock().await;
            s.ideal_state.clone()
        };

        if let Some(ref isa) = isa {
            Self::validate_isa_for_execution(isa, &state).await?;
        } else {
            return Err(CoreError::IdealStateNotSatisfied(
                "No Ideal State Artifact set -- run_task was called without an ISA".into(),
            ));
        }

        let mut s = state.lock().await;
        s.log_event("Execute: running cargo check");

        if let Some(tool) = self.tools.find("shell") {
            match tool.execute(ctx, &["cargo check 2>&1".into()]).await {
                Ok(out) => {
                    s.append_message(&format!("Build:\n{}", out.output));
                    s.log_event("Execute: cargo check complete");
                }
                Err(e) => s.log_event(&format!("Execute: failed ({e})")),
            }
        }
        Ok(())
    }

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

    pub(super) async fn verify(&self, ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let mut s = state.lock().await;
        s.log_event("Verify: starting ISA criterion verification");
        let isa = s.ideal_state.clone();
        drop(s);

        let isa = match isa {
            Some(ref isa) => isa.clone(),
            None => {
                state
                    .lock()
                    .await
                    .log_event("Verify: no ISA set -- skipping verification");
                return Ok(());
            }
        };

        // -- Phase 1: Run tests as a general health check --
        let _test_output = if let Some(tool) = self.tools.find("run_tests") {
            match tool.execute(ctx, &[]).await {
                Ok(out) => {
                    let mut s = state.lock().await;
                    s.append_message(&format!("Test results:\n{}", out.output));
                    s.log_event(if out.success {
                        "Verify: tests PASSED"
                    } else {
                        "Verify: tests FAILED"
                    });
                    Some(out.success)
                }
                Err(e) => {
                    state.lock().await.log_event(&format!("Verify: test error ({e})"));
                    None
                }
            }
        } else {
            None
        };

        // -- Phase 2: Verify each acceptance criterion --
        let mut results: HashMap<String, bool> = HashMap::new();
        let mut summary = String::from("## ISA Verification Results\n\n");

        for criterion in &isa.acceptance_criteria {
            let passed = self.verify_criterion(criterion, ctx, &state).await;
            results.insert(criterion.id.clone(), passed);

            let status = if passed { "PASS" } else { "FAIL" };
            let label = match &criterion.verification_method {
                VerificationMethod::ShellCommand { .. } => "shell",
                VerificationMethod::TestCase { .. } => "test",
                VerificationMethod::FileExists { .. } => "file",
                VerificationMethod::FileMatches { .. } => "file-matches",
                VerificationMethod::LintCheck { .. } => "lint",
                VerificationMethod::HumanConfirmation { .. } => "human",
            };
            summary.push_str(&format!(
                "- **{}**: {} -- {} ({})\n",
                criterion.id, status, criterion.description, label,
            ));
            state
                .lock()
                .await
                .log_event(&format!("Verify: criterion '{}': {}", criterion.id, status));
        }

        // -- Phase 3: Check for required human confirmation criteria --
        let has_human_criteria = isa
            .acceptance_criteria
            .iter()
            .any(|c| matches!(c.verification_method, VerificationMethod::HumanConfirmation { .. }));

        if has_human_criteria {
            summary.push_str("\nHuman confirmation required for some criteria.\n");
            {
                let mut s = state.lock().await;
                s.awaiting_approval = true;
            }
        }

        {
            let mut s = state.lock().await;
            s.verification_results = results.clone();
            s.append_message(&summary);
        }

        // -- Phase 4: Determine if all criteria passed --
        let all_passed = results.values().all(|&v| v);
        if all_passed {
            state.lock().await.log_event("Verify: ALL CRITERIA PASSED");
        } else {
            let failed: Vec<_> = results.iter().filter(|&(_, &v)| !v).map(|(id, _)| id.clone()).collect();
            let msg = format!("Verify: criteria FAILED: [{}]", failed.join(", "));
            state.lock().await.log_event(&msg);
        }

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
    pub(super) async fn verify_shell_command(&self, command: &str, ctx: &ToolContext, _state: &Arc<Mutex<AgentState>>) -> bool {
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
    pub(super) async fn verify_test_case(&self, test_name: &str, ctx: &ToolContext, _state: &Arc<Mutex<AgentState>>) -> bool {
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
    pub(super) async fn verify_file_exists(&self, path: &str, _state: &Arc<Mutex<AgentState>>) -> bool {
        if path.trim().is_empty() {
            return false;
        }
        tokio::fs::metadata(path).await.is_ok()
    }

    /// Verify a file content matches a pattern criterion.
    pub(super) async fn verify_file_matches(&self, path: &str, pattern: &str, _state: &Arc<Mutex<AgentState>>) -> bool {
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

    pub(super) async fn learn(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let (task, pid, log) = {
            let s = state.lock().await;
            (s.active_task.clone(), s.project_id.clone(), s.execution_log.clone())
        };

        // Generate session summary
        let mut summary = format!("# Session\n**Task:** {task}\n\n## Events\n");
        for e in log.iter().rev().take(30) {
            summary.push_str(&format!("- {e}\n"));
        }

        if let Some(ref pid) = pid {
            let emb = vec![0.0_f32; 384];
            let _ = self.memory.store_memory(pid.clone(), summary.clone(), emb).await;
            info!(pid = %pid, "Learn: stored");
        }

        let mut s = state.lock().await;
        s.log_event(&format!("Learn: {} events archived", log.len()));
        Ok(())
    }
}
