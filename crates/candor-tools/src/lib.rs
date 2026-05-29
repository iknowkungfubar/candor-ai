// candor-tools: the agent's capability system.
// Tools are what the agent uses during the 7-phase execution.

pub mod registry;
pub mod fs_tools;
pub mod shell_tools;
pub mod search_tools;
pub mod test_tools;
pub mod git_tools;

pub use registry::{Tool, ToolContext, ToolOutput, ToolRegistry};
pub use fs_tools::{ReadFileTool, WriteFileTool, ListDirTool};
pub use shell_tools::ShellTool;
pub use search_tools::{SearchCodeTool, SearchFilesTool};
pub use test_tools::RunTestsTool;
pub use git_tools::{
    GitBranchTool, GitCommitTool, GitPushTool, GitStatusTool,
};
