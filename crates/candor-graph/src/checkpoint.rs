/// Checkpoint system for durable graph state persistence.
///
/// Persists AgentState after every node transition so that
/// long-running agents can pause, await human approval, and
/// resume without losing context.
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use candor_core::error::CoreError;
use candor_core::state::AgentState;

/// A checkpoint backend that persists state to disk.
pub struct CheckpointManager {
    checkpoint_dir: PathBuf,
    max_checkpoints: usize,
}

impl CheckpointManager {
    pub fn new(checkpoint_dir: PathBuf, max_checkpoints: usize) -> Self {
        Self {
            checkpoint_dir,
            max_checkpoints,
        }
    }

    /// Save the current agent state as a JSON checkpoint file.
    pub async fn save(&self, state: Arc<Mutex<AgentState>>) -> Result<PathBuf, CoreError> {
        let state = state.lock().await;

        tokio::fs::create_dir_all(&self.checkpoint_dir)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        let filename = format!(
            "checkpoint-{}-{}.json",
            chrono::Utc::now().format("%Y%m%dT%H%M%S"),
            state.iteration_count
        );

        let path = self.checkpoint_dir.join(&filename);
        let json = serde_json::to_string_pretty(&*state)
            .map_err(|e| CoreError::Serialization(e.to_string()))?;

        tokio::fs::write(&path, json)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        tracing::info!(path = %path.display(), "Checkpoint saved");
        Ok(path)
    }

    /// Load the latest checkpoint from disk.
    pub async fn load_latest(&self, state: Arc<Mutex<AgentState>>) -> Result<bool, CoreError> {
        // Collect checkpoint file names, sorted by name (timestamps are in filenames).
        let mut read_dir = tokio::fs::read_dir(&self.checkpoint_dir)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        let mut entries = Vec::new();
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?
        {
            let name = entry.file_name();
            if let Some(name_str) = name.to_str()
                && name_str.ends_with(".json")
            {
                entries.push(entry);
            }
        }

        // Sort by filename descending (latest first).
        entries.sort_by_key(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.to_string())
                .unwrap_or_default()
        });
        entries.reverse();

        match entries.first() {
            Some(entry) => {
                let json = tokio::fs::read_to_string(entry.path())
                    .await
                    .map_err(|e| CoreError::Io(e.to_string()))?;

                let loaded: AgentState = serde_json::from_str(&json)
                    .map_err(|e| CoreError::Serialization(e.to_string()))?;

                let mut state = state.lock().await;
                *state = loaded;

                tracing::info!("Checkpoint restored from {}", entry.path().display());
                Ok(true)
            }
            None => {
                tracing::info!("No checkpoint found — starting fresh.");
                Ok(false)
            }
        }
    }

    /// Prune old checkpoints keeping only `max_checkpoints` latest.
    pub async fn prune(&self) -> Result<(), CoreError> {
        let mut read_dir = tokio::fs::read_dir(&self.checkpoint_dir)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        let mut entries = Vec::new();
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?
        {
            entries.push(entry);
        }

        // Sort by modification time ascending.
        entries.sort_by_key(|e| {
            std::fs::metadata(e.path())
                .ok()
                .and_then(|m| m.modified().ok())
        });

        let to_remove = entries.len().saturating_sub(self.max_checkpoints);
        for entry in entries.iter().take(to_remove) {
            tokio::fs::remove_file(entry.path())
                .await
                .map_err(|e| CoreError::Io(e.to_string()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_save_and_load_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = CheckpointManager::new(dir.path().to_path_buf(), 5);
        let state = Arc::new(Mutex::new(AgentState::default()));

        // Modify state.
        {
            let mut s = state.lock().await;
            s.active_task = "test task".into();
            s.iteration_count = 42;
        }

        // Save.
        let path = mgr.save(Arc::clone(&state)).await.unwrap();
        assert!(path.exists());

        // Reset state and load.
        {
            let mut s = state.lock().await;
            s.active_task = String::new();
            s.iteration_count = 0;
        }

        let loaded = mgr.load_latest(Arc::clone(&state)).await.unwrap();
        assert!(loaded);

        {
            let s = state.lock().await;
            assert_eq!(s.active_task, "test task");
            assert_eq!(s.iteration_count, 42);
        }
    }
}
