/// Server configuration.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The daemon configuration, loaded from candor.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Server bind address.
    pub server: ServerConfig,

    /// Sandbox configuration.
    pub sandbox: SandboxConfig,

    /// Inference backends.
    pub inference: InferenceConfig,

    /// Memory/storage.
    pub memory: MemoryConfig,

    /// Sentinel guardrails.
    pub sentinel: SentinelConfig,

    /// Telemetry.
    pub telemetry: TelemetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub checkpoint_dir: PathBuf,
    pub max_iterations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub scratchpad_dir: PathBuf,
    pub wasm_cache_dir: PathBuf,
    pub default_timeout_secs: u64,
    pub default_memory_mb: u64,
    pub default_fuel_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub local_model_path: Option<PathBuf>,
    pub embedding_model: String,
    pub embedding_dim: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub database_path: Option<PathBuf>,
    pub backend: String,
    pub compaction_token_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentinelConfig {
    pub enabled: bool,
    pub semantic_audit_enabled: bool,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub otlp_endpoint: Option<String>,
}

/// Load configuration from a TOML file, with defaults.
pub fn load_config(path: &Path) -> Result<DaemonConfig, Box<dyn std::error::Error>> {
    use figment::{
        Figment,
        providers::{Format, Toml},
    };

    let config: DaemonConfig = Figment::new()
        .merge(Toml::file(path))
        .extract()?;

    Ok(config)
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 31337,
                checkpoint_dir: PathBuf::from("/tmp/candor-checkpoints"),
                max_iterations: 100,
            },
            sandbox: SandboxConfig {
                scratchpad_dir: PathBuf::from("/tmp/agent_scratchpad"),
                wasm_cache_dir: PathBuf::from("/tmp/candor-wasm"),
                default_timeout_secs: 15,
                default_memory_mb: 256,
                default_fuel_limit: 1_000_000,
            },
            inference: InferenceConfig {
                anthropic_api_key: None,
                openai_api_key: None,
                local_model_path: None,
                embedding_model: "all-MiniLM-L6-v2".into(),
                embedding_dim: 384,
            },
            memory: MemoryConfig {
                database_path: None,
                backend: "mem".into(),
                compaction_token_limit: 135_000,
            },
            sentinel: SentinelConfig {
                enabled: true,
                semantic_audit_enabled: true,
                scopes: vec![],
            },
            telemetry: TelemetryConfig {
                enabled: false,
                otlp_endpoint: None,
            },
        }
    }
}
