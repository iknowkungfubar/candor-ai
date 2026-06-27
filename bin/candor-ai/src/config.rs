use figment::Figment;
/// Configuration module — reads `candor.toml` using Figment.
///
/// Sources (earlier sources have lower priority):
/// 1. `./candor.toml` (project-local)
/// 2. `~/.candor/config.toml` (user-global)
/// 3. Environment variables prefixed with `CANDOR_` (highest priority)
///
/// Environment variables override TOML values.  The `CANDOR_` prefix is
/// stripped and the remainder is lowercased to match TOML key names.
/// Nested keys use `__` as separator, e.g. `CANDOR_SERVER__PORT=9090`.
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;
use std::path::PathBuf;

/// Top-level configuration structure mirroring `candor.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct CandorConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub sandbox: SandboxConfig,
    #[serde(default)]
    pub inference: InferenceConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_checkpoint_dir")]
    pub checkpoint_dir: String,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct SandboxConfig {
    #[serde(default = "default_scratchpad_dir")]
    pub scratchpad_dir: String,
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,
    #[serde(default = "default_memory_mb")]
    pub default_memory_mb: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct InferenceConfig {
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    #[serde(default = "default_embedding_dim")]
    pub embedding_dim: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct MemoryConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_compaction_token_limit")]
    pub compaction_token_limit: usize,
}

// ── Default helpers ──

#[allow(dead_code)]
fn default_host() -> String {
    "127.0.0.1".into()
}
#[allow(dead_code)]
fn default_port() -> u16 {
    31337
}
#[allow(dead_code)]
fn default_checkpoint_dir() -> String {
    "/tmp/candor-checkpoints".into()
}
#[allow(dead_code)]
fn default_max_iterations() -> u32 {
    100
}
#[allow(dead_code)]
fn default_scratchpad_dir() -> String {
    "/tmp/agent_scratchpad".into()
}
#[allow(dead_code)]
fn default_timeout_secs() -> u64 {
    15
}
#[allow(dead_code)]
fn default_memory_mb() -> u64 {
    256
}
#[allow(dead_code)]
fn default_embedding_model() -> String {
    "all-MiniLM-L6-v2".into()
}
#[allow(dead_code)]
fn default_embedding_dim() -> usize {
    384
}
#[allow(dead_code)]
fn default_backend() -> String {
    "mem".into()
}
#[allow(dead_code)]
fn default_compaction_token_limit() -> usize {
    135_000
}

/// Load configuration from candor.toml sources and environment variables.
///
/// Search order:
/// 1. `./candor.toml` (lowest priority)
/// 2. `~/.candor/config.toml`
/// 3. Environment variables prefixed with `CANDOR_` (highest priority)
#[allow(dead_code, clippy::result_large_err)]
pub fn load_config() -> Result<CandorConfig, figment::Error> {
    let home_config = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".candor").join("config.toml"))
        .filter(|p| p.exists());

    let local_config = if PathBuf::from("./candor.toml").exists() {
        Some(PathBuf::from("./candor.toml"))
    } else {
        None
    };

    let mut figment = Figment::new();

    // Push sources in priority order (last wins)
    if let Some(path) = &local_config {
        figment = figment.merge(Toml::file(path));
    }
    if let Some(path) = &home_config {
        figment = figment.merge(Toml::file(path));
    }
    figment = figment.merge(Env::prefixed("CANDOR_").split("__"));

    figment.extract()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CandorConfig::default();
        assert_eq!(config.server.port, 31337);
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.inference.embedding_dim, 384);
        assert_eq!(config.memory.backend, "mem");
    }

    #[test]
    fn test_load_config_defaults() {
        // Should not panic even without candor.toml present
        let config = load_config().unwrap_or_default();
        assert_eq!(config.server.port, 31337);
    }
}
