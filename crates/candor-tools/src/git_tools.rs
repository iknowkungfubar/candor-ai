/// Git tools: branch, commit, push — all gated through the sentinel.
use std::process::Stdio;
use tracing::info;

use candor_core::error::CoreError;

use super::registry::{Tool, ToolContext, ToolOutput};

pub struct GitBranchTool;

#[async_trait::async_trait]
impl Tool for GitBranchTool {
    fn name(&self) -> &str {
        "git_branch"
    }
    fn description(&self) -> &str {
        "Create a new git branch. Args: <branch_name>"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let branch = args
            .first()
            .ok_or_else(|| CoreError::Internal("git_branch requires a branch name".into()))?;

        // Sentinel: block force-push patterns even in branch names
        if branch.contains("--force") || branch.contains("-f") {
            return Err(CoreError::SentinelPolicyViolation(
                "Branch name contains force flags — blocked by sentinel".into(),
            ));
        }

        info!(branch = %branch, "Creating git branch");
        let output = git(&ctx.workdir, &["checkout", "-b", branch]).await?;
        Ok(ToolOutput::ok(output))
    }
}

pub struct GitCommitTool;

#[async_trait::async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }
    fn description(&self) -> &str {
        "Stage all changes and commit. Args: <message>"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        let message = args.join(" ");
        if message.is_empty() {
            return Err(CoreError::Internal(
                "git_commit requires a commit message".into(),
            ));
        }

        // Sentinel: validate conventional commit format
        let conventional = regex::Regex::new(
            r"^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?!?: .+",
        )
        .unwrap();

        if !conventional.is_match(&message) {
            return Err(CoreError::SentinelPolicyViolation(
                "Commit message must follow conventional commits format".into(),
            ));
        }

        info!(message = %message, "Committing");

        git(&ctx.workdir, &["add", "."]).await?;
        git(&ctx.workdir, &["commit", "-m", &message]).await?;

        Ok(ToolOutput::ok(format!("Committed: {message}")))
    }
}

pub struct GitPushTool;

#[async_trait::async_trait]
impl Tool for GitPushTool {
    fn name(&self) -> &str {
        "git_push"
    }
    fn description(&self) -> &str {
        "Push current branch to remote. Args: [--force] — blocked by sentinel"
    }

    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError> {
        // Sentinel: block force push
        if args.contains(&"--force".to_string()) || args.contains(&"-f".to_string()) {
            return Err(CoreError::SentinelPolicyViolation(
                "Force push is strictly prohibited by sentinel Git-Discipline rule".into(),
            ));
        }

        info!("Pushing to remote");
        let mut cmd_args = vec!["push"];
        cmd_args.extend(args.iter().map(|s| s.as_str()));
        let output = git(&ctx.workdir, &cmd_args).await?;
        Ok(ToolOutput::ok(output))
    }
}

pub struct GitStatusTool;

#[async_trait::async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }
    fn description(&self) -> &str {
        "Show git working tree status. No args."
    }

    async fn execute(&self, ctx: &ToolContext, _args: &[String]) -> Result<ToolOutput, CoreError> {
        let output = git(&ctx.workdir, &["status", "--short"]).await?;
        if output.trim().is_empty() {
            Ok(ToolOutput::ok("Working tree clean."))
        } else {
            Ok(ToolOutput::ok(output))
        }
    }
}

/// Run a git command in the given directory.
async fn git(workdir: &str, args: &[&str]) -> Result<String, CoreError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| CoreError::Internal(format!("Failed to run git: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        Err(CoreError::Internal(format!("Git command failed: {stderr}")))
    } else {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    }
}
