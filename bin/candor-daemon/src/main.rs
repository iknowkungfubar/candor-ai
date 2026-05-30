// candor-daemon: the axum-based daemon with LLM backend auto-detection.
// v2: improved CLI, colored output, --init, better UX.
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

mod chat;
mod routes;
mod stt;

const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Candor AI — Lawful Good, Rust-native Agentic Operating System.
///
/// A production-grade agent harness implementing Algorithm v6.3.0.
/// Runs the 7-phase execution loop: Observe→Think→Plan→Build→Execute→Verify→Learn.
///
/// Examples:
///   candor --task "build a CLI tool for fibonacci"
///   candor --health
///   candor --init my-project
///   candor --port 31337
#[derive(Parser, Debug)]
#[command(name = "candor", version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[arg(short, long, default_value = "31337")]
    port: u16,

    #[arg(long, default_value = "100", help = "Maximum graph iterations")]
    max_iterations: u32,

    #[arg(long, default_value = "384", help = "Embedding vector dimensions")]
    embedding_dim: usize,

    /// Run a task through the 7-phase agent pipeline.
    #[arg(long, help = "Task description for the agent to execute")]
    task: Option<String>,

    /// Enter interactive chat REPL mode (readline-style conversation).
    #[arg(long, help = "Start an interactive conversational chat REPL")]
    chat: bool,

    /// Listen mode: read tasks line-by-line from stdin pipe (for AI integration).
    #[arg(long, help = "Read tasks from stdin pipe (non-interactive)")]
    listen: bool,

    /// Voice task: record from microphone, transcribe with whisper, and execute.
    #[arg(long, help = "Record voice command, transcribe, and run as a task")]
    voice_task: bool,

    /// Optional prompt prefix for --voice-task (e.g. \"In French, \").
    #[arg(long, help = "Optional prompt prefix prepended to voice transcription")]
    voice_prompt: Option<String>,

    /// Check daemon health and exit.
    #[arg(long)]
    health: bool,

    /// Initialize a new candor project in the given directory.
    #[arg(long, help = "Create a new project with candor.toml scaffold")]
    init: Option<String>,

    #[arg(long, help = "Model to use (e.g. claude-sonnet-4, gpt-4o)")]
    model: Option<String>,

    #[arg(long, help = "OpenAI-compatible base URL for LM Studio/Ollama")]
    openai_base: Option<String>,

    #[arg(long, help = "OpenAI API key")]
    openai_key: Option<String>,

    #[arg(long, help = "Anthropic API key")]
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

    // ── --init: bootstrap a project ──
    if let Some(ref dir) = cli.init {
        return init_project(dir);
    }

    println!("{CYAN}{BOLD}   Candor AI v{} — Lawful Good Agentic OS{RESET}\n", env!("CARGO_PKG_VERSION"));

    // ── Build subsystems ──
    let cognitive = build_cognitive(&cli).await?;
    let memory = Arc::new(MemorySystem::new(cli.embedding_dim).await?);
    let orchestrator = Arc::new(tokio::sync::Mutex::new(
        OrchestratorEngine::new(cognitive, memory, cli.max_iterations).await?,
    ));

    // ── CLI modes ──
    if let Some(task) = cli.task {
        run_cli_task(task, orchestrator).await?;
        return Ok(());
    }

    if cli.chat {
        chat::run_chat(orchestrator).await?;
        return Ok(());
    }

    if cli.listen {
        chat::run_listen(orchestrator).await?;
        return Ok(());
    }

    // ── Voice task ──
    if cli.voice_task {
        run_voice_task(cli.voice_prompt, orchestrator).await?;
        return Ok(());
    }

    if cli.health {
        run_health_check(orchestrator).await;
        return Ok(());
    }

    // ── Daemon mode ──
    let state = AppState {
        orchestrator,
        session_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
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

/// Bootstrap a new candor project: write candor.toml and .gitignore.
fn init_project(dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&path)?;

    let config = r#"[server]
host = "0.0.0.0"
port = 31337
checkpoint_dir = "/tmp/candor-checkpoints"
max_iterations = 100

[sandbox]
scratchpad_dir = "/tmp/agent_scratchpad"
default_timeout_secs = 15
default_memory_mb = 256
default_fuel_limit = 1_000_000

[inference]
# Uncomment to enable cloud APIs:
# anthropic_api_key = "sk-ant-..."
# openai_api_key = "sk-..."
embedding_model = "all-MiniLM-L6-v2"
embedding_dim = 384

[memory]
backend = "mem"
compaction_token_limit = 135000

[sentinel]
enabled = true
semantic_audit_enabled = true
scopes = []

[telemetry]
enabled = false
"#;

    std::fs::write(path.join("candor.toml"), config)?;
    std::fs::write(path.join(".gitignore"), "/target/\n/tmp/\n.env\n")?;

    println!("{GREEN}✓ Project initialized at {}{RESET}", path.display());
    println!("  Run: cd {} && candor --task \"your task here\"", dir);

    Ok(())
}

async fn build_cognitive(cli: &Cli) -> Result<Arc<CognitiveEngine>, Box<dyn std::error::Error>> {
    use std::env;

    let anthropic_key = cli.anthropic_key.clone().or_else(|| env::var("ANTHROPIC_API_KEY").ok());
    let openai_key = cli.openai_key.clone().or_else(|| env::var("OPENAI_API_KEY").ok());
    let openai_base = cli.openai_base.clone().or_else(|| env::var("OPENAI_BASE_URL").ok());
    let model = cli.model.clone().or_else(|| env::var("CANDOR_MODEL").ok());

    let mut backend: Option<Box<dyn candor_cognitive::LlmBackend>> = None;
    let mut label = String::new();

    if let Some(ref key) = anthropic_key {
        let m = model.clone().unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        backend = Some(Box::new(AnthropicBackend::new(key.clone(), &m)));
        label = format!("anthropic/{}", m);
    } else if let Some(ref key) = openai_key {
        let m = model.clone().unwrap_or_else(|| "gpt-4o".into());
        backend = Some(Box::new(OpenAiBackend::new(key.clone(), &m, openai_base.clone())));
        label = if let Some(ref b) = openai_base { format!("openai-compatible@{b}/{m}") } else { format!("openai/{m}") };
    } else if let Ok(base) = env::var("LM_STUDIO_URL") {
        let m = model.unwrap_or_else(|| "local-model".into());
        backend = Some(Box::new(OpenAiBackend::new("lm-studio".into(), &m, Some(base))));
        label = format!("lm-studio/{}", m);
    } else if let Ok(base) = env::var("OLLAMA_URL") {
        let m = model.unwrap_or_else(|| "llama3".into());
        backend = Some(Box::new(OpenAiBackend::new("ollama".into(), &m, Some(base))));
        label = format!("ollama/{}", m);
    }

    match backend {
        Some(b) => {
            println!("{GREEN}✓{RESET} LLM connected: {BOLD}{label}{RESET}");
            info!(backend = %label, "LLM backend connected");
            Ok(Arc::new(CognitiveEngine::new(Some(b), None).await?))
        }
        None => {
            println!("{YELLOW}⚠{RESET} No LLM configured — using mock mode");
            println!("  Set {CYAN}LM_STUDIO_URL{RESET}, {CYAN}OPENAI_API_KEY{RESET}, or {CYAN}ANTHROPIC_API_KEY{RESET}");
            warn!("No LLM backend — using mock mode");
            Ok(Arc::new(CognitiveEngine::new(Some(Box::new(MockBackend::new("I am a mock LLM."))), None).await?))
        }
    }
}

async fn run_cli_task(task: String, orchestrator: Arc<tokio::sync::Mutex<OrchestratorEngine>>) -> Result<(), Box<dyn std::error::Error>> {
    println!("{CYAN}▶ Task:{RESET} {task}\n");

    let isa = IdealStateArtifact {
        id: format!("cli-{}", uuid::Uuid::new_v4()),
        goal: task.clone(), acceptance_criteria: vec![], constraints: vec![],
        expected_artifacts: vec![], phase_requirements: Default::default(),
        fully_autonomous: true,
    };

    let mut orch = orchestrator.lock().await;
    let phases = ["Observe", "Think", "Plan", "Build", "Execute", "Verify", "Learn"];

    match orch.run_task(&task, &isa).await {
        Ok(()) => {
            println!("\n{GREEN}{BOLD}✓ Task completed{RESET}");
            let state_arc = orch.graph_runner.state();
            let s = state_arc.lock().await;
            for event in s.execution_log.iter().rev().take(10).rev() {
                let phase_icon = phases.iter().position(|p| event.contains(p)).map(|i| i + 1).unwrap_or(0);
                println!("  {GREEN}[{phase_icon}/7]{RESET} {event}");
            }
        }
        Err(e) => {
            eprintln!("\n{RED}✗ Task failed:{RESET} {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Run a voice-activated task: record → transcribe → execute.
async fn run_voice_task(
    voice_prompt: Option<String>,
    orchestrator: Arc<tokio::sync::Mutex<OrchestratorEngine>>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n{}🎤 Candor Voice Task{}", BOLD, RESET);

    match stt::transcribe_mic().await {
        Ok(transcribed) => {
            if transcribed.trim().is_empty() {
                eprintln!("{RED}✗ No speech detected.{RESET}");
                return Ok(());
            }

            let task = match voice_prompt {
                Some(prefix) => format!("{} {}", prefix.trim(), transcribed.trim()),
                None => transcribed.trim().to_string(),
            };

            println!("\n{CYAN}▶ Task:{RESET} {BOLD}{task}{RESET}\n");
            run_cli_task(task, orchestrator).await
        }
        Err(e) => {
            eprintln!("\n{RED}✗ Voice task failed:{RESET} {e}");
            if matches!(&e, stt::SttError::Unavailable) {
                println!("\n{YELLOW}💡 Tip:{RESET} Install whisper-cpp:");
                println!("  git clone https://github.com/ggerganov/whisper.cpp.git");
                println!("  cd whisper.cpp && make && sudo make install");
                println!("  Or download a model: make tiny.en");
            }
            Ok(())
        }
    }
}

async fn run_health_check(orchestrator: Arc<tokio::sync::Mutex<OrchestratorEngine>>) {
    let orch = orchestrator.lock().await;
    let frontier = orch.cognitive.is_frontier_healthy();
    let local = orch.cognitive.is_local_healthy();

    println!("{BOLD}Session:{RESET} {}", orch.session_id);
    println!("{BOLD}Frontier LLM:{RESET} {}", if frontier { format!("{GREEN}connected{RESET}") } else { format!("{YELLOW}not configured{RESET}") });
    println!("{BOLD}Local LLM:{RESET}    {}", if local { format!("{GREEN}connected{RESET}") } else { format!("{YELLOW}not configured{RESET}") });
    println!("{BOLD}Sandbox:{RESET}      {}", if orch.sandbox.native_engine().is_bwrap_available() { "bubblewrap" } else { "direct" });
    println!("{BOLD}Sentinel:{RESET}     {}", if orch.sentinel.is_active() { format!("{GREEN}active{RESET}") } else { format!("{YELLOW}inactive{RESET}") });
    println!("{BOLD}Tools:{RESET}        {} registered", orch.tools.tool_count());
    println!("{BOLD}Memory:{RESET}       {} dimensions", orch.memory.embedding_dim());
    println!("\n{GREEN}All subsystems operational.{RESET}");
}
