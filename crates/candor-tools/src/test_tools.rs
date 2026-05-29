/// Test execution tool.
use std::process::Stdio;
use tracing::info;

use candor_core::error::CoreError;

use super::registry::{Tool, ToolContext, ToolOutput};

pub struct RunTestsTool;

#[async_trait::async_trait]
impl Tool for RunTestsTool {
    fn name(&self) -> &str { "run_tests" }
    fn description(&self) -> &str {
        "Run the project's test suite. Args: [test_filter] [--no-fail-fast]"
    }

    async fn execute(
        &self,
        ctx: &ToolContext,
        args: &[String],
    ) -> Result<ToolOutput, CoreError> {
        let test_filter = args.first().cloned();
        let fail_fast = !args.contains(&"--no-fail-fast".to_string());

        info!("Running test suite");

        let mut cmd = tokio::process::Command::new("cargo");
        cmd.arg("test");
        cmd.current_dir(&ctx.workdir);

        if !fail_fast {
            cmd.arg("--no-fail-fast");
        }
        if let Some(filter) = &test_filter
            && !filter.starts_with("--") {
                cmd.arg(filter);
            }

        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                CoreError::Internal(format!("Failed to run tests: {e}"))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let summary = if output.status.success() {
            // Extract test summary
            format!(
                "Tests passed.\n{}",
                extract_summary(&stdout)
            )
        } else {
            format!(
                "Tests FAILED.\nStdout:\n{}\nStderr:\n{}",
                extract_summary(&stdout),
                stderr
            )
        };

        let data = serde_json::json!({
            "passed": output.status.success(),
            "exit_code": output.status.code(),
        });

        Ok(ToolOutput::ok_with_data(summary, data))
    }
}

fn extract_summary(output: &str) -> String {
    // Extract the last meaningful lines from cargo test output.
    let lines: Vec<&str> = output.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("test result:"))
        .unwrap_or(lines.len().saturating_sub(5));

    lines[start..]
        .iter()
        .take(10)
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}
