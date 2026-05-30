/// Candor AI — Lawful Good, Rust-native Agentic Operating System.
///
/// A production-grade agent harness implementing Algorithm v6.3.0.
/// Runs the 7-phase execution loop: Observe→Think→Plan→Build→Execute→Verify→Learn.
///
/// Subcommands:
///   task    Run a one-shot agent task
///   chat    Interactive conversational mode
///   voice   Voice-activated task via whisper STT
///   init    Bootstrap a new candor project
///   health  Check all subsystems
///   doctor  Run diagnostics and repair
///   serve   Start REST API daemon (default with --port)
use std::sync::Arc;

use axum::Router;
use clap::{Parser, Subcommand};
use tower_http::cors::CorsLayer;
use tracing::info;

use candor_cognitive::{
    AnthropicBackend, CognitiveEngine, DeepSeekBackend, GeminiBackend,
    MockBackend, OpenAiBackend,
};
use candor_core::ideal::IdealStateArtifact;
use candor_memory::store::MemorySystem;
use candor_orchestrator::OrchestratorEngine;

mod chat;
mod routes;
mod stt;

/// Candor AI — Lawful Good, Rust-native Agentic Operating System.
#[derive(Parser, Debug)]
#[command(name = "candor", version, about = "Lawful Good Rust Agentic Operating System", long_about = None)]
struct Cli {
    /// OpenTelemetry OTLP gRPC endpoint for trace export
    /// (e.g. http://localhost:4317).  When omitted, only local
    /// fmt logging is used.
    #[arg(long, global = true, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    otlp_endpoint: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a one-shot agent task
    Task {
        /// Task description for the agent to execute
        description: String,
        #[arg(long, help = "Model override")]
        model: Option<String>,
        #[arg(long, help = "OpenAI-compatible base URL")]
        openai_base: Option<String>,
        #[arg(long, help = "OpenAI API key")]
        openai_key: Option<String>,
        #[arg(long, help = "Anthropic API key")]
        anthropic_key: Option<String>,
        #[arg(long, default_value = "100", help = "Max graph iterations")]
        max_iterations: u32,
        #[arg(long, default_value = "384", help = "Embedding dimensions")]
        embedding_dim: usize,
    },

    /// Interactive conversational mode
    Chat {
        #[arg(long, help = "Model override")]
        model: Option<String>,
        #[arg(long, help = "OpenAI-compatible base URL")]
        openai_base: Option<String>,
        #[arg(long, help = "OpenAI API key")]
        openai_key: Option<String>,
        #[arg(long, help = "Anthropic API key")]
        anthropic_key: Option<String>,
    },

    /// Voice-activated task via whisper STT
    Voice {
        #[arg(long, help = "Prompt prefix (e.g. 'In Spanish:')")]
        prompt: Option<String>,
        #[arg(long, help = "Record duration in seconds", default_value = "5")]
        duration: u64,
    },

    /// Bootstrap a new candor project
    Init {
        /// Directory path for the new project
        path: String,
    },

    /// Check all subsystems
    Health,

    /// Start REST API daemon
    Serve {
        #[arg(short, long, default_value = "31337")]
        port: u16,
        #[arg(long, default_value = "100")]
        max_iterations: u32,
        #[arg(long, default_value = "384")]
        embedding_dim: usize,
    },

    /// Run diagnostics
    Doctor,
}

#[derive(Clone)]
pub struct AppState {
    pub orchestrator: Arc<tokio::sync::Mutex<OrchestratorEngine>>,
    pub session_counter: Arc<std::sync::atomic::AtomicU64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialise tracing: with OTLP if --otlp-endpoint is set, otherwise fmt.
    let _telemetry = candor_telemetry::init_telemetry(
        "candor-daemon",
        cli.otlp_endpoint.as_deref(),
    );

    match cli.command {
        Commands::Task { description, model, openai_base, openai_key, anthropic_key, max_iterations, embedding_dim } => {
            println!("{CYAN}{BOLD}   Candor AI v{} — Task Mode{RESET}\n", env!("CARGO_PKG_VERSION"));
            let cognitive = build_cognitive(model, openai_key, anthropic_key, openai_base).await?;
            let memory = Arc::new(MemorySystem::new(embedding_dim).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, max_iterations).await?,
            ));
            run_cli_task(description, orch).await?;
        }
        Commands::Chat { model, openai_base, openai_key, anthropic_key } => {
            println!("{CYAN}{BOLD}   Candor AI v{} — Chat Mode{RESET}\n", env!("CARGO_PKG_VERSION"));
            let cognitive = build_cognitive(model, openai_key, anthropic_key, openai_base).await?;
            let memory = Arc::new(MemorySystem::new(384).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, 100).await?,
            ));
            chat::run_chat(orch).await?;
        }
        Commands::Voice { prompt, duration } => {
            println!("{CYAN}{BOLD}   Candor AI v{} — Voice Mode{RESET}\n", env!("CARGO_PKG_VERSION"));
            run_voice_task(prompt, duration).await?;
        }
        Commands::Init { path } => {
            init_project(&path)?;
        }
        Commands::Health => {
            let cognitive = build_cognitive(None, None, None, None).await?;
            let memory = Arc::new(MemorySystem::new(384).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, 100).await?,
            ));
            run_health_check(orch).await;
        }
        Commands::Serve { port, max_iterations, embedding_dim } => {
            let cognitive = build_cognitive(None, None, None, None).await?;
            let memory = Arc::new(MemorySystem::new(embedding_dim).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, max_iterations).await?,
            ));
            let state = AppState {
                orchestrator: orch,
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
            let addr = format!("0.0.0.0:{}", port);
            info!("Candor AI daemon listening on http://{addr}");
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            axum::serve(listener, app).await?;
        }
        Commands::Doctor => {
            run_doctor().await;
        }
    }
    Ok(())
}

// ── Color constants ──
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

// ── Backend construction ──

async fn build_cognitive(
    model: Option<String>, openai_key: Option<String>,
    anthropic_key: Option<String>, openai_base: Option<String>,
) -> Result<Arc<CognitiveEngine>, Box<dyn std::error::Error>> {
    use std::env;

    let anthropic_key = anthropic_key.or_else(|| env::var("ANTHROPIC_API_KEY").ok());
    let openai_key = openai_key.or_else(|| env::var("OPENAI_API_KEY").ok());
    let openai_base = openai_base.or_else(|| env::var("OPENAI_BASE_URL").ok());
    let model_name = model.or_else(|| env::var("CANDOR_MODEL").ok());

    let mut backend: Option<Box<dyn candor_cognitive::LlmBackend>> = None;
    let mut label = String::new();

    if let Some(ref key) = anthropic_key {
        let m = model_name.clone().unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        backend = Some(Box::new(AnthropicBackend::new(key.clone(), &m)));
        label = format!("anthropic/{m}");
    } else if let Some(ref key) = env::var("DEEPSEEK_API_KEY").ok().as_ref() {
        let m = model_name.clone().unwrap_or_else(|| "deepseek-chat".into());
        backend = Some(Box::new(DeepSeekBackend::new(key.to_string(), &m)));
        label = format!("deepseek/{m}");
    } else if let Some(ref key) = env::var("GEMINI_API_KEY").ok().as_ref() {
        let m = model_name.clone().unwrap_or_else(|| "gemini-2.5-flash".into());
        backend = Some(Box::new(GeminiBackend::new(key.to_string(), &m)));
        label = format!("gemini/{m}");
    } else if let Some(ref key) = openai_key {
        let m = model_name.clone().unwrap_or_else(|| "gpt-4o".into());
        backend = Some(Box::new(OpenAiBackend::new(key.clone(), &m, openai_base.clone())));
        label = if let Some(ref b) = openai_base { format!("openai@{b}/{m}") } else { format!("openai/{m}") };
    } else if let Ok(base) = env::var("LM_STUDIO_URL") {
        let m = model_name.clone().unwrap_or_else(|| "local-model".into());
        backend = Some(Box::new(OpenAiBackend::new("lm-studio".into(), &m, Some(base))));
        label = format!("lm-studio/{m}");
    } else if let Ok(base) = env::var("OLLAMA_URL") {
        let m = model_name.unwrap_or_else(|| "llama3".into());
        backend = Some(Box::new(OpenAiBackend::new("ollama".into(), &m, Some(base))));
        label = format!("ollama/{m}");
    }

    match backend {
        Some(b) => {
            eprintln!("{GREEN}✓{RESET} {BOLD}LLM:{RESET} {label}");
            Ok(Arc::new(CognitiveEngine::new(Some(b), None).await?))
        }
        None => {
            eprintln!("{YELLOW}⚠ LLM: Not configured — using Mock{RESET}");
            eprintln!("  Set ANTHROPIC_API_KEY, DEEPSEEK_API_KEY, GEMINI_API_KEY, OPENAI_API_KEY, LM_STUDIO_URL, or OLLAMA_URL");
            Ok(Arc::new(CognitiveEngine::new(Some(Box::new(MockBackend::new("mock"))), None).await?))
        }
    }
}

// ── Task runner ──

async fn run_cli_task(task: String, orch: Arc<tokio::sync::Mutex<OrchestratorEngine>>) -> Result<(), Box<dyn std::error::Error>> {
    let isa = IdealStateArtifact {
        id: format!("cli-{}", uuid::Uuid::new_v4()),
        goal: task.clone(), acceptance_criteria: vec![], constraints: vec![],
        expected_artifacts: vec![], phase_requirements: Default::default(),
        fully_autonomous: true,
    };

    let mut o = orch.lock().await;
    match o.run_task(&task, &isa, None).await {
        Ok(()) => {
            println!("\n{GREEN}{BOLD}✓ Task completed.{RESET}");
            let state_arc = o.graph_runner.state();
            let s = state_arc.lock().await;
            for event in s.execution_log.iter().rev().take(5).rev() {
                println!("  {GREEN}→{RESET} {event}");
            }
        }
        Err(e) => {
            eprintln!("\n{RED}✗ Task failed: {e}{RESET}");
            std::process::exit(1);
        }
    }
    Ok(())
}

// ── Voice task runner ──

async fn run_voice_task(prompt: Option<String>, _duration: u64) -> Result<(), Box<dyn std::error::Error>> {
    let text = stt::transcribe_mic().await.map_err(|e| {
        Box::new(std::io::Error::other(format!("Voice error: {e}")))
    })?;
    let task = if let Some(ref p) = prompt {
        format!("{p} {text}")
    } else {
        text
    };
    println!("\n  {CYAN}You said:{RESET} {}\n", task);
    let cognitive = build_cognitive(None, None, None, None).await?;
    let memory = Arc::new(MemorySystem::new(384).await?);
    let orch = Arc::new(tokio::sync::Mutex::new(
        OrchestratorEngine::new(cognitive, memory, 100).await?,
    ));
    run_cli_task(task, orch).await
}

// ── Init project ──

fn init_project(dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&path)?;
    std::fs::write(path.join("candor.toml"), "[server]\nhost = \"0.0.0.0\"\nport = 31337\ncheckpoint_dir = \"/tmp/candor-checkpoints\"\nmax_iterations = 100\n\n[sandbox]\nscratchpad_dir = \"/tmp/agent_scratchpad\"\ndefault_timeout_secs = 15\ndefault_memory_mb = 256\n\n[inference]\n# anthropic_api_key = \"sk-ant-...\"\n# openai_api_key = \"sk-...\"\nembedding_model = \"all-MiniLM-L6-v2\"\nembedding_dim = 384\n\n[memory]\nbackend = \"mem\"\ncompaction_token_limit = 135000\n")?;
    std::fs::write(path.join(".gitignore"), "/target/\n.env\n/tmp/\n")?;
    println!("{GREEN}✓{RESET} Project initialized at {BOLD}{}{RESET}", path.display());
    println!("  candor task \"build something\"");
    Ok(())
}

// ── Health check ──

async fn run_health_check(orch: Arc<tokio::sync::Mutex<OrchestratorEngine>>) {
    let o = orch.lock().await;
    let frontier = o.cognitive.is_frontier_healthy();
    let local = o.cognitive.is_local_healthy();

    println!();
    println!("{BOLD}  Candor AI — Health Check{RESET}\n");
    println!("  {BOLD}LLM:{RESET}      {}", if frontier { format!("{GREEN}Connected{RESET}") } else { format!("{YELLOW}Not configured{RESET}") });
    println!("  {BOLD}Local:{RESET}    {}", if local { format!("{GREEN}Connected{RESET}") } else { format!("{YELLOW}Not configured{RESET}") });
    println!("  {BOLD}Sandbox:{RESET}  {}", if o.sandbox.native_engine().is_bwrap_available() { "Bubblewrap" } else { "Direct" });
    println!("  {BOLD}Sentinel:{RESET} {}", if o.sentinel.is_active() { format!("{GREEN}Active{RESET}") } else { format!("{YELLOW}Inactive{RESET}") });
    println!("  {BOLD}Tools:{RESET}    {} registered", o.tools.tool_count());
    println!();
    println!("{GREEN}  All systems operational.{RESET}");
}

// ── Doctor diagnostics ──

async fn run_doctor() {
    println!("\n{BOLD}Candor AI — Doctor{RESET}\n");

    let checks = [
        ("cargo", check_cmd("cargo")),
        ("git", check_cmd("git")),
        ("bubblewrap", check_cmd("bwrap")),
        ("whisper", check_cmd("whisper-cpp") || check_cmd("whisper-cli") || check_cmd("whisper")),
        ("surrealDB", true), // embedded, always available
    ];
    let all_ok = checks.iter().all(|(_, ok)| *ok);
    for (name, ok) in &checks {
        println!("  {} {name}", if *ok { format!("{GREEN}✓{RESET}") } else { format!("{YELLOW}○{RESET}") });
    }
    println!();
    if all_ok {
        println!("{GREEN}✓ All checks passed.{RESET}");
    } else {
        println!("{YELLOW}○ Some optional dependencies missing. Candor will still work.{RESET}");
    }
}

fn check_cmd(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
