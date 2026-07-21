use std::sync::Arc;

use axum::response::IntoResponse;
use axum::middleware;
use clap::{Parser, Subcommand};

use candor_orchestrator::OrchestratorEngine;

/// Candor AI — Lawful Good, Rust-native Agentic Operating System.
#[derive(Parser, Debug)]
#[command(name = "candor", version, about = "Lawful Good Rust Agentic Operating System", long_about = None)]
pub struct Cli {
    /// OpenTelemetry OTLP gRPC endpoint for trace export
    #[arg(long, global = true, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    pub otlp_endpoint: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a one-shot agent task
    Task {
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
        #[arg(long, help = "Prompt prefix")]
        prompt: Option<String>,
        #[arg(long, help = "Record duration in seconds", default_value = "5")]
        duration: u64,
    },
    /// Interactive voice conversation
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
    /// Personal Digital Assistant
    Pda {
        #[command(subcommand)]
        action: PdaAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum PdaAction {
    /// Initialize ~/.candor/ home directory
    Init,
    /// Show PDA status
    Status,
    /// Read or update IDENTITY.md
    Identity {
        #[arg(short, long)]
        set: Option<String>,
    },
    /// Read or update DA_IDENTITY.md
    DaIdentity {
        #[arg(short, long)]
        set: Option<String>,
    },
    /// List active work sessions
    Work,
    /// Start a new work session
    WorkStart { slug: String, goal: String },
    /// Generate a morning digest
    Digest,
    /// Run PDA monitor scan
    Monitor,
}

// -- Auth middleware --

/// Middleware that checks Authorization: Bearer against CANDOR_API_KEY env var.
pub async fn auth_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> impl axum::response::IntoResponse {
    let api_key = std::env::var("CANDOR_API_KEY").ok();

    if let Some(key) = api_key {
        if key.is_empty() {
            return next.run(req).await;
        }

        let path = req.uri().path();
        if path != "/api/health" {
            let auth_header = req
                .headers()
                .get("Authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

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