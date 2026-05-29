// candor-daemon: the axum-based daemon with LLM backend auto-detection.
use std::sync::Arc;

use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use candor_cognitive::{
    AnthropicBackend, CognitiveEngine, MockBackend, OpenAiBackend,
};
use candor_core::ideal::IdealStateArtifact;
use candor_memory::store::MemorySystem;
use candor_orchestrator::OrchestratorEngine;

mod routes;

/// Candor AI — Lawful Good, Rust-native Agentic Operating System.
#[derive(Parser, Debug)]
#[command(name = "candor", version, about)]
struct Cli {
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[arg(short, long, default_value = "31337")]
    port: u16,

    #[arg(long, default_value = "100")]
    max_iterations: u32,

    #[arg(long, default_value = "384")]
    embedding_dim: usize,

    /// Run a task through the 7-phase agent pipeline.
    #[arg(long)]
    task: Option<String>,

    /// Check daemon health and exit.
    #[arg(long)]
    health: bool,

    /// Explicit model to use (e.g., "claude-sonnet-4", "gpt-4o").
    #[arg(long)]
    model: Option<String>,

    /// OpenAI-compatible base URL (for LM Studio, Ollama, etc.).
    #[arg(long)]
    openai_base: Option<String>,

    /// OpenAI API key (or set OPENAI_API_KEY env var).
    #[arg(long)]
    openai_key: Option<String>,

    /// Anthropic API key (or set ANTHROPIC_API_KEY env var).
    #[arg(long)]
    anthropic_key: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub orchestrator: Arc<tokio::sync::Mutex<OrchestratorEngine>>,
    pub session_counter: Arc<std::sync::atomic::AtomicU64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .init();

    info!(
        "Starting Candor AI daemon v{}",
        env!("CARGO_PKG_VERSION")
    );

    // ── Detect and connect to LLM backends ──
    let cognitive = build_cognitive(&cli).await?;
    let memory = Arc::new(
        MemorySystem::new(cli.embedding_dim).await?,
    );

    let orchestrator = Arc::new(tokio::sync::Mutex::new(
        OrchestratorEngine::new(
            cognitive,
            memory,
            cli.max_iterations,
        )
        .await?,
    ));

    // ── CLI modes ──

    if let Some(task) = cli.task {
        run_cli_task(task, orchestrator).await?;
        return Ok(());
    }

    if cli.health {
        run_health_check(orchestrator).await;
        return Ok(());
    }

    // ── Daemon mode ──
    let state = AppState {
        orchestrator,
        session_counter: Arc::new(
            std::sync::atomic::AtomicU64::new(0),
        ),
    };

    let app = Router::new()
        .route("/", axum::routing::get(routes::root))
        .route("/api/health", axum::routing::get(routes::health))
        .route("/api/status", axum::routing::get(routes::status))
        .route("/api/task", axum::routing::post(routes::submit_task))
        .route("/api/metrics", axum::routing::get(routes::metrics))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cli.port);
    info!("Life Dashboard listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ── Backend auto-detection ──

async fn build_cognitive(
    cli: &Cli,
) -> Result<Arc<CognitiveEngine>, Box<dyn std::error::Error>> {
    use std::env;

    let anthropic_key = cli
        .anthropic_key
        .clone()
        .or_else(|| env::var("ANTHROPIC_API_KEY").ok());

    let openai_key = cli
        .openai_key
        .clone()
        .or_else(|| env::var("OPENAI_API_KEY").ok());

    let openai_base = cli
        .openai_base
        .clone()
        .or_else(|| env::var("OPENAI_BASE_URL").ok());

    let model = cli
        .model
        .clone()
        .or_else(|| env::var("CANDOR_MODEL").ok());

    // Priority: Anthropic > OpenAI/LM Studio > Ollama > Mock
    let mut backend: Option<Box<dyn candor_cognitive::LlmBackend>> = None;
    let mut backend_label = String::new();

    if let Some(ref key) = anthropic_key {
        let model = model.clone().unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        backend = Some(Box::new(AnthropicBackend::new(key.clone(), &model)));
        backend_label = format!("anthropic/{}", model);
    } else if let Some(ref key) = openai_key {
        let model = model.clone().unwrap_or_else(|| "gpt-4o".into());
        backend = Some(Box::new(OpenAiBackend::new(
            key.clone(),
            &model,
            openai_base.clone(),
        )));
        if let Some(ref base) = openai_base {
            backend_label = format!("openai-compatible@{}/{}", base, model);
        } else {
            backend_label = format!("openai/{}", model);
        }
    } else if let Ok(base) = env::var("LM_STUDIO_URL") {
        // LM Studio local — no API key needed
        let model = model.unwrap_or_else(|| "local-model".into());
        backend = Some(Box::new(OpenAiBackend::new(
            "lm-studio".into(),
            &model,
            Some(base),
        )));
        backend_label = format!("lm-studio/{}", model);
    } else if let Ok(base) = env::var("OLLAMA_URL") {
        // Ollama local — no API key needed
        let model = model.unwrap_or_else(|| "llama3".into());
        backend = Some(Box::new(OpenAiBackend::new(
            "ollama".into(),
            &model,
            Some(base),
        )));
        backend_label = format!("ollama/{}", model);
    }

    let cognitive = match backend {
        Some(backend) => {
            info!(
                backend = %backend_label,
                "LLM backend connected"
            );
            Arc::new(
                CognitiveEngine::new(
                    Some(backend),
                    None, // local pipeline not yet configured
                )
                .await?,
            )
        }
        None => {
            warn!("No LLM backend configured. Use --anthropic-key, --openai-key, or set ANTHROPIC_API_KEY / OPENAI_API_KEY / LM_STUDIO_URL / OLLAMA_URL.");
            warn!("Running in MOCK mode — agent will use placeholder responses.");
            let backend = MockBackend::new(
                "I am a mock LLM. No real backend is configured. \
                 Set ANTHROPIC_API_KEY or OPENAI_API_KEY to enable real AI capabilities.",
            );
            Arc::new(
                CognitiveEngine::new(
                    Some(Box::new(backend)),
                    None,
                )
                .await?,
            )
        }
    };

    Ok(cognitive)
}

// ── CLI task runner ──

async fn run_cli_task(
    task: String,
    orchestrator: Arc<tokio::sync::Mutex<OrchestratorEngine>>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(task = %task, "Running task from CLI");

    let isa = IdealStateArtifact {
        id: format!("cli-task-{}", uuid::Uuid::new_v4()),
        goal: task.clone(),
        acceptance_criteria: vec![],
        constraints: vec![],
        expected_artifacts: vec![],
        phase_requirements: Default::default(),
        fully_autonomous: true,
    };

    let mut orch = orchestrator.lock().await;
    match orch.run_task(&task, &isa).await {
        Ok(()) => {
            println!("Task completed: {task}");
            let state_arc = orch.graph_runner.state();
            let s = state_arc.lock().await;
            println!("\nExecution log (last 10 events):");
            for event in s.execution_log.iter().rev().take(10).rev() {
                println!("  {event}");
            }
        }
        Err(e) => {
            eprintln!("Task failed: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

async fn run_health_check(
    orchestrator: Arc<tokio::sync::Mutex<OrchestratorEngine>>,
) {
    let orch = orchestrator.lock().await;
    let has_frontier = orch.cognitive.is_frontier_healthy();
    let has_local = orch.cognitive.is_local_healthy();

    println!("Candor AI v{}", env!("CARGO_PKG_VERSION"));
    println!("Session: {}", orch.session_id);
    println!("Frontier LLM: {}", if has_frontier { "connected" } else { "not configured" });
    println!("Local LLM:    {}", if has_local { "connected" } else { "not configured" });
    println!("Sandbox:      {}", if orch.sandbox.native_engine().is_bwrap_available() { "bubblewrap" } else { "direct" });
    println!("Sentinel:     {}", if orch.sentinel.is_active() { "active" } else { "inactive" });
    println!("Tools:        {} registered", orch.tools.tool_count());
    println!("Memory:       {} dimensions", orch.memory.embedding_dim());
    println!("\nReady for agentic SWE tasks.");
}
