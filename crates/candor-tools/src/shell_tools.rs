/// Shell execution tool (runs through the sandbox).
use tracing::info;

use candor_core::error::CoreError;
use candor_sandbox::unified::{ExecLanguage, ToolSandbox};

use super::registry::{Tool, ToolContext, ToolOutput};

pub struct ShellTool {
    sandbox: ToolSandbox,
}

impl ShellTool {
    pub fn new(sandbox: ToolSandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait::async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "Execute a shell command in the sandbox. Args: <command>"
    }

    async fn execute(&self, _ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let command = args.join(" ");
        if command.is_empty() {
            return Err(CoreError::Internal(
                "shell requires a command argument".into(),
            ));
        }

        info!(command = %command, "Executing in sandbox");
        match self
            .sandbox
            .execute_tool(&command, ExecLanguage::Shell)
            .await
        {
            Ok(output) => Ok(ToolOutput::ok(output)),
            Err(e) => Ok(ToolOutput::err(format!("Command failed: {e}"))),
        }
    }
}
