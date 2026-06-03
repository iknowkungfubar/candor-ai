/// Memory Nudge — background knowledge consolidation cron task.
///
/// From the design doc:
/// "Agent-Curated Memory Nudges: The daemon actively prompts the agent during
/// idle periods to consolidate knowledge. It forces the agent to read daily
/// execution logs and extract persistent facts about the user's workflow into
/// a centralized, cross-session memory database."
///
/// This module:
/// 1. Queries the MemorySystem for recent execution_log entries
/// 2. Groups them by session_id and summarizes them
/// 3. Stores each summary as a memory_block with a deterministic embedding
/// 4. Purges old log entries after summarization
use std::collections::HashMap;

use candor_core::error::CoreError;
use candor_memory::store::{ExecutionLogEntry, MemorySystem};
use tracing::{info, instrument};

/// Run a single memory nudge cycle.
///
/// 1. Fetches all execution_log entries from SurrealDB
/// 2. Groups them by session_id
/// 3. Generates a structured text summary per session
/// 4. Stores each summary as a memory_block with a hash-based embedding
/// 5. Purges all execution_log entries after summarization
///
/// Returns the number of summaries generated (i.e., unique sessions found).
#[instrument(skip(memory))]
pub async fn run_nudge(memory: &MemorySystem) -> Result<usize, CoreError> {
    info!("Memory nudge: starting knowledge consolidation cycle");

    // Step 1: Fetch all execution logs
    let logs = memory.get_all_execution_logs().await?;

    if logs.is_empty() {
        info!("Memory nudge: no execution logs to consolidate");
        return Ok(0);
    }

    info!(
        log_count = logs.len(),
        "Memory nudge: fetched execution logs"
    );

    // Step 2: Group by session_id
    let mut grouped: HashMap<String, Vec<ExecutionLogEntry>> = HashMap::new();
    for log in &logs {
        grouped
            .entry(log.session_id.clone())
            .or_default()
            .push(log.clone());
    }

    let mut summaries_generated = 0;

    // Step 3 & 4: Summarize each session and store as memory_block
    for (session_id, session_logs) in &grouped {
        let summary = build_session_summary(session_id, session_logs);
        let dim = memory.embedding_dim();
        let embedding = derive_embedding(&summary, dim);

        memory
            .store_memory("default".into(), summary, embedding)
            .await?;

        summaries_generated += 1;
    }

    // Step 5: Purge old log entries
    if !grouped.is_empty() {
        memory.delete_all_execution_logs().await?;
        info!(
            "Memory nudge: purged {} log entries after summarization",
            logs.len()
        );
    }

    info!(
        summaries_generated,
        "Memory nudge: knowledge consolidation complete"
    );
    Ok(summaries_generated)
}

/// Build a structured textual summary for a single session's logs.
///
/// The summary contains the session ID, total action count, and a
/// chronological log of each action grouped by phase.
fn build_session_summary(session_id: &str, logs: &[ExecutionLogEntry]) -> String {
    let mut lines = Vec::new();

    lines.push(format!("=== Session Summary: {} ===", session_id));
    lines.push(format!("Total execution log entries: {}", logs.len()));
    lines.push(String::new());

    // Group by phase within the session
    let mut by_phase: HashMap<&str, Vec<&ExecutionLogEntry>> = HashMap::new();
    for log in logs {
        by_phase.entry(log.phase.as_str()).or_default().push(log);
    }

    let mut phases: Vec<&&str> = by_phase.keys().collect();
    phases.sort();

    for phase in phases {
        // SAFETY: phase key comes from by_phase.keys() just above, so it must exist.
        let entries = &by_phase[phase];
        lines.push(format!("[Phase: {}] {} action(s)", phase, entries.len()));
        for entry in entries {
            lines.push(format!("  action: {} -> {}", entry.action, entry.result));
        }
        lines.push(String::new());
    }

    // Overall findings
    let action_count = logs.len();
    let phases_count = by_phase.len();
    lines.push(format!(
        "Session executed {} actions across {} phases.",
        action_count, phases_count
    ));

    lines.join("\n")
}

/// Generate a deterministic embedding vector from text.
///
/// Uses the text's hash as a seed to produce `dim` floats in [-0.5, 0.5].
/// This is not a real semantic embedding — it's a placeholder that produces
/// deterministic, content-dependent vectors. In production the LLM-based
/// embedding pipeline from the CognitiveEngine should be used instead.
fn derive_embedding(text: &str, dim: usize) -> Vec<f32> {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    let seed = hasher.finish();

    (0..dim)
        .map(|i| {
            let mixed = seed.wrapping_mul(i as u64 + 1).wrapping_add(seed >> 32);
            (mixed as f32) / (u64::MAX as f32) - 0.5
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use surrealdb::types::Datetime;

    /// Helper to create a log entry for tests.
    fn make_log(session: &str, phase: &str, action: &str, result: &str) -> ExecutionLogEntry {
        ExecutionLogEntry {
            session_id: session.to_string(),
            phase: phase.to_string(),
            action: action.to_string(),
            result: result.to_string(),
            timestamp: Datetime::now(),
        }
    }

    // ── Unit tests (no SurrealDB dependency) ──

    #[test]
    fn test_build_session_summary_single_session() {
        let logs = vec![
            make_log("sess-1", "observe", "scan_files", "3 files found"),
            make_log("sess-1", "think", "analyze", "identified bug pattern"),
            make_log("sess-1", "build", "cargo build", "compiled successfully"),
        ];

        let summary = build_session_summary("sess-1", &logs);

        assert!(summary.contains("Session Summary: sess-1"));
        assert!(summary.contains("3 actions"));
        assert!(summary.contains("observe"));
        assert!(summary.contains("think"));
        assert!(summary.contains("build"));
        assert!(summary.contains("scan_files"));
        assert!(summary.contains("cargo build"));
    }

    #[test]
    fn test_build_session_summary_empty_logs() {
        let logs: Vec<ExecutionLogEntry> = vec![];

        let summary = build_session_summary("sess-empty", &logs);

        assert!(summary.contains("Session Summary: sess-empty"));
        assert!(summary.contains("execution log entries: 0"));
        assert!(summary.contains("0 actions across 0 phases"));
    }

    #[test]
    fn test_build_session_summary_multiple_phases_sorted() {
        let logs = vec![
            make_log("sess-2", "verify", "run tests", "2 passed"),
            make_log("sess-2", "build", "cargo check", "no errors"),
            make_log("sess-2", "observe", "read_file", "src/main.rs"),
        ];

        let summary = build_session_summary("sess-2", &logs);

        // Phases should be alphabetically sorted: build, observe, verify
        let build_pos = summary.find("[Phase: build]").unwrap();
        let observe_pos = summary.find("[Phase: observe]").unwrap();
        let verify_pos = summary.find("[Phase: verify]").unwrap();

        assert!(build_pos < observe_pos, "build should come before observe");
        assert!(
            observe_pos < verify_pos,
            "observe should come before verify"
        );
        assert!(summary.contains("3 actions across 3 phases"));
    }

    #[test]
    fn test_derive_embedding_deterministic() {
        let text = "session-abc observed 5 files and built successfully";

        let emb1 = derive_embedding(text, 384);
        let emb2 = derive_embedding(text, 384);

        assert_eq!(emb1.len(), 384);
        assert_eq!(emb2.len(), 384);
        assert_eq!(emb1, emb2, "embeddings must be deterministic");
    }

    #[test]
    fn test_derive_embedding_different_inputs_differ() {
        let emb_a = derive_embedding("session alpha completed", 64);
        let emb_b = derive_embedding("session beta completed", 64);

        assert_ne!(
            emb_a, emb_b,
            "different inputs should produce different embeddings"
        );
    }

    #[test]
    fn test_derive_embedding_dimension() {
        for dim in [64, 128, 384] {
            let emb = derive_embedding("test", dim);
            assert_eq!(emb.len(), dim, "embedding dimension mismatch for dim={dim}");
        }
    }

    // ── Integration tests (with real SurrealDB in-memory) ──

    #[tokio::test]
    async fn test_run_nudge_empty_db() {
        let memory = MemorySystem::new(64).await.unwrap();
        let count = run_nudge(&memory).await.unwrap();
        assert_eq!(count, 0, "no summaries for empty execution_log");
    }

    #[tokio::test]
    async fn test_run_nudge_with_logs() {
        let memory = MemorySystem::new(64).await.unwrap();

        // Insert a few log entries
        memory
            .store_execution_log("sess-alpha", "observe", "list_dir", "5 entries")
            .await
            .unwrap();
        memory
            .store_execution_log("sess-alpha", "build", "cargo test", "all passed")
            .await
            .unwrap();
        memory
            .store_execution_log("sess-beta", "think", "design review", "approved")
            .await
            .unwrap();

        let count = run_nudge(&memory).await.unwrap();
        assert_eq!(
            count, 2,
            "should generate 2 summaries (sess-alpha, sess-beta)"
        );

        // Verify logs were purged
        let remaining = memory.get_all_execution_logs().await.unwrap();
        assert!(
            remaining.is_empty(),
            "execution logs should be purged after nudge"
        );

        // Verify memory blocks were stored — we can't directly query memory_block
        // without a retrieval method, but run_nudge returned Ok(2) so storage succeeded.
    }

    #[tokio::test]
    async fn test_run_nudge_idempotent() {
        let memory = MemorySystem::new(64).await.unwrap();

        memory
            .store_execution_log("sess-1", "verify", "check output", "OK")
            .await
            .unwrap();

        // First nudge
        let count1 = run_nudge(&memory).await.unwrap();
        assert_eq!(count1, 1);

        // Second nudge on empty DB
        let count2 = run_nudge(&memory).await.unwrap();
        assert_eq!(count2, 0, "no logs remain after first nudge");
    }

    #[tokio::test]
    async fn test_run_nudge_multi_session_multi_phase() {
        let memory = MemorySystem::new(128).await.unwrap();

        // Session 1: 4 entries across 3 phases
        memory
            .store_execution_log("multi-1", "observe", "scan", "found 10")
            .await
            .unwrap();
        memory
            .store_execution_log("multi-1", "think", "plan", "decided refactor")
            .await
            .unwrap();
        memory
            .store_execution_log("multi-1", "build", "cargo fix", "applied 3 changes")
            .await
            .unwrap();
        memory
            .store_execution_log("multi-1", "verify", "cargo test", "42 passed")
            .await
            .unwrap();

        // Session 2: 2 entries
        memory
            .store_execution_log("multi-2", "observe", "read config", "parsed")
            .await
            .unwrap();
        memory
            .store_execution_log("multi-2", "build", "apply config", "done")
            .await
            .unwrap();

        // Session 3: 1 entry
        memory
            .store_execution_log("multi-3", "think", "research", "found docs")
            .await
            .unwrap();

        let count = run_nudge(&memory).await.unwrap();
        assert_eq!(count, 3, "should generate 3 summaries for 3 sessions");

        // Verify cleanup
        let remaining = memory.get_all_execution_logs().await.unwrap();
        assert!(remaining.is_empty());
    }
}
