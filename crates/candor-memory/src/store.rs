/// Unified memory storage engine with SurrealDB.
///
/// From the design doc: "Utilizing the surrealdb crate with the kv-mem
/// feature allows the harness to embed the database directly into the
/// binary. This eliminates the need for external middleware."
///
/// "The database schemas define project-scoped memory isolation, ensuring
/// that disparate agent tasks do not contaminate each other's context retrieval."
use serde::{Deserialize, Serialize};
use tracing::{error, info, instrument};

use candor_core::error::CoreError;

/// Represents a single discrete unit of memory inside the vector database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBlock {
    pub project_id: String,
    pub textual_content: String,
    pub semantic_embedding: Vec<f32>,
    pub timestamp: surrealdb::sql::Datetime,
}

/// The unified storage engine managing document and vector data.
pub struct MemorySystem {
    /// Embedded SurrealDB instance.
    db: surrealdb::Surreal<surrealdb::engine::local::Db>,

    /// Dimension of the embedding vectors in use.
    embedding_dim: usize,
}

impl MemorySystem {
    /// Create a new in-memory MemorySystem.
    ///
    /// Uses `surrealdb::engine::local::Mem` for zero-dependency operation.
    /// In production, switch to `surrealdb::engine::local::RocksDb`.
    pub async fn new(embedding_dim: usize) -> Result<Self, CoreError> {
        info!("Initializing embedded SurrealDB memory engine");

        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .map_err(|e| {
                CoreError::Internal(format!("SurrealDB init failed: {e}"))
            })?;

        db.use_ns("candor_namespace")
            .use_db("candor_database")
            .await
            .map_err(|e| CoreError::Internal(format!("SurrealDB namespace/db error: {e}")))?;

        // Run schema queries with the correct embedding dimension.
        let schema_queries = super::schema::schema_queries(embedding_dim);
        let mut query_response =
            db.query(&schema_queries).await.map_err(|e| {
                CoreError::Internal(format!("Schema query failed: {e}"))
            })?;

        if !query_response.take_errors().is_empty() {
            error!("Schema definition errors encountered");
            return Err(CoreError::Internal(
                "Database schema initialization failure — check embedding dimension".into(),
            ));
        }

        info!("Memory engine initialized successfully");

        Ok(Self {
            db,
            embedding_dim,
        })
    }

    /// Store a new memory block with its embedding vector.
    #[instrument(skip(self, embedding))]
    pub async fn store_memory(
        &self,
        project_id: String,
        content: String,
        embedding: Vec<f32>,
    ) -> Result<(), CoreError> {
        let memory_entry = MemoryBlock {
            project_id,
            textual_content: content,
            semantic_embedding: embedding,
            timestamp: surrealdb::sql::Datetime::default(),
        };

        let _created: Option<MemoryBlock> = self
            .db
            .create("memory_block")
            .content(memory_entry)
            .await
            .map_err(|e| CoreError::Internal(format!("Store memory failed: {e}")))?;

        info!("Memory block successfully persisted to database");
        Ok(())
    }

    /// Retrieve context blocks nearest to the query embedding.
    ///
    /// Uses cosine distance over the HNSW index, strictly scoped by project ID
    /// to prevent cross-contamination between agent tasks.
    #[instrument(skip(self, query_embedding))]
    pub async fn retrieve_context(
        &self,
        project_id: &str,
        query_embedding: Vec<f32>,
        top_k: u32,
    ) -> Result<Vec<String>, CoreError> {
        let sql_query = "
            SELECT textual_content, vector::similarity::cosine(semantic_embedding, $query_vector) AS sim
            FROM memory_block
            WHERE project_id = $pid
            ORDER BY sim DESC
            LIMIT $limit;
        ";

        let mut result = self
            .db
            .query(sql_query)
            .bind(("query_vector", query_embedding))
            .bind(("pid", project_id.to_string()))
            .bind(("limit", top_k))
            .await
            .map_err(|e| CoreError::Internal(format!("Retrieve context failed: {e}")))?;

        // Extract textual_content from each result row.
        let contents: Vec<String> = result
            .take::<Vec<serde_json::Value>>(0)
            .map_err(|e| CoreError::Internal(format!("Deserialize memory blocks failed: {e}")))?
            .into_iter()
            .filter_map(|val| {
                val.get("textual_content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        info!(count = contents.len(), "Context retrieved");
        Ok(contents)
    }

    /// Store an execution log entry.
    pub async fn store_execution_log(
        &self,
        session_id: &str,
        phase: &str,
        action: &str,
        result: &str,
    ) -> Result<(), CoreError> {
        #[derive(Debug, Serialize, Deserialize)]
        struct LogEntry {
            session_id: String,
            phase: String,
            action: String,
            result: String,
            timestamp: surrealdb::sql::Datetime,
        }

        let entry = LogEntry {
            session_id: session_id.to_string(),
            phase: phase.to_string(),
            action: action.to_string(),
            result: result.to_string(),
            timestamp: surrealdb::sql::Datetime::default(),
        };

        let _created: Option<LogEntry> = self
            .db
            .create("execution_log")
            .content(entry)
            .await
            .map_err(|e| CoreError::Internal(format!("Store execution log failed: {e}")))?;

        Ok(())
    }

    /// Delete all memory blocks for a project (cleanup).
    pub async fn delete_project_memories(
        &self,
        project_id: &str,
    ) -> Result<(), CoreError> {
        self.db
            .query("DELETE FROM memory_block WHERE project_id = $pid")
            .bind(("pid", project_id.to_string()))
            .await
            .map_err(|e| CoreError::Internal(format!("Delete project memories failed: {e}")))?;

        Ok(())
    }

    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_store_and_retrieve() {
        let memory = MemorySystem::new(384).await.unwrap();

        let embedding = vec![0.1_f32; 384];
        memory
            .store_memory("test-project".into(), "Hello world".into(), embedding.clone())
            .await
            .unwrap();

        let results = memory
            .retrieve_context("test-project", embedding, 5)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "Hello world");
    }

    #[tokio::test]
    async fn test_project_isolation() {
        let memory = MemorySystem::new(384).await.unwrap();

        let emb_a = vec![0.1_f32; 384];
        let emb_b = vec![0.2_f32; 384];

        memory
            .store_memory("project-a".into(), "A data".into(), emb_a.clone())
            .await
            .unwrap();
        memory
            .store_memory("project-b".into(), "B data".into(), emb_b.clone())
            .await
            .unwrap();

        // Query project A — should NOT see project B's data.
        let results = memory
            .retrieve_context("project-a", emb_a, 10)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "A data");
    }
}
