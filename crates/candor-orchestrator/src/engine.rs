/// The complete Candor Agent — fully LLM-driven 7-phase SWE agent.
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
use candor_graph::runner::GraphRunner;
use candor_memory::store::MemorySystem;
use candor_sandbox::unified::ToolSandbox;
use candor_tools::registry::{ToolContext, ToolRegistry};
use candor_tools::{
    ListDirTool, ReadFileTool, RunTestsTool, SearchCodeTool,
    SearchFilesTool, ShellTool, WriteFileTool,
    GitBranchTool, GitCommitTool, GitPushTool, GitStatusTool,
};

use super::phases::Phase;

const COMPACTION_CHARS: usize = 8192;

// ── Phase context (immutable during graph run) ──

struct PhaseContext {
    phase: Phase,
    cognitive: Arc<CognitiveEngine>,
    memory: Arc<MemorySystem>,
    tools: Arc<ToolRegistry>,
    workdir: String,
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
}

impl OrchestratorEngine {
    pub async fn new(
        cognitive: Arc<CognitiveEngine>,
        memory: Arc<MemorySystem>,
        max_iterations: u32,
    ) -> Result<Self, CoreError> {
        info!("Initializing Candor Agent");

        let sandbox = ToolSandbox::new().map_err(|e| {
            CoreError::Internal(format!("Sandbox: {e}"))
        })?;

        let sentinel = candor_sentinel::interceptor::SentinelInterceptor::new(
            Arc::clone(&cognitive), vec![],
        );

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
            .with_before_tool(sentinel.clone_box());
        let graph_runner = GraphRunner::new(max_iterations).with_hooks(hooks);
        let session_id = Uuid::new_v4().to_string();

        info!(session_id = %session_id, tools = tools_arc.tool_count(), "Agent ready");

        Ok(Self {
            graph_runner, sandbox, cognitive, memory,
            sentinel, session_id, tools: tools_arc,
        })
    }

    pub async fn run_task(
        &mut self, task: &str, isa: &IdealStateArtifact,
    ) -> Result<(), CoreError> {
        info!(task = %task, "Starting agent task");
        {
            let state_arc = self.graph_runner.state();
            let mut s = state_arc.lock().await;
            s.active_task = task.to_string();
            s.project_id = Some(isa.id.clone());
            s.log_event(&format!("Task: {task}"));
        }

        let start = self.build_graph()?;
        let result = self.graph_runner.execute_graph(start).await;

        self.maybe_compact().await;

        match result {
            Ok(()) => {
                info!("Task complete");
                self.memory.store_execution_log(
                    &self.session_id, "complete", "task", task,
                ).await?;
                Ok(())
            }
            Err(e) => {
                error!(error = %e, "Task failed");
                self.memory.store_execution_log(
                    &self.session_id, "failed", "task", &format!("{e}"),
                ).await?;
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
                phase, cognitive: Arc::clone(&self.cognitive),
                memory: Arc::clone(&self.memory),
                tools: Arc::clone(&self.tools),
                workdir: wd.clone(),
            };
            idxs.push(self.graph_runner.insert_node(phase.name(), Box::new(ctx)));
        }
        for w in idxs.windows(2) {
            self.graph_runner.insert_edge(w[0], w[1], "next".into());
        }
        Ok(idxs[0])
    }

    async fn maybe_compact(&self) {
        let state_arc = self.graph_runner.state();
        if state_arc.lock().await.is_over_token_limit() {
            info!("Compacting context");
            state_arc.lock().await.compact_context(COMPACTION_CHARS);
        }
    }
}

// ── Real Agent Logic ──

#[async_trait::async_trait]
impl GraphNode for PhaseContext {
    fn name(&self) -> &str { self.phase.name() }

    async fn execute(&self, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        info!(phase = %self.phase, "Executing");

        let ph = self.phase.name().to_string();
        { let mut s = state.lock().await;
          s.current_phase = Some(ph.clone());
          s.log_event(&format!("Phase: {}", self.phase)); }

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
    async fn observe(&self, ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let mut s = state.lock().await;
        s.log_event("Observe: scanning project");

        // List project structure
        if let Some(tool) = self.tools.find("list_dir") {
            if let Ok(out) = tool.execute(ctx, &[]).await {
                s.append_message(&format!("Project files:\n{}", out.output));
            }
        }
        // Find key files
        if let Some(tool) = self.tools.find("search_files") {
            if let Ok(out) = tool.execute(ctx, &["*.rs".to_string()]).await {
                s.append_message(&format!("\nRust sources:\n{}", out.output));
            }
        }
        // Read Cargo.toml
        if let Some(tool) = self.tools.find("read_file") {
            if let Ok(out) = tool.execute(ctx, &["Cargo.toml".into()]).await {
                s.append_message(&format!("\nCargo.toml:\n{}", out.output));
            }
        }
        s.log_event("Observe: complete");
        Ok(())
    }

    async fn think(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let context = {
            let s = state.lock().await;
            s.message_history.iter().rev().take(15).cloned().collect::<Vec<_>>().join("\n")
        };

        let prompt = format!(
            "You are a software engineering agent. Analyze the project context and identify what needs to be done.\n\nContext:\n{context}\n\nOutput a brief, specific analysis.",
        );

        match self.cognitive.generate_fast(&prompt).await {
            Ok(analysis) => {
                let mut s = state.lock().await;
                s.append_message(&format!("Analysis: {analysis}"));
                s.log_event("Think: complete");
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.log_event(&format!("Think: LLM unavailable ({e})"));
            }
        }
        Ok(())
    }

    async fn plan(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let context = {
            let s = state.lock().await;
            s.message_history.iter().rev().take(20).cloned().collect::<Vec<_>>().join("\n")
        };

        let tools_desc = self.tools.descriptions_for_llm();
        let prompt = format!(
            "You are a software engineering agent. Generate a numbered plan.\n\nAvailable tools:\n{tools_desc}\n\nContext:\n{context}\n\nOutput numbered, actionable steps.",
        );

        match self.cognitive.generate_fast(&prompt).await {
            Ok(plan) => {
                let mut s = state.lock().await;
                s.append_message(&format!("Plan:\n{plan}"));
                s.log_event("Plan: complete");
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.append_message("Plan: 1) Implement changes 2) Run tests 3) Commit");
                s.log_event(&format!("Plan: fallback ({e})"));
            }
        }
        Ok(())
    }

    async fn build(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let context = {
            let s = state.lock().await;
            s.message_history.iter().rev().take(25).cloned().collect::<Vec<_>>().join("\n")
        };

        let prompt = format!(
            "You are a software engineering agent. Write code changes.\n\nContext:\n{context}\n\nFormat each file as:\n### FILE: path\n```\ncode\n```\n\nWrite COMPLETE, compilable code files.",
        );

        match self.cognitive.generate_fast(&prompt).await {
            Ok(code) => {
                let written = write_code_files(&code, &self.workdir).await;
                let mut s = state.lock().await;
                s.append_message(&format!("Generated code ({written} files):\n{code}"));
                s.log_event(&format!("Build: {written} files written"));
            }
            Err(e) => {
                let mut s = state.lock().await;
                s.log_event(&format!("Build: LLM unavailable ({e})"));
            }
        }
        Ok(())
    }

    async fn exec(&self, ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let mut s = state.lock().await;
        s.log_event("Execute: running cargo check");

        if let Some(tool) = self.tools.find("shell") {
            match tool.execute(ctx, &["cargo check 2>&1".into()]).await {
                Ok(out) => {
                    s.append_message(&format!("Build:\n{}", out.output));
                    s.log_event("Execute: cargo check complete");
                }
                Err(e) => s.log_event(&format!("Execute: failed ({e})")),
            }
        }
        Ok(())
    }

    async fn verify(&self, ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let mut s = state.lock().await;
        s.log_event("Verify: running tests");

        if let Some(tool) = self.tools.find("run_tests") {
            match tool.execute(ctx, &[]).await {
                Ok(out) => {
                    let passed = out.success;
                    s.append_message(&format!("Tests:\n{}", out.output));
                    let msg = if passed { "Verify: PASSED" } else { "Verify: FAILED" };
                    s.log_event(msg);
                }
                Err(e) => s.log_event(&format!("Verify: error ({e})")),
            }
        }
        Ok(())
    }

    async fn learn(&self, _ctx: &ToolContext, state: Arc<Mutex<AgentState>>) -> Result<(), CoreError> {
        let (task, pid, log) = {
            let s = state.lock().await;
            (s.active_task.clone(), s.project_id.clone(), s.execution_log.clone())
        };

        // Generate session summary
        let mut summary = format!("# Session\n**Task:** {task}\n\n## Events\n");
        for e in log.iter().rev().take(30) { summary.push_str(&format!("- {e}\n")); }

        if let Some(ref pid) = pid {
            let emb = vec![0.0_f32; 384];
            let _ = self.memory.store_memory(pid.clone(), summary.clone(), emb).await;
            info!(pid = %pid, "Learn: stored");
        }

        let mut s = state.lock().await;
        s.log_event(&format!("Learn: {} events archived", log.len()));
        Ok(())
    }
}

// ── Code writer helper ──

async fn write_code_files(output: &str, workdir: &str) -> usize {
    let count = 0;
    let mut path: Option<String> = None;
    let mut code = String::new();
    let mut in_block = false;

    for line in output.lines() {
        if line.starts_with("### FILE:") || line.starts_with("## FILE:") {
            flush_file(&path, &code, workdir).await;
            path = Some(line.trim_start_matches("### FILE:").trim_start_matches("## FILE:").trim().to_string());
            code.clear();
            in_block = false;
        } else if line.trim() == "```" {
            if in_block {
                in_block = false;
            } else {
                in_block = true;
            }
        } else if in_block {
            if !code.is_empty() { code.push('\n'); }
            code.push_str(line);
        }
    }
    flush_file(&path, &code, workdir).await;
    count
}

async fn flush_file(path: &Option<String>, code: &str, workdir: &str) {
    if let Some(p) = path {
        if !code.is_empty() && !p.is_empty() {
            let full = std::path::PathBuf::from(workdir).join(p);
            let _ = tokio::fs::create_dir_all(full.parent().unwrap()).await;
            let _ = tokio::fs::write(&full, code).await;
        }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_init() {
        let c = Arc::new(CognitiveEngine::new(None, None).await.unwrap());
        let m = Arc::new(MemorySystem::new(384).await.unwrap());
        assert!(OrchestratorEngine::new(c, m, 100).await.is_ok());
    }

    #[tokio::test]
    async fn test_run_task() {
        let c = Arc::new(CognitiveEngine::new(None, None).await.unwrap());
        let m = Arc::new(MemorySystem::new(384).await.unwrap());
        let mut agent = OrchestratorEngine::new(c, m, 100).await.unwrap();
        agent.sentinel.deactivate();
        let hooks = LifecycleHooks::default().with_before_tool(agent.sentinel.clone_box());
        agent.graph_runner = GraphRunner::new(100).with_hooks(hooks);

        let isa = IdealStateArtifact {
            id: "test".into(), goal: "test".into(),
            acceptance_criteria: vec![], constraints: vec![],
            expected_artifacts: vec![], phase_requirements: Default::default(),
            fully_autonomous: true,
        };
        assert!(agent.run_task("list files", &isa).await.is_ok());
        assert_eq!(agent.graph_runner.node_count(), 7);
    }
}
