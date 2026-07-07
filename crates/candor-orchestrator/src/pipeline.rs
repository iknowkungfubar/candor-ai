/// Phase handler trait for the agent execution pipeline.
///
/// The 7-phase loop (Observe → Think → Plan → Build → Execute → Verify → Learn)
/// is modeled as a pipeline of PhaseHandlers. Each phase implements this trait,
/// and the pipeline runs them in sequence.
///
/// To add a new phase, implement PhaseHandler and register it in the pipeline
/// builder — no changes needed to OrchestratorEngine.
use std::sync::Arc;

use async_trait::async_trait;
use candor_cognitive::CognitiveEngine;
use candor_core::error::CoreError;
use candor_core::ideal::IdealStateArtifact;
use candor_graph::runner::GraphRunner;
use candor_memory::store::MemorySystem;
use candor_sandbox::unified::ToolSandbox;
use candor_sentinel::interceptor::SentinelInterceptor;
use candor_tools::registry::ToolRegistry;

/// Context provided to each phase handler.
pub struct PhaseContext<'a> {
    pub task: &'a str,
    pub isa: &'a IdealStateArtifact,
    pub graph_runner: &'a mut GraphRunner,
    pub sandbox: &'a ToolSandbox,
    pub cognitive: &'a Arc<CognitiveEngine>,
    pub memory: &'a Arc<MemorySystem>,
    pub sentinel: &'a SentinelInterceptor,
    pub tools: &'a Arc<ToolRegistry>,
    pub session_id: &'a str,
}

/// Output from a phase handler.
pub enum PhaseOutcome {
    /// Continue to the next phase.
    Continue,
    /// Skip remaining phases and complete early.
    Complete,
    /// Abort the pipeline with an error.
    Fail(CoreError),
}

/// A single phase in the agent execution pipeline.
#[async_trait]
pub trait PhaseHandler: Send {
    /// Execute this phase. Return PhaseOutcome to control pipeline flow.
    async fn execute(&self, ctx: &mut PhaseContext<'_>) -> Result<PhaseOutcome, CoreError>;
}

/// The agent execution pipeline — runs PhaseHandlers in sequence.
pub struct AgentPipeline {
    phases: Vec<Box<dyn PhaseHandler>>,
}

impl AgentPipeline {
    pub fn new(phases: Vec<Box<dyn PhaseHandler>>) -> Self {
        Self { phases }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        task: &str,
        isa: &IdealStateArtifact,
        graph_runner: &mut GraphRunner,
        sandbox: &ToolSandbox,
        cognitive: &Arc<CognitiveEngine>,
        memory: &Arc<MemorySystem>,
        sentinel: &SentinelInterceptor,
        tools: &Arc<ToolRegistry>,
        session_id: &str,
    ) -> Result<(), CoreError> {
        let mut ctx = PhaseContext {
            task,
            isa,
            graph_runner,
            sandbox,
            cognitive,
            memory,
            sentinel,
            tools,
            session_id,
        };

        for phase in &self.phases {
            match phase.execute(&mut ctx).await? {
                PhaseOutcome::Continue => continue,
                PhaseOutcome::Complete => break,
                PhaseOutcome::Fail(err) => return Err(err),
            }
        }
        Ok(())
    }
}
