#![allow(clippy::result_large_err, dead_code)]
/// Candor AI - Lawful Good, Rust-native Agentic Operating System.
///
/// A production-grade agent harness implementing Algorithm v6.3.0.
/// Runs the 7-phase execution loop: Observe->Think->Plan->Build->Execute->Verify->Learn.
mod agents;
mod backend;
mod chat;
mod cli;
mod config;
mod diagnostics;
mod display;
mod pda;
mod project;
mod routes;
mod stt;
mod tts;
mod util;
mod voice;

use std::sync::Arc;

use axum::http::HeaderValue;
use axum::{Router, middleware};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::info;

use candor_core::ideal::IdealStateArtifact;
use candor_memory::store::MemorySystem;
use candor_orchestrator::OrchestratorEngine;

use clap::Parser;
use cli::{auth_middleware, AppState, Cli, Commands, PdaAction};
use display::{BOLD, CYAN, GREEN, YELLOW, RED, RESET};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialise tracing: with OTLP if --otlp-endpoint is set, otherwise fmt.
    let _telemetry = candor_telemetry::init_telemetry("candor-ai", cli.otlp_endpoint.as_deref());

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
                "{CYAN}{BOLD}   Candor AI v{} - Task Mode{RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            let _ = pda::init().await;
            let cognitive = backend::build_cognitive(model, openai_key, anthropic_key, openai_base).await?;
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
                "{CYAN}{BOLD}   Candor AI v{} - Chat Mode{RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            let _ = pda::init().await;
            let cognitive = backend::build_cognitive(model, openai_key, anthropic_key, openai_base).await?;
            let memory = Arc::new(MemorySystem::new(384).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, 100).await?,
            ));
            chat::run_chat(orch).await?;
        }
        Commands::Voice { prompt, duration } => {
            println!(
                "{CYAN}{BOLD}   Candor AI v{} - Voice Task (One-Shot){RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            voice::run_voice_task(prompt, duration).await?;
        }
        Commands::VoiceInteractive {
            prompt,
            duration,
            max_turns,
        } => {
            println!(
                "{CYAN}{BOLD}   Candor AI v{} - Voice Interactive{RESET}\n",
                env!("CARGO_PKG_VERSION")
            );
            voice::run_voice_interactive(prompt, duration, max_turns).await?;
        }
        Commands::Init { path } => {
            project::init_project(&path)?;
        }
        Commands::Health => {
            let cognitive = backend::build_cognitive(None, None, None, None).await?;
            let memory = Arc::new(MemorySystem::new(384).await?);
            let orch = Arc::new(tokio::sync::Mutex::new(
                OrchestratorEngine::new(cognitive, memory, 100).await?,
            ));
            diagnostics::run_health_check(orch).await;
        }
        Commands::Serve {
            port,
            max_iterations,
            embedding_dim,
        } => {
            let cognitive = backend::build_cognitive(None, None, None, None).await?;
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
                .layer(
                    CorsLayer::new()
                        .allow_origin(AllowOrigin::list([
                            HeaderValue::from_static("http://localhost:5173"),
                            HeaderValue::from_static("http://localhost:31337"),
                            HeaderValue::from_static("http://127.0.0.1:5173"),
                            HeaderValue::from_static("http://127.0.0.1:31337"),
                            HeaderValue::from_static("tauri://localhost"),
                        ]))
                        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                        .allow_credentials(true),
                )
                .with_state(state);
            let addr = format!("127.0.0.1:{port}");
            info!("Candor AI daemon listening on http://{addr}");
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            axum::serve(listener, app).await?;
        }
        Commands::Doctor => {
            diagnostics::run_doctor().await;
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
                            println!("  . {slug}");
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
                    println!("{BOLD}Generating morning digest...{RESET}");
                    let cognitive = backend::build_cognitive(None, None, None, None).await?;
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
                            if tts::is_available() {
                                println!("\n  {YELLOW}🔊 Speaking digest...{RESET}");
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
                    println!("{BOLD}Running PDA monitor scan...{RESET}");
                    let cognitive = backend::build_cognitive(None, None, None, None).await?;
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

// -- Task runner --

/// Execute a one-shot CLI task through the orchestrator engine.
pub async fn run_cli_task(
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
                println!("  {GREEN}->{RESET} {event}");
            }
        }
        Err(e) => {
            eprintln!("\n{RED}✗ Task failed: {e}{RESET}");
            std::process::exit(1);
        }
    }
    Ok(())
}