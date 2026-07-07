use std::path::PathBuf;
/// The complete Candor Agent -- fully LLM-driven 7-phase SWE agent.
///
/// Binds all subsystems: tools, LLM, sandbox, sentinel, memory, git.
/// Each phase does real work using the cognitive engine and tool registry.
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use uuid::Uuid;

use candor_cognitive::CognitiveEngine;
use candor_core::error::CoreError;
use candor_core::ideal::IdealStateArtifact;
use candor_core::state::AgentState;
use candor_graph::hooks::LifecycleHooks;
use candor_graph::node::AgentNode as GraphNode;
use candor_graph::recovery::{RecoveryNode, analyze_error};
use candor_graph::runner::GraphRunner;
use candor_memory::store::MemorySystem;
use candor_sandbox::unified::ToolSandbox;
use candor_tools::registry::{ToolContext, ToolRegistry};
use candor_tools::{
    GitBranchTool, GitCommitTool, GitPushTool, GitStatusTool, ListDirTool, ReadFileTool, RunTestsTool, SearchCodeTool,
    SearchFilesTool, ShellTool, WriteFileTool,
};

use super::markdown_router::MarkdownContext;
use super::phases::Phase;

pub mod phases;

const COMPACTION_CHARS: usize = 8192;

// ── Phase context (immutable during graph run) ──

struct PhaseContext {
    phase: Phase,
    cognitive: Arc<CognitiveEngine>,
    memory: Arc<MemorySystem>,
    tools: Arc<ToolRegistry>,
    workdir: String,
    markdown_ctx: Option<MarkdownContext>,
}

/// The Candor Agent — full LLM-driven SWE agent.
pub struct OrchestratorEngine {
    pub graph_runner: GraphRunner,
    pub sandbox: ToolSandbox,
    pub cognitive: Arc<CognitiveEngine>,
    pub memory: Arc<MemorySystem>,
    pub sentinel: candor_sentinel::interceptor::SentinelInterceptor,
    pub session_id: String,
    pub tools: Arc<ToolRegistry>,
    markdown_ctx: Option<MarkdownContext>,
}

impl OrchestratorEngine {
    pub async fn new(
        cognitive: Arc<CognitiveEngine>,
        memory: Arc<MemorySystem>,
        max_iterations: u32,
    ) -> Result<Self, CoreError> {
        info!("Initializing Candor Agent");

        let sandbox = ToolSandbox::new().map_err(|e| CoreError::Internal(format!("Sandbox: {e}")))?;

        let sentinel = candor_sentinel::interceptor::SentinelInterceptor::new(Arc::clone(&cognitive), vec![]);

        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(ReadFileTool));
        tools.register(Arc::new(WriteFileTool));
        tools.register(Arc::new(ListDirTool));
        tools.register(Arc::new(SearchCodeTool));
        tools.register(Arc::new(SearchFilesTool));
        tools.register(Arc::new(ShellTool::new(ToolSandbox::new()?)));
        tools.register(Arc::new(RunTestsTool));
        tools.register(Arc::new(GitBranchTool));
        tools.register(Arc::new(GitCommitTool));
        tools.register(Arc::new(GitPushTool));
        tools.register(Arc::new(GitStatusTool));
        let tools_arc = Arc::new(tools);

        let hooks = LifecycleHooks::default()
            .with_before_tool(sentinel.clone_box())
            .with_before_execute(Box::new(crate::approval_gate::ApprovalGate));
        let graph_runner = GraphRunner::new(max_iterations).with_hooks(hooks);
        let session_id = Uuid::new_v4().to_string();

        info!(session_id = %session_id, tools = tools_arc.tool_count(), "Agent ready");

        Ok(Self {
            graph_runner,
            sandbox,
            cognitive,
            memory,
            sentinel,
            session_id,
            tools: tools_arc,
            markdown_ctx: None,
        })
    }

    pub async fn run_task(
        &mut self,
        task: &str,
        isa: &IdealStateArtifact,
        markdown_ctx: Option<MarkdownContext>,
    ) -> Result<(), CoreError> {
        info!(task = %task, has_markdown_ctx = markdown_ctx.is_some(), "Starting agent task");
        self.markdown_ctx = markdown_ctx;
        {
            let state_arc = self.graph_runner.state();
            let mut s = state_arc.lock().await;
            s.active_task = task.to_string();
            s.project_id = Some(isa.id.clone());
            s.ideal_state = Some(isa.clone());
            s.log_event(&format!("Task: {task}"));

            // Inject PDA identity into the agent's context if available.
            if let Ok(identity) = std::fs::read_to_string(dirs_or_default().join("IDENTITY.md")) {
                s.append_message(&format!("## User Identity\n\n{}", identity));
            }
            if let Ok(da_identity) = std::fs::read_to_string(dirs_or_default().join("DA_IDENTITY.md")) {
                s.append_message(&format!("## Assistant Identity\n\n{}", da_identity));
            }

            // Inject prior learnings from PDA file-based memory into context.
            let learning_dir = dirs_or_default().join("MEMORY").join("LEARNING");
            if learning_dir.is_dir() {
                let mut entries: Vec<String> = Vec::new();
                if let Ok(read_dir) = std::fs::read_dir(&learning_dir) {
                    for entry in read_dir.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("md")
                            && let Ok(content) = std::fs::read_to_string(&path)
                        {
                            entries.push(content);
                        }
                    }
                }
                if !entries.is_empty() {
                    s.append_message(&format!("## Prior Learnings\n\n{}", entries.join("\n\n---\n\n")));
                }
            }
        }

        let start = self.build_graph()?;

        // ── Recovery loop around graph execution ──
        // Wraps execute_graph() with analyze_error -> RecoveryNode retry logic.
        // Retryable errors are looped back after RecoveryNode resets state.
        // Non-retryable errors are escalated as CoreError::GraphExecution.
        let recovery = RecoveryNode::new("phase-recovery", 3);
        let result = loop {
            let graph_result = self.graph_runner.execute_graph(start).await;

            match graph_result {
                Ok(()) => break Ok(()),
                Err(e) => {
                    let strategy = analyze_error(&e);
                    info!(
                        reason = %strategy.reason,
                        retry = strategy.retry,
                        escalate = strategy.escalate,
                        "Graph execution error analyzed"
                    );

                    if strategy.retry {
                        // Attempt recovery via RecoveryNode
                        let state = self.graph_runner.state();
                        match recovery.execute(state).await {
                            Ok(()) => {
                                info!("Recovery applied -- retrying phase");
                                // Reset the graph runner's iteration counter
                                // slightly so the retried run has room.
                                {
                                    let state = self.graph_runner.state();
                                    let mut s = state.lock().await;
                                    if s.iteration_count > 0 {
                                        s.iteration_count = s.iteration_count.saturating_sub(1);
                                    }
                                }
                                continue;
                            }
                            Err(recovery_err) => {
                                // Recovery node exhausted its retries
                                let msg =
                                    format!("Recovery exhausted ({}): {} -- {}", strategy.reason, recovery_err, e,);
                                error!("{}", msg);
                                break Err(CoreError::GraphExecution(msg));
                            }
                        }
                    } else {
                        // Escalate -- this error cannot be recovered from
                        let msg = format!("Escalated: {} -- {}", strategy.reason, e);
                        error!("{}", msg);
                        break Err(CoreError::GraphExecution(msg));
                    }
                }
            }
        };

        self.maybe_compact().await;

        match result {
            Ok(()) => {
                info!("Task complete");
                self.memory
                    .store_execution_log(&self.session_id, "complete", "task", task)
                    .await?;

                // Store a learning entry in PDA file-based memory.
                {
                    let state_arc = self.graph_runner.state();
                    let s = state_arc.lock().await;
                    let slug = slugify(&s.active_task);
                    let learning_content = format!(
                        "# Learning: {}\n\n## Task\n{}\n\n## Key Events\n{}",
                        s.active_task,
                        s.active_task,
                        s.execution_log
                            .iter()
                            .rev()
                            .take(30)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    let learning_path = dirs_or_default()
                        .join("MEMORY")
                        .join("LEARNING")
                        .join(format!("{}.md", slug));
                    if let Some(parent) = learning_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = tokio::fs::write(&learning_path, &learning_content).await;
                }

                Ok(())
            }
            Err(e) => {
                error!(error = %e, "Task failed");
                self.memory
                    .store_execution_log(&self.session_id, "failed", "task", &format!("{e}"))
                    .await?;
                Err(e)
            }
        }
    }

    fn build_graph(&mut self) -> Result<petgraph::graph::NodeIndex, CoreError> {
        let wd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".into());
        let mut idxs = Vec::new();
        for phase in Phase::ordered() {
            let ctx = PhaseContext {
                phase,
                cognitive: Arc::clone(&self.cognitive),
                memory: Arc::clone(&self.memory),
                tools: Arc::clone(&self.tools),
                workdir: wd.clone(),
                markdown_ctx: self.markdown_ctx.clone(),
            };
            idxs.push(self.graph_runner.insert_node(phase.name(), Box::new(ctx)));
        }
        for w in idxs.windows(2) {
            self.graph_runner.insert_edge(w[0], w[1], "next".into());
        }
        Ok(idxs[0])
    }

    async fn maybe_compact(&self) -> &Self {
        let state_arc = self.graph_runner.state();
        if state_arc.lock().await.is_over_token_limit() {
            info!("Compacting context");
            state_arc.lock().await.compact_context(COMPACTION_CHARS);
        }
        self
    }

    /// Load an Ideal State Artifact from a SYSTEM.md file or an explicit path.
    ///
    /// 1. If `path` is `Some`, loads from that exact file.
    /// 2. If `path` is `None`, tries `SYSTEM.md` in the current directory.
    ///
    /// Returns the parsed `IdealStateArtifact` or a `CoreError` if the file
    /// cannot be found or parsed.
    pub async fn load_isa(path: Option<&std::path::Path>) -> Result<IdealStateArtifact, CoreError> {
        let resolved = match path {
            Some(p) => p.to_path_buf(),
            None => {
                let cwd = std::env::current_dir().map_err(|e| CoreError::Io(e.to_string()))?;
                cwd.join("SYSTEM.md")
            }
        };

        if !resolved.exists() {
            return Err(CoreError::Config(format!(
                "ISA file not found at '{}'. Create a SYSTEM.md with acceptance criteria.",
                resolved.display(),
            )));
        }

        let markdown = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        let id = resolved
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("SYSTEM")
            .to_string();

        let isa = crate::isa_parser::parse_isa_from_markdown(&id, &markdown)?;
        info!(path = %resolved.display(), id = %isa.id, criteria = isa.acceptance_criteria.len(), "ISA loaded");
        Ok(isa)
    }
}

// ── Real Agent Logic ──

#[async_trait::async_trait]
impl GraphNode for PhaseContext {
    fn name(&self) -> &str {
        self.phase.name()
    }

    async fn execute(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        info!(phase = %self.phase, "Executing");

        let ph = self.phase.name().to_string();
        {
            let mut s = state.lock().await;
            s.current_phase = Some(ph.clone());
            s.log_event(&format!("Phase: {}", self.phase));
        }

        let ctx = ToolContext {
            workdir: self.workdir.clone(),
            project_id: state.lock().await.project_id.clone().unwrap_or_default(),
        };

        match self.phase {
            Phase::Observe => self.observe(&ctx, state).await,
            Phase::Think => self.think(&ctx, state).await,
            Phase::Plan => self.plan(&ctx, state).await,
            Phase::Build => self.build(&ctx, state).await,
            Phase::Execute => self.exec(&ctx, state).await,
            Phase::Verify => self.verify(&ctx, state).await,
            Phase::Learn => self.learn(&ctx, state).await,
        }?;

        info!(phase = %ph, "Complete");
        Ok(())
    }
}

impl PhaseContext {
    /// Build a prompt with optional markdown context prepended.
    ///
    /// If a MarkdownContext is available, injects the formatted system prompt
    /// (doctrine, goal, criteria, constraints) as a preamble to the agent's
    /// reasoning. This ensures the agent operates within the defined
    /// constraints and targets the specified acceptance criteria.
    fn build_prompt_with_context(&self, base_prompt: &str) -> String {
        if let Some(ref ctx) = self.markdown_ctx {
            let preamble = ctx.format_system_prompt();
            if !preamble.is_empty() {
                return format!("{}\n\n---\n\n{}", preamble, base_prompt,);
            }
        }
        base_prompt.to_string()
    }
}

// ── Code writer helper ──

async fn write_code_files(output: &str, workdir: &str) -> usize {
    let mut count = 0;
    let mut path: Option<String> = None;
    let mut code = String::new();
    let mut in_block = false;

    for line in output.lines() {
        if line.starts_with("### FILE:") || line.starts_with("## FILE:") {
            if flush_file(&path, &code, workdir).await {
                count += 1;
            }
            path = Some(
                line.trim_start_matches("### FILE:")
                    .trim_start_matches("## FILE:")
                    .trim()
                    .to_string(),
            );
            code.clear();
            in_block = false;
        } else if line.trim() == "```" {
            in_block = !in_block;
        } else if in_block {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
        }
    }
    if flush_file(&path, &code, workdir).await {
        count += 1;
    }
    count
}

/// Returns `true` if a file was actually written.
async fn flush_file(path: &Option<String>, code: &str, workdir: &str) -> bool {
    if let Some(p) = path
        && !code.is_empty()
        && !p.is_empty()
    {
        let full = std::path::PathBuf::from(workdir).join(p);
        let _ = tokio::fs::create_dir_all(full.parent().unwrap()).await;
        let _ = tokio::fs::write(&full, code).await;
        true
    } else {
        false
    }
}

/// Resolve the PDA home directory (~/.candor) or fall back to /tmp.
fn dirs_or_default() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".candor")
    } else {
        PathBuf::from("/tmp/candor")
    }
}

/// Convert a string into a filesystem-safe slug.
/// Replaces spaces with hyphens, removes other non-alphanumeric chars.
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .trim_matches('-')
        .to_string()
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use candor_cognitive::CognitiveEngine;
    use candor_core::ideal::AcceptanceCriterion;
    use candor_core::ideal::VerificationMethod;
    #[tokio::test]
    async fn test_agent_init() {
        let c = Arc::new(CognitiveEngine::new(None, None).await.unwrap());
        let m = Arc::new(MemorySystem::new(384).await.unwrap());
        assert!(OrchestratorEngine::new(c, m, 100).await.is_ok());
    }

    #[tokio::test]
    async fn test_run_task() {
        // Safety guard: prevent RunTestsTool from recursively invoking
        // this test via `cargo test` during the Verify phase.
        // SAFETY: Single-threaded test, no concurrent access to this env var.
        unsafe {
            std::env::set_var("CANDOR_SKIP_TEST_EXECUTION", "1");
        }

        let c = Arc::new(CognitiveEngine::new(None, None).await.unwrap());
        let m = Arc::new(MemorySystem::new(384).await.unwrap());
        let mut agent = OrchestratorEngine::new(c, m, 100).await.unwrap();
        agent.sentinel.deactivate();
        let hooks = LifecycleHooks::default().with_before_tool(agent.sentinel.clone_box());
        agent.graph_runner = GraphRunner::new(100).with_hooks(hooks);

        let isa = IdealStateArtifact {
            id: "test".into(),
            goal: "list files".into(),
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "list-output".into(),
                description: "list_dir produces output".into(),
                verification_method: VerificationMethod::ShellCommand { command: "ls".into() },
            }],
            constraints: vec![],
            expected_artifacts: vec![],
            phase_requirements: Default::default(),
            fully_autonomous: true,
        };
        assert!(agent.run_task("list files", &isa, None).await.is_ok());
        assert_eq!(agent.graph_runner.node_count(), 7);
    }
}
