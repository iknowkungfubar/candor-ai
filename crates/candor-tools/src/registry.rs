use candor_core::error::CoreError;
/// The tool system — agent capabilities.
use std::sync::Arc;

/// Context passed to every tool execution.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Working directory for file operations.
    pub workdir: String,
    /// Project identifier for scoping.
    pub project_id: String,
}

/// Output from a tool execution.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Whether the tool succeeded.
    pub success: bool,
    /// Human-readable output.
    pub output: String,
    /// Structured data (optional).
    pub data: Option<serde_json::Value>,
    /// Error message if failed.
    pub error: Option<String>,
}

impl ToolOutput {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            data: None,
            error: None,
        }
    }

    pub fn ok_with_data(output: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            output: output.into(),
            data: Some(data),
            error: None,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: String::new(),
            data: None,
            error: Some(error.into()),
        }
    }
}

/// A tool that an agent can invoke.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Unique name for this tool (e.g., "read_file", "run_tests").
    fn name(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// Execute the tool with given arguments.
    async fn execute(&self, ctx: &ToolContext, args: &[String]) -> Result<ToolOutput, CoreError>;
}

/// Registry of all available tools.
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn find(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    pub fn list_all(&self) -> Vec<(String, String)> {
        self.tools
            .iter()
            .map(|t| (t.name().to_string(), t.description().to_string()))
            .collect()
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn descriptions_for_llm(&self) -> String {
        self.tools
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
