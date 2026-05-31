use candor_cognitive::deterministic_embed;
/// Phase 4 integration tests: context compaction + persistence.
use candor_core::state::AgentState;
use candor_memory::schema::schema_queries;
use candor_memory::store::MemorySystem;

// ── SurrealDB kv-mem initialization ──

#[tokio::test]
async fn test_surrealdb_kv_mem_initializes() {
    let memory = MemorySystem::new(384).await;
    assert!(memory.is_ok());
    let memory = memory.unwrap();
    assert_eq!(memory.embedding_dim(), 384);
}

#[tokio::test]
async fn test_memory_store_and_retrieve() {
    let memory = MemorySystem::new(384).await.unwrap();
    let embedding = vec![0.1; 384];

    memory
        .store_memory("test-proj".into(), "test content".into(), embedding.clone())
        .await
        .unwrap();

    let results = memory
        .retrieve_context("test-proj", embedding, 5)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "test content");
}

#[tokio::test]
async fn test_project_isolation() {
    let memory = MemorySystem::new(384).await.unwrap();

    let emb_a = vec![0.1; 384];
    let emb_b = vec![0.2; 384];

    memory
        .store_memory("proj-a".into(), "A data".into(), emb_a.clone())
        .await
        .unwrap();
    memory
        .store_memory("proj-b".into(), "B data".into(), emb_b.clone())
        .await
        .unwrap();

    let results = memory.retrieve_context("proj-a", emb_a, 10).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "A data");
}

#[tokio::test]
async fn test_delete_project_memories() {
    let memory = MemorySystem::new(384).await.unwrap();
    let emb = vec![0.1; 384];

    memory
        .store_memory("temp".into(), "temp data".into(), emb.clone())
        .await
        .unwrap();
    memory.delete_project_memories("temp").await.unwrap();

    let results = memory.retrieve_context("temp", emb, 5).await.unwrap();
    assert!(results.is_empty());
}

// ── Embedding (fastembed-compatible) ──

#[test]
fn test_embedding_384_dimensions() {
    let v = deterministic_embed("hello world", 384);
    assert_eq!(v.len(), 384);
}

#[test]
fn test_embedding_idempotent() {
    let a = deterministic_embed("test", 384);
    let b = deterministic_embed("test", 384);
    assert_eq!(a, b);
}

#[test]
fn test_embedding_different_inputs() {
    let a = deterministic_embed("rust programming", 384);
    let b = deterministic_embed("cake recipes", 384);
    assert_ne!(a, b);
}

#[test]
fn test_embedding_empty_input() {
    let v = deterministic_embed("", 384);
    assert_eq!(v, vec![0.0; 384]);
}

// ── 135K Token Limit + Auto-Compaction ──

#[test]
fn test_token_limit_breached() {
    let mut state = AgentState::default();
    state.estimated_token_count = 135_000;
    assert!(state.is_over_token_limit());

    state.estimated_token_count = 100_000;
    assert!(!state.is_over_token_limit());
}

#[test]
fn test_auto_compaction() {
    let mut state = AgentState::default();
    for i in 0..100 {
        state.append_message(&format!(
            "message number {i} with padding to make it longer and more realistic"
        ));
    }
    let original_count = state.message_history.len();

    state.compact_context(500);
    assert!(state.message_history.len() < original_count);
    assert!(!state.compaction_required);
}

#[test]
fn test_execution_log_storage() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let memory = MemorySystem::new(384).await.unwrap();
        memory
            .store_execution_log("session-1", "build", "cargo build", "ok")
            .await
            .unwrap();
        // No assertion needed — just verify no panic
    });
}

// ── Schema validation ──

#[test]
fn test_schema_queries_contain_tables() {
    let queries = schema_queries(384);
    assert!(queries.contains("memory_block"));
    assert!(queries.contains("project"));
    assert!(queries.contains("execution_log"));
    assert!(queries.contains("HNSW"));
    assert!(queries.contains("DIMENSION 384"));
}
