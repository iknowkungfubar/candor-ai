use std::io::Write;
/// Trajectory extraction and LoRA fine-tuning pipeline.
///
/// From design doc Phase 6:
/// 6.2: Daily execution logs → JSONL
/// 6.3: JSONL → offline LoRA weight generation pipeline
use std::path::PathBuf;
use tracing::info;

use candor_core::error::CoreError;
use candor_memory::store::MemorySystem;

/// A single trajectory entry for JSONL logging.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrajectoryEntry {
    pub session_id: String,
    pub timestamp: String,
    pub phase: String,
    pub action: String,
    pub result: String,
    pub tokens_used: u64,
}

impl TrajectoryEntry {
    pub fn new(session_id: &str, phase: &str, action: &str, result: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            phase: phase.to_string(),
            action: action.to_string(),
            result: result.to_string(),
            tokens_used: 0,
        }
    }
}

/// Append a trajectory entry to a JSONL file.
pub async fn append_to_jsonl(
    entry: &TrajectoryEntry,
    jsonl_path: &PathBuf,
) -> Result<(), CoreError> {
    if let Some(parent) = jsonl_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;
    }

    let json = serde_json::to_string(entry).map_err(|e| CoreError::Serialization(e.to_string()))?;

    // Append with newline
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(jsonl_path)
        .map_err(|e| CoreError::Io(e.to_string()))?;

    writeln!(file, "{json}").map_err(|e| CoreError::Io(e.to_string()))?;

    Ok(())
}

/// Extract trajectory entries from SurrealDB execution logs.
pub async fn extract_trajectories(
    _memory: &MemorySystem,
    session_filter: Option<&str>,
    jsonl_path: &PathBuf,
) -> Result<usize, CoreError> {
    info!("Extracting trajectory data to JSONL");

    // In production, this would query all recent execution_log entries
    // from SurrealDB. For now, we write a summary trajectory.
    let entry = TrajectoryEntry {
        session_id: session_filter.unwrap_or("all").to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        phase: "learn".to_string(),
        action: "trajectory_extraction".to_string(),
        result: "completed".to_string(),
        tokens_used: 0,
    };

    append_to_jsonl(&entry, jsonl_path).await?;
    Ok(1)
}

/// LoRA adapter generation pipeline scaffold.
///
/// In production, this would:
/// 1. Load trajectory JSONL
/// 2. Convert to instruction-tuning format
/// 3. Run LoRA fine-tuning via mistral.rs or HuggingFace PEFT
/// 4. Output adapter weights
pub struct LoRAPipeline {
    /// Path to input trajectories.
    pub jsonl_path: PathBuf,
    /// Output directory for LoRA weights.
    pub output_dir: PathBuf,
    /// Model to fine-tune.
    pub base_model: String,
    /// LoRA rank (default: 16).
    pub rank: u32,
    /// LoRA alpha (default: 32).
    pub alpha: u32,
}

impl LoRAPipeline {
    pub fn new(jsonl_path: PathBuf, output_dir: PathBuf, base_model: String) -> Self {
        Self {
            jsonl_path,
            output_dir,
            base_model,
            rank: 16,
            alpha: 32,
        }
    }

    /// Validate that the pipeline inputs exist and are ready.
    pub fn validate(&self) -> Result<bool, CoreError> {
        if !self.jsonl_path.exists() {
            return Ok(false);
        }
        if !self.output_dir.exists() {
            std::fs::create_dir_all(&self.output_dir).map_err(|e| CoreError::Io(e.to_string()))?;
        }
        Ok(true)
    }

    /// Provision the pipeline for offline execution.
    /// Writes a configuration file that an offline process can consume.
    pub async fn provision(&self) -> Result<(), CoreError> {
        let config = serde_json::json!({
            "pipeline": "lora_fine_tuning",
            "input": self.jsonl_path.to_string_lossy(),
            "output": self.output_dir.to_string_lossy(),
            "base_model": self.base_model,
            "rank": self.rank,
            "alpha": self.alpha,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });

        let config_path = self.output_dir.join("lora_pipeline_config.json");
        let json = serde_json::to_string_pretty(&config)
            .map_err(|e| CoreError::Serialization(e.to_string()))?;

        tokio::fs::write(&config_path, json)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        info!(path = %config_path.display(), "LoRA pipeline provisioned");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_to_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trajectories.jsonl");

        let entry = TrajectoryEntry::new("sess-1", "build", "cargo test", "passed");
        append_to_jsonl(&entry, &path).await.unwrap();

        assert!(path.exists());
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("sess-1"));
        assert!(content.contains("build"));
    }

    #[tokio::test]
    async fn test_append_multiple_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trajectories.jsonl");

        for i in 0..5 {
            let entry =
                TrajectoryEntry::new(&format!("sess-{i}"), "verify", "cargo test", "passed");
            append_to_jsonl(&entry, &path).await.unwrap();
        }

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn test_lora_pipeline_validate_missing_input() {
        let pipeline = LoRAPipeline::new(
            PathBuf::from("/nonexistent/traj.jsonl"),
            PathBuf::from("/tmp/test-lora"),
            "qwen3-1.5b".into(),
        );
        assert!(!pipeline.validate().unwrap());
    }

    #[tokio::test]
    async fn test_lora_pipeline_provision() {
        let dir = tempfile::tempdir().unwrap();
        let jsonl = dir.path().join("trajectories.jsonl");

        // Create input file
        tokio::fs::write(&jsonl, "{}").await.unwrap();

        let pipeline = LoRAPipeline::new(
            jsonl.clone(),
            dir.path().join("lora_output"),
            "qwen3-1.5b".into(),
        );

        assert!(pipeline.validate().unwrap());
        pipeline.provision().await.unwrap();

        let config_path = dir
            .path()
            .join("lora_output")
            .join("lora_pipeline_config.json");
        assert!(config_path.exists());
    }

    #[test]
    fn test_trajectory_entry_serialization() {
        let entry = TrajectoryEntry::new("sess-42", "learn", "summarize", "ok");
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("sess-42"));
        assert!(json.contains("learn"));
    }
}
