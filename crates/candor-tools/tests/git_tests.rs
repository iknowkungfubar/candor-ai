/// Tests for git tools — branch, commit, push, status.
use candor_tools::registry::{Tool, ToolContext};
use candor_tools::{GitBranchTool, GitCommitTool, GitPushTool, GitStatusTool};
fn make_ctx() -> ToolContext {
    ToolContext {
        workdir: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        project_id: "test".into(),
    }
}

#[tokio::test]
async fn test_git_status_tool() {
    let tool = GitStatusTool;
    let ctx = make_ctx();
    // May fail if not in a git repo — that's OK, just verify no panic
    let _ = tool.execute(&ctx, &[]).await;
}

#[tokio::test]
async fn test_git_branch_tool_no_args() {
    let tool = GitBranchTool;
    let ctx = make_ctx();
    let result = tool.execute(&ctx, &[]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_git_branch_blocks_force() {
    let tool = GitBranchTool;
    let ctx = make_ctx();
    let result = tool.execute(&ctx, &["--force".into()]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("blocked"));
}

#[tokio::test]
async fn test_git_commit_no_args() {
    let tool = GitCommitTool;
    let ctx = make_ctx();
    let result = tool.execute(&ctx, &[]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_git_commit_non_conventional() {
    let tool = GitCommitTool;
    let ctx = make_ctx();
    let result = tool.execute(&ctx, &["fixed the bug".into()]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("conventional"));
}

#[tokio::test]
async fn test_git_push_blocks_force() {
    let tool = GitPushTool;
    let ctx = make_ctx();
    let result = tool.execute(&ctx, &["--force".into()]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_git_push_blocks_short_force() {
    let tool = GitPushTool;
    let ctx = make_ctx();
    let result = tool.execute(&ctx, &["-f".into()]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_tool_names_and_descriptions() {
    assert_eq!(GitBranchTool.name(), "git_branch");
    assert_eq!(GitCommitTool.name(), "git_commit");
    assert_eq!(GitPushTool.name(), "git_push");
    assert_eq!(GitStatusTool.name(), "git_status");
    assert!(!GitBranchTool.description().is_empty());
    assert!(!GitCommitTool.description().is_empty());
    assert!(!GitPushTool.description().is_empty());
}
