/// Search tools: code search (grep) and file name search.
use std::process::Stdio;
use tracing::info;

use candor_core::error::CoreError;

use super::registry::{Tool, ToolContext, ToolOutput};

pub struct SearchCodeTool;

#[async_trait::async_trait]
impl Tool for SearchCodeTool {
    fn name(&self) -> &str {
        "search_code"
    }
    fn description(&self) -> &str {
        "Search for a pattern in source files using ripgrep. Args: <pattern> [file_glob]"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let pattern = args
            .first()
            .ok_or_else(|| CoreError::Internal("search_code requires a pattern argument".into()))?;
        let file_glob = args.get(1).map(|s| s.as_str()).unwrap_or("*.rs");

        info!(pattern = %pattern, "Searching code");

        let output = tokio::process::Command::new("rg")
            .arg("--line-number")
            .arg("--max-count=50")
            .arg("-g")
            .arg(file_glob)
            .arg(pattern)
            .current_dir(&ctx.workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.trim().is_empty() {
                    Ok(ToolOutput::ok("No matches found."))
                } else {
                    Ok(ToolOutput::ok(stdout.to_string()))
                }
            }
            Err(e) => {
                // rg not installed or other error
                Ok(ToolOutput::err(format!(
                    "Search failed (is ripgrep installed?): {e}"
                )))
            }
        }
    }
}

pub struct SearchFilesTool;

#[async_trait::async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }
    fn description(&self) -> &str {
        "Find files by name pattern. Args: <glob_pattern>"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let pattern = args.first().ok_or_else(|| {
            CoreError::Internal("search_files requires a glob pattern argument".into())
        })?;

        info!(pattern = %pattern, "Finding files");

        let output = tokio::process::Command::new("find")
            .arg(&ctx.workdir)
            .arg("-name")
            .arg(pattern)
            .arg("-not")
            .arg("-path")
            .arg("*/target/*")
            .arg("-not")
            .arg("-path")
            .arg("*/.git/*")
            .arg("-type")
            .arg("f")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
            .unwrap_or_default();

        if output.trim().is_empty() {
            Ok(ToolOutput::ok("No files found."))
        } else {
            Ok(ToolOutput::ok(output))
        }
    }
}
