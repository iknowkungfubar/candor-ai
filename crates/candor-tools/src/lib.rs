// candor-tools: the agent's capability system.
// Tools are what the agent uses during the 7-phase execution.

pub mod fs_tools;
pub mod git_tools;
pub mod registry;
pub mod search_tools;
pub mod shell_tools;
pub mod test_tools;

pub use fs_tools::{ListDirTool, ReadFileTool, WriteFileTool};
pub use git_tools::{GitBranchTool, GitCommitTool, GitPushTool, GitStatusTool};
pub use registry::{Tool, ToolContext, ToolOutput, ToolRegistry};
pub use search_tools::{SearchCodeTool, SearchFilesTool};
pub use shell_tools::ShellTool;
pub use test_tools::RunTestsTool;
