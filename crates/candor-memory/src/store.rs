/// Unified memory storage engine with SurrealDB.
use std::sync::atomic::{AtomicBool, Ordering};
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
    db: surrealdb::Surreal<surrealdb::engine::local::Db>,
    embedding_dim: usize,
    /// Whether schema has been initialized (lazy init).
    schema_ready: AtomicBool,
}

impl MemorySystem {
    /// Create a new in-memory MemorySystem with lazy schema initialization.
    /// Schema queries run on first actual operation, not at construction.
    pub async fn new(embedding_dim: usize) -> Result<Self, CoreError> {
        info!("Creating SurrealDB connection (schema init deferred)");

        let db = surrealdb::Surreal::new::<surrealdb::engine::local::Mem>(())
            .await
            .map_err(|e| CoreError::Internal(format!("SurrealDB connect failed: {e}")))?;

        db.use_ns("candor_namespace")
            .use_db("candor_database")
            .await
            .map_err(|e| CoreError::Internal(format!("SurrealDB ns/db error: {e}")))?;

        info!("SurrealDB memory engine ready (lazy schema)");
        Ok(Self {
            db,
            embedding_dim,
            schema_ready: AtomicBool::new(false),
        })
    }

    /// Lazily initialize schema on first actual operation.
    async fn ensure_schema(&self) -> Result<(), CoreError> {
        if self.schema_ready.load(Ordering::SeqCst) {
            return Ok(());
        }

        info!("Running lazy SurrealDB schema init");
        let schema_queries = super::schema::schema_queries(self.embedding_dim);
        let mut qr = self.db.query(&schema_queries).await
            .map_err(|e| CoreError::Internal(format!("Schema query failed: {e}")))?;

        if !qr.take_errors().is_empty() {
            error!("Schema definition errors");
            return Err(CoreError::Internal(
                "Database schema init failure — check embedding dimension".into(),
            ));
        }

        self.schema_ready.store(true, Ordering::SeqCst);
        info!("SurrealDB schema initialized");
        Ok(())
    }

    /// Store a new memory block with its embedding vector.
    #[instrument(skip(self, embedding))]
    pub async fn store_memory(
        &self,
        project_id: String,
        content: String,
        embedding: Vec<f32>,
    ) -> Result<(), CoreError> {
        self.ensure_schema().await?;

        let entry = MemoryBlock {
            project_id,
            textual_content: content,
            semantic_embedding: embedding,
            timestamp: surrealdb::sql::Datetime::default(),
        };

        let _created: Option<MemoryBlock> = self.db
            .create("memory_block")
            .content(entry)
            .await
            .map_err(|e| CoreError::Internal(format!("Store failed: {e}")))?;

        info!("Memory block persisted");
        Ok(())
    }

    #[instrument(skip(self, query_embedding))]
    pub async fn retrieve_context(
        &self,
        project_id: &str,
        query_embedding: Vec<f32>,
        top_k: u32,
    ) -> Result<Vec<String>, CoreError> {
        self.ensure_schema().await?;

        let sql = "
            SELECT textual_content, vector::similarity::cosine(semantic_embedding, $query_vector) AS sim
            FROM memory_block
            WHERE project_id = $pid
            ORDER BY sim DESC
            LIMIT $limit;
        ";

        let mut result = self.db.query(sql)
            .bind(("query_vector", query_embedding))
            .bind(("pid", project_id.to_string()))
            .bind(("limit", top_k))
            .await
            .map_err(|e| CoreError::Internal(format!("Retrieve failed: {e}")))?;

        let contents: Vec<String> = result
            .take::<Vec<serde_json::Value>>(0)
            .map_err(|e| CoreError::Internal(format!("Deserialize failed: {e}")))?
            .into_iter()
            .filter_map(|val| val.get("textual_content")?.as_str().map(|s| s.to_string()))
            .collect();

        info!(count = contents.len(), "Context retrieved");
        Ok(contents)
    }

    pub async fn store_execution_log(
        &self,
        session_id: &str,
        phase: &str,
        action: &str,
        result: &str,
    ) -> Result<(), CoreError> {
        self.ensure_schema().await?;

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

        let _created: Option<LogEntry> = self.db
            .create("execution_log")
            .content(entry)
            .await
            .map_err(|e| CoreError::Internal(format!("Store log failed: {e}")))?;

        Ok(())
    }

    pub async fn delete_project_memories(&self, project_id: &str) -> Result<(), CoreError> {
        self.ensure_schema().await?;

        self.db
            .query("DELETE FROM memory_block WHERE project_id = $pid")
            .bind(("pid", project_id.to_string()))
            .await
            .map_err(|e| CoreError::Internal(format!("Delete failed: {e}")))?;

        Ok(())
    }

    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}
