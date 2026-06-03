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

use axum::{Router, middleware};
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use clap::{Parser, Subcommand};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::info;

use candor_cognitive::{
    AnthropicBackend, CognitiveEngine, DeepSeekBackend, GeminiBackend, MockBackend, OpenAiBackend,
};
use candor_core::ideal::IdealStateArtifact;
use candor_memory::store::MemorySystem;
use candor_orchestrator::OrchestratorEngine;

mod agents;
mod chat;
mod pda;
mod routes;
mod stt;
mod tts;
mod util;

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

    /// Voice-activated task via whisper STT (one-shot)
    Voice {
        #[arg(long, help = "Prompt prefix (e.g. 'In Spanish:')")]
        prompt: Option<String>,
        #[arg(long, help = "Record duration in seconds", default_value = "5")]
        duration: u64,
    },

    /// Interactive voice conversation (listen → think → speak → loop)
    VoiceInteractive {
        #[arg(long, help = "Initial prompt prefix")]
        prompt: Option<String>,
        #[arg(long, help = "Record duration in seconds", default_value = "5")]
        duration: u64,
        #[arg(long, help = "Max conversation turns", default_value = "20")]
        max_turns: u32,
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

    /// Personal Digital Assistant — manage your identity, memory, and agents
    Pda {
        #[command(subcommand)]
        action: PdaAction,
    },
}

#[derive(Subcommand, Debug)]
enum PdaAction {
    /// Initialize ~/.candor/ home directory with default files
    Init,
    /// Show PDA status
    Status,
    /// Read or update IDENTITY.md
    Identity {
        #[arg(
            short,
            long,
            help = "New identity content (if not provided, reads current)"
        )]
        set: Option<String>,
    },
    /// Read or update DA_IDENTITY.md
    DaIdentity {
        #[arg(
            short,
            long,
            help = "New DA identity content (if not provided, reads current)"
        )]
        set: Option<String>,
    },
    /// List active work sessions
    Work,
    /// Start a new work session
    WorkStart {
        /// Unique slug for the work session (e.g., "build-pda-dashboard")
        slug: String,
        /// Goal description
        goal: String,
    },
    /// Generate a morning digest (uses LLM for briefing)
    Digest,
    /// Run PDA monitor scan
    Monitor,
}

// ── Auth middleware ──

/// Middleware that checks Authorization: Bearer against CANDOR_API_KEY env var.
/// Skipped if CANDOR_API_KEY is empty or not set, or for GET /api/health.
async fn auth_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> impl axum::response::IntoResponse {
    let api_key = std::env::var("CANDOR_API_KEY").ok();
    let should_check = api_key.as_ref().map_or(false, |k| !k.is_empty());

    if should_check {
        let path = req.uri().path();
        // Allow health check without auth
        if path != "/api/health" {
            let auth_header = req
                .headers()
                .get("Authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            // SAFETY: should_check verified api_key is Some and non-empty
            let key = api_key.as_ref().unwrap();
            let expected = format!("Bearer {key}");
            if auth_header != expected {
                return (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Unauthorized: invalid or missing API key. Set Authorization: Bearer <CANDOR_API_KEY>",
                )
                    .into_response();
            }
        }
    }

    next.run(req).await
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
    let _telemetry =
        candor_telemetry::init_telemetry("candor-daemon", cli.otlp_endpoint.as_deref());

    match cli.command {
        Commands::Task {
            description,
            model,
            openai_base,
            openai_key,
            anthropic_key,
            max_iterations,
            embedding_dim,
        } => {
            println!(
                "{CYAN}{BOLD}   Candor AI v{} — Task Mode{RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            // Auto-initialize PDA home if not set up.
            let _ = pda::init().await;
            let cognitive = build_cognitive(model, openai_key, anthropic_key, openai_base).await?;
            let memory = Arc::new(MemorySystem::new(embedding_dim).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, max_iterations).await?,
            ));
            run_cli_task(description, orch).await?;
        }
        Commands::Chat {
            model,
            openai_base,
            openai_key,
            anthropic_key,
        } => {
            println!(
                "{CYAN}{BOLD}   Candor AI v{} — Chat Mode{RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            let _ = pda::init().await;
            let cognitive = build_cognitive(model, openai_key, anthropic_key, openai_base).await?;
            let memory = Arc::new(MemorySystem::new(384).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, 100).await?,
            ));
            chat::run_chat(orch).await?;
        }
        Commands::Voice { prompt, duration } => {
            println!(
                "{CYAN}{BOLD}   Candor AI v{} — Voice Task (One-Shot){RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            run_voice_task(prompt, duration).await?;
        }
        Commands::VoiceInteractive {
            prompt,
            duration,
            max_turns,
        } => {
            println!(
                "{CYAN}{BOLD}   Candor AI v{} — Voice Interactive{RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            run_voice_interactive(prompt, duration, max_turns).await?;
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
        Commands::Serve {
            port,
            max_iterations,
            embedding_dim,
        } => {
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
                .layer(middleware::from_fn(auth_middleware))
                .layer(CorsLayer::new()
                    .allow_origin(AllowOrigin::list([
                        HeaderValue::from_static("http://localhost:5173"),
                        HeaderValue::from_static("http://localhost:31337"),
                        HeaderValue::from_static("http://127.0.0.1:5173"),
                        HeaderValue::from_static("http://127.0.0.1:31337"),
                        HeaderValue::from_static("tauri://localhost"),
                    ]))
                    .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                    .allow_credentials(true))
                .with_state(state);
            let addr = format!("127.0.0.1:{}", port);
            info!("Candor AI daemon listening on http://{addr}");
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            axum::serve(listener, app).await?;
        }
        Commands::Doctor => {
            run_doctor().await;
        }
        Commands::Pda { action } => {
            println!("{CYAN}{BOLD}   Candor PDA{RESET}\n");
            match action {
                PdaAction::Init => {
                    pda::init()
                        .await
                        .map_err(|e| Box::new(std::io::Error::other(format!("{e}"))))?;
                    println!("  {GREEN}✅ PDA initialized at ~/.candor/{RESET}");
                    println!("  Edit IDENTITY.md and DA_IDENTITY.md to personalize.");
                }
                PdaAction::Status => {
                    let status = pda::status()
                        .await
                        .map_err(|e| Box::new(std::io::Error::other(format!("{e}"))))?;
                    println!("{status}");
                }
                PdaAction::Identity { set } => {
                    if let Some(content) = set {
                        tokio::fs::write(pda::identity_path(), &content).await?;
                        pda::auto_commit("update IDENTITY.md").await.ok();
                        println!("  {GREEN}✅ IDENTITY.md updated{RESET}");
                    } else {
                        let content = pda::read_identity()
                            .await
                            .map_err(|e| Box::new(std::io::Error::other(format!("{e}"))))?;
                        println!("{content}");
                    }
                }
                PdaAction::DaIdentity { set } => {
                    if let Some(content) = set {
                        tokio::fs::write(pda::da_identity_path(), &content).await?;
                        pda::auto_commit("update DA_IDENTITY.md").await.ok();
                        println!("  {GREEN}✅ DA_IDENTITY.md updated{RESET}");
                    } else {
                        let content = pda::read_da_identity()
                            .await
                            .map_err(|e| Box::new(std::io::Error::other(format!("{e}"))))?;
                        println!("{content}");
                    }
                }
                PdaAction::Work => {
                    let slugs = pda::list_work()
                        .await
                        .map_err(|e| Box::new(std::io::Error::other(format!("{e}"))))?;
                    if slugs.is_empty() {
                        println!("  {YELLOW}No active work sessions.{RESET}");
                        println!("  Start one: candor pda work-start <slug> <goal>");
                    } else {
                        println!("{BOLD}Active Work Sessions:{RESET}");
                        for slug in slugs {
                            println!("  • {slug}");
                        }
                    }
                }
                PdaAction::WorkStart { slug, goal } => {
                    pda::start_work(&slug, &goal)
                        .await
                        .map_err(|e| Box::new(std::io::Error::other(format!("{e}"))))?;
                    println!("  {GREEN}✅ Work session '{slug}' started.{RESET}");
                }
                PdaAction::Digest => {
                    let prompt = agents::morning_digest_prompt()
                        .await
                        .map_err(|e| Box::new(std::io::Error::other(e)))?;
                    println!("{BOLD}Generating morning digest…{RESET}");
                    let cognitive = build_cognitive(None, None, None, None).await?;
                    let request = candor_cognitive::LlmRequest {
                        system_prompt: Some(prompt),
                        prompt: "Generate a brief morning digest based on my PDA state.".into(),
                        max_tokens: Some(512),
                        temperature: Some(0.7),
                        stream: false,
                        model_override: None,
                    };
                    match cognitive.generate(&request).await {
                        Ok(response) => {
                            println!("\n{CYAN}{BOLD}Morning Digest{RESET}\n");
                            println!("{response}");
                            // Speak the digest via TTS if available.
                            if tts::is_available() {
                                println!("\n  {YELLOW}🔊 Speaking digest…{RESET}");
                                let _ = tts::speak(&response).await;
                            }
                        }
                        Err(e) => {
                            eprintln!("  {RED}LLM error: {e}{RESET}");
                        }
                    }
                }
                PdaAction::Monitor => {
                    let prompt = agents::monitor_prompt()
                        .await
                        .map_err(|e| Box::new(std::io::Error::other(e)))?;
                    println!("{BOLD}Running PDA monitor scan…{RESET}");
                    let cognitive = build_cognitive(None, None, None, None).await?;
                    let request = candor_cognitive::LlmRequest {
                        system_prompt: Some(prompt),
                        prompt: "Scan my PDA state and suggest actions.".into(),
                        max_tokens: Some(512),
                        temperature: Some(0.5),
                        stream: false,
                        model_override: None,
                    };
                    match cognitive.generate(&request).await {
                        Ok(response) => {
                            println!("\n{CYAN}{BOLD}PDA Monitor{RESET}\n");
                            println!("{response}");
                        }
                        Err(e) => {
                            eprintln!("  {RED}LLM error: {e}{RESET}");
                        }
                    }
                }
            }
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
    model: Option<String>,
    openai_key: Option<String>,
    anthropic_key: Option<String>,
    openai_base: Option<String>,
) -> Result<Arc<CognitiveEngine>, Box<dyn std::error::Error>> {
    use std::env;

    let anthropic_key = anthropic_key.or_else(|| env::var("ANTHROPIC_API_KEY").ok());
    let openai_key = openai_key.or_else(|| env::var("OPENAI_API_KEY").ok());
    let openai_base = openai_base.or_else(|| env::var("OPENAI_BASE_URL").ok());
    let model_name = model.or_else(|| env::var("CANDOR_MODEL").ok());

    let mut backend: Option<Box<dyn candor_cognitive::LlmBackend>> = None;
    let mut label = String::new();

    if let Some(ref key) = anthropic_key {
        let m = model_name
            .clone()
            .unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        backend = Some(Box::new(AnthropicBackend::new(key.clone(), &m)));
        label = format!("anthropic/{m}");
    } else if let Some(ref key) = env::var("DEEPSEEK_API_KEY").ok().as_ref() {
        let m = model_name.clone().unwrap_or_else(|| "deepseek-chat".into());
        backend = Some(Box::new(DeepSeekBackend::new(key.to_string(), &m)));
        label = format!("deepseek/{m}");
    } else if let Some(ref key) = env::var("GEMINI_API_KEY").ok().as_ref() {
        let m = model_name
            .clone()
            .unwrap_or_else(|| "gemini-2.5-flash".into());
        backend = Some(Box::new(GeminiBackend::new(key.to_string(), &m)));
        label = format!("gemini/{m}");
    } else if let Some(ref key) = openai_key {
        let m = model_name.clone().unwrap_or_else(|| "gpt-4o".into());
        backend = Some(Box::new(OpenAiBackend::new(
            key.clone(),
            &m,
            openai_base.clone(),
        )));
        label = if let Some(ref b) = openai_base {
            format!("openai@{b}/{m}")
        } else {
            format!("openai/{m}")
        };
    } else if let Ok(base) = env::var("LM_STUDIO_URL") {
        let m = model_name.clone().unwrap_or_else(|| "local-model".into());
        backend = Some(Box::new(OpenAiBackend::new(
            "lm-studio".into(),
            &m,
            Some(base),
        )));
        label = format!("lm-studio/{m}");
    } else if let Ok(base) = env::var("OLLAMA_URL") {
        let m = model_name.unwrap_or_else(|| "llama3".into());
        backend = Some(Box::new(OpenAiBackend::new(
            "ollama".into(),
            &m,
            Some(base),
        )));
        label = format!("ollama/{m}");
    }

    match backend {
        Some(b) => {
            eprintln!("{GREEN}✓{RESET} {BOLD}LLM:{RESET} {label}");
            Ok(Arc::new(CognitiveEngine::new(Some(b), None).await?))
        }
        None => {
            eprintln!("{YELLOW}⚠ LLM: Not configured — using Mock{RESET}");
            eprintln!(
                "  Set ANTHROPIC_API_KEY, DEEPSEEK_API_KEY, GEMINI_API_KEY, OPENAI_API_KEY, LM_STUDIO_URL, or OLLAMA_URL"
            );
            Ok(Arc::new(
                CognitiveEngine::new(Some(Box::new(MockBackend::new("mock"))), None).await?,
            ))
        }
    }
}

// ── Task runner ──

async fn run_cli_task(
    task: String,
    orch: Arc<tokio::sync::Mutex<OrchestratorEngine>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let isa = IdealStateArtifact {
        id: format!("cli-{}", uuid::Uuid::new_v4()),
        goal: task.clone(),
        acceptance_criteria: vec![],
        constraints: vec![],
        expected_artifacts: vec![],
        phase_requirements: Default::default(),
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

async fn run_voice_task(
    prompt: Option<String>,
    _duration: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = stt::transcribe_mic()
        .await
        .map_err(|e| Box::new(std::io::Error::other(format!("Voice error: {e}"))))?;
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

/// Interactive voice conversation loop.
///
/// For each turn:
///   1. Record audio from microphone (STT)
///   2. Transcribe with whisper-cpp
///   3. Process as a chat message via the cognitive engine
///   4. Speak the response aloud (TTS)
///   5. Loop until the user says "exit", "quit", or max_turns reached
async fn run_voice_interactive(
    initial_prompt: Option<String>,
    duration: u64,
    max_turns: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    // Build the cognitive engine (no sentinel needed for chat).
    let cognitive = build_cognitive(None, None, None, None).await?;

    // Check TTS availability — if not installed, warn but continue.
    let tts_ok = tts::is_available();
    if !tts_ok {
        println!(
            "  {YELLOW}⚠ TTS backend not found. Install piper-tts or espeak-ng for voice responses.{RESET}"
        );
    }

    println!("  {GREEN}Say '{CYAN}exit{GREEN}' or '{CYAN}quit{GREEN}' to stop.{RESET}");
    println!("  {GREEN}Max {max_turns} turns.{RESET}\n");

    let exit_words = ["exit", "quit", "goodbye", "stop", "done"];

    for turn in 1..=max_turns {
        println!("\n  {BOLD}[Turn {turn}/{max_turns}]{RESET}");

        // ── Step 1: Listen ──
        let text = match stt::transcribe_mic_with_duration(duration).await {
            Ok(t) => t,
            Err(stt::SttError::NoSpeech) => {
                println!("  {YELLOW}No speech detected — listening again…{RESET}");
                continue;
            }
            Err(e) => {
                eprintln!("  {RED}STT error: {e}{RESET}");
                println!("  {YELLOW}Type your message instead (or 'exit' to quit):{RESET}");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let input = input.trim().to_string();
                if input.is_empty() {
                    continue;
                }
                input
            }
        };

        let task_text = if let Some(ref p) = initial_prompt {
            format!("{p} {text}")
        } else {
            text.clone()
        };

        println!("  {CYAN}You:{RESET} {text}");

        // Check for exit commands.
        if exit_words.contains(&task_text.to_lowercase().as_str()) {
            println!("  {GREEN}Goodbye!{RESET}");
            if tts_ok {
                let _ = tts::speak("Goodbye!").await;
            }
            break;
        }

        // ── Step 2: Think (generate response via cognitive engine) ──
        println!("  {YELLOW}Thinking…{RESET}");
        let request = candor_cognitive::LlmRequest {
            system_prompt: Some("You are a helpful voice assistant. Keep responses concise and conversational — suitable for being read aloud. Answer in 1-3 sentences when possible.".into()),
            prompt: task_text,
            max_tokens: Some(256),
            temperature: Some(0.7),
            stream: false,
            model_override: None,
        };

        let response = match cognitive.generate(&request).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  {RED}LLM error: {e}{RESET}");
                println!("  {YELLOW}Sorry, I couldn't process that.{RESET}");
                continue;
            }
        };

        println!("  {GREEN}Candor:{RESET} {response}");

        // ── Step 3: Speak ──
        if tts_ok {
            match tts::speak(&response).await {
                Ok(()) => {}
                Err(tts::TtsError::Unavailable) => {
                    // Backend disappeared after initial check — unlikely.
                }
                Err(e) => {
                    eprintln!("  {YELLOW}TTS warning: {e}{RESET}");
                }
            }
        }

        // Small pause between turns.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    println!("\n  {BOLD}Voice session ended.{RESET}");
    Ok(())
}

// ── Init project ──

fn init_project(dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&path)?;
    std::fs::write(
        path.join("candor.toml"),
        "[server]\nhost = \"127.0.0.1\"\nport = 31337\ncheckpoint_dir = \"/tmp/candor-checkpoints\"\nmax_iterations = 100\n\n[sandbox]\nscratchpad_dir = \"/tmp/agent_scratchpad\"\ndefault_timeout_secs = 15\ndefault_memory_mb = 256\n\n[inference]\n# anthropic_api_key = \"sk-ant-...\"\n# openai_api_key = \"sk-...\"\nembedding_model = \"all-MiniLM-L6-v2\"\nembedding_dim = 384\n\n[memory]\nbackend = \"mem\"\ncompaction_token_limit = 135000\n",
    )?;
    std::fs::write(path.join(".gitignore"), "/target/\n.env\n/tmp/\n")?;
    println!(
        "{GREEN}✓{RESET} Project initialized at {BOLD}{}{RESET}",
        path.display()
    );
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
    println!(
        "  {BOLD}LLM:{RESET}      {}",
        if frontier {
            format!("{GREEN}Connected{RESET}")
        } else {
            format!("{YELLOW}Not configured{RESET}")
        }
    );
    println!(
        "  {BOLD}Local:{RESET}    {}",
        if local {
            format!("{GREEN}Connected{RESET}")
        } else {
            format!("{YELLOW}Not configured{RESET}")
        }
    );
    println!(
        "  {BOLD}Sandbox:{RESET}  {}",
        if o.sandbox.native_engine().is_bwrap_available() {
            "Bubblewrap"
        } else {
            "Direct"
        }
    );
    println!(
        "  {BOLD}Sentinel:{RESET} {}",
        if o.sentinel.is_active() {
            format!("{GREEN}Active{RESET}")
        } else {
            format!("{YELLOW}Inactive{RESET}")
        }
    );
    println!(
        "  {BOLD}Tools:{RESET}    {} registered",
        o.tools.tool_count()
    );
    println!();
    println!("{GREEN}  All systems operational.{RESET}");
}

// ── Doctor diagnostics ──

/// Check if a newer version of Candor is available on GitHub.
async fn check_version() -> Option<String> {
    let current = env!("CARGO_PKG_VERSION");
    let url = "https://api.github.com/repos/iknowkungfubar/candor-ai/releases/latest";
    let client = reqwest::Client::builder()
        .user_agent("candor-ai-doctor")
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    let latest = json.get("tag_name")?.as_str()?.trim_start_matches('v');
    if latest != current {
        Some(format!("{current} → {latest}"))
    } else {
        None
    }
}

async fn run_doctor() {
    println!("\n{BOLD}Candor AI — Doctor{RESET}\n");

    let checks = [
        ("cargo", check_cmd("cargo")),
        ("git", check_cmd("git")),
        ("bubblewrap", check_cmd("bwrap")),
        (
            "whisper",
            check_cmd("whisper-cpp") || check_cmd("whisper-cli") || check_cmd("whisper"),
        ),
        ("piper-tts", check_cmd("piper")),
        ("espeak-ng", check_cmd("espeak-ng") || check_cmd("espeak")),
        ("aplay", check_cmd("aplay")),
        ("arecord", check_cmd("arecord")),
        ("PDA home", check_pda()),
        ("surrealDB", true), // embedded, always available
    ];
    let all_ok = checks.iter().all(|(_, ok)| *ok);
    for (name, ok) in &checks {
        println!(
            "  {} {name}",
            if *ok {
                format!("{GREEN}✓{RESET}")
            } else {
                format!("{YELLOW}○{RESET}")
            }
        );
    }
    println!();
    // Version check
    match check_version().await {
        Some(update) => println!("  {YELLOW}⚠ Update available: {update}{RESET}"),
        None => println!(
            "  {GREEN}✓ Up to date (v{}){RESET}",
            env!("CARGO_PKG_VERSION")
        ),
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
        .is_ok()
}

/// Check if PDA home directory is initialized.
fn check_pda() -> bool {
    if let Ok(home) = std::env::var("HOME") {
        std::path::Path::new(&home)
            .join(".candor")
            .join("IDENTITY.md")
            .exists()
    } else {
        false
    }
}
