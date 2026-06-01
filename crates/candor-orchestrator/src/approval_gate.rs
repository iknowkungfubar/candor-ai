/// Human-in-the-loop approval gate for non-autonomous ISAs.
///
/// When an ISA has `fully_autonomous: false` or contains
/// `HumanConfirmation` acceptance criteria, the ApprovalGate
/// pauses before the Execute phase and prompts the operator.
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use candor_core::error::CoreError;
use candor_core::state::AgentState;
use candor_graph::hooks::BeforeExecuteConfirmation;

/// Prompts the user for approval before the Execute phase.
///
/// Checks the ISA's `fully_autonomous` flag and `awaiting_approval`
/// state. If the task requires human confirmation, blocks execution
/// until the operator approves via stdin.
pub struct ApprovalGate;

#[async_trait::async_trait]
impl BeforeExecuteConfirmation for ApprovalGate {
    async fn before_execute(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let (fully_autonomous, awaiting_approval, task) = {
            let s = state.lock().await;
            let autonomous = s
                .ideal_state
                .as_ref()
                .map(|isa| isa.fully_autonomous)
                .unwrap_or(true);
            (autonomous, s.awaiting_approval, s.active_task.clone())
        };

        // If the ISA is fully autonomous and no human confirmation criteria,
        // proceed without interruption.
        if fully_autonomous && !awaiting_approval {
            info!("ApprovalGate: ISA is fully autonomous — proceeding to Execute");
            return Ok(());
        }

        // Prompt the operator for approval
        let prompt = if awaiting_approval {
            format!(
                "\n╔══════════════════════════════════════════════════════════╗\n\
                 ║        🔴 HUMAN CONFIRMATION REQUIRED                    ║\n\
                 ╠══════════════════════════════════════════════════════════╣\n\
                 ║ The ISA for this task contains criteria that require     ║\n\
                 ║ explicit human approval before execution proceeds.       ║\n\
                 ╚══════════════════════════════════════════════════════════╝\n\n\
                 Task: {}\n\n\
                 Type 'yes' to approve, or anything else to abort: ",
                task
            )
        } else {
            format!(
                "\n╔══════════════════════════════════════════════════════════╗\n\
                 ║        🟡 HUMAN APPROVAL REQUIRED                        ║\n\
                 ╠══════════════════════════════════════════════════════════╣\n\
                 ║ The ISA for this task is not fully autonomous.          ║\n\
                 ║ Execution will pause until you approve.                  ║\n\
                 ╚══════════════════════════════════════════════════════════╝\n\n\
                 Task: {}\n\n\
                 Type 'yes' to approve, or anything else to abort: ",
                task
            )
        };

        // Print prompt and read response
        use std::io::{Write, stdin, stdout};
        print!("{}", prompt);
        let _ = stdout().flush();

        let mut input = String::new();
        match stdin().read_line(&mut input) {
            Ok(_) => {
                let trimmed = input.trim().to_lowercase();
                if trimmed == "yes" || trimmed == "y" {
                    info!("ApprovalGate: operator approved execution");
                    let mut s = state.lock().await;
                    s.awaiting_approval = false;
                    Ok(())
                } else {
                    tracing::error!("ApprovalGate: operator rejected execution");
                    Err(CoreError::HumanApprovalDenied)
                }
            }
            Err(e) => {
                let msg = format!("Failed to read operator input: {e}");
                tracing::error!("ApprovalGate: {msg}");
                Err(CoreError::Io(msg))
            }
        }
    }
}
