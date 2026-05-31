use serde::{Deserialize, Serialize};
/// Unified memory storage engine with SurrealDB.
use std::time::Duration;
use tokio::sync::OnceCell;
use tracing::{error, info, instrument};

use candor_core::error::CoreError;

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

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
        Ok(Self { db, embedding_dim })
    }

    /// Lazily initialize schema on first actual operation.
    async fn ensure_schema(&self) -> Result<(), CoreError> {
        SCHEMA_INIT
            .get_or_try_init(|| async {
                info!("Running lazy SurrealDB schema init");
                let schema_queries = super::schema::schema_queries(self.embedding_dim);
                let mut qr = self
                    .db
                    .query(&schema_queries)
                    .await
                    .map_err(|e| CoreError::Internal(format!("Schema query failed: {e}")))?;

                if !qr.take_errors().is_empty() {
                    error!("Schema definition errors");
                    return Err(CoreError::Internal(
                        "Database schema init failure — check embedding dimension".into(),
                    ));
                }

                info!("SurrealDB schema initialized");
                Ok(())
            })
            .await
            .map(|_| ())
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

        tokio::time::timeout(Duration::from_secs(5), async {
            let _created: Option<MemoryBlock> = self
                .db
                .create("memory_block")
                .content(entry)
                .await
                .map_err(|e| CoreError::Internal(format!("Store failed: {e}")))?;
            Ok::<_, CoreError>(())
        })
        .await
        .map_err(|_| CoreError::Internal("Store memory timed out after 5s".into()))??;

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

        let contents: Vec<String> = tokio::time::timeout(Duration::from_secs(5), async {
            let mut result = self
                .db
                .query(sql)
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
            Ok::<_, CoreError>(contents)
        })
        .await
        .map_err(|_| CoreError::Internal("Retrieve context timed out after 5s".into()))??;

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

        let _created: Option<LogEntry> = tokio::time::timeout(Duration::from_secs(5), async {
            let created: Option<LogEntry> = self
                .db
                .create("execution_log")
                .content(entry)
                .await
                .map_err(|e| CoreError::Internal(format!("Store log failed: {e}")))?;
            Ok::<_, CoreError>(created)
        })
        .await
        .map_err(|_| CoreError::Internal("Store execution log timed out after 5s".into()))??;

        Ok(())
    }

    pub async fn delete_project_memories(&self, project_id: &str) -> Result<(), CoreError> {
        self.ensure_schema().await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            self.db
                .query("DELETE FROM memory_block WHERE project_id = $pid")
                .bind(("pid", project_id.to_string()))
                .await
                .map_err(|e| CoreError::Internal(format!("Delete failed: {e}")))
        })
        .await
        .map_err(|_| CoreError::Internal("Delete project memories timed out after 5s".into()))??;

        Ok(())
    }

    pub async fn get_all_execution_logs(&self) -> Result<Vec<ExecutionLogEntry>, CoreError> {
        self.ensure_schema().await?;

        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct RawLog {
            session_id: String,
            phase: String,
            action: String,
            result: String,
            timestamp: surrealdb::sql::Datetime,
        }

        let rows: Vec<RawLog> = tokio::time::timeout(Duration::from_secs(5), async {
            let rows: Vec<RawLog> = self
                .db
                .query("SELECT session_id, phase, action, result, timestamp FROM execution_log ORDER BY timestamp ASC")
                .await
                .map_err(|e| CoreError::Internal(format!("Query execution logs failed: {e}")))?
                .take(0)
                .map_err(|e| CoreError::Internal(format!("Deserialize execution logs failed: {e}")))?;
            Ok::<_, CoreError>(rows)
        })
        .await
        .map_err(|_| CoreError::Internal("Get all execution logs timed out after 5s".into()))??;

        Ok(rows
            .into_iter()
            .map(|r| ExecutionLogEntry {
                session_id: r.session_id,
                phase: r.phase,
                action: r.action,
                result: r.result,
                timestamp: r.timestamp,
            })
            .collect())
    }

    /// Delete all execution_log entries after summarization.
    pub async fn delete_all_execution_logs(&self) -> Result<(), CoreError> {
        self.ensure_schema().await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            self.db
                .query("DELETE FROM execution_log")
                .await
                .map_err(|e| CoreError::Internal(format!("Delete execution logs failed: {e}")))
        })
        .await
        .map_err(|_| {
            CoreError::Internal("Delete all execution logs timed out after 5s".into())
        })??;

        Ok(())
    }

    /// Query execution logs filtered by session_id.
    pub async fn get_execution_logs_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<ExecutionLogEntry>, CoreError> {
        self.ensure_schema().await?;

        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct RawLog {
            session_id: String,
            phase: String,
            action: String,
            result: String,
            timestamp: surrealdb::sql::Datetime,
        }

        let rows: Vec<RawLog> = tokio::time::timeout(Duration::from_secs(5), async {
            let rows: Vec<RawLog> = self
                .db
                .query("SELECT session_id, phase, action, result, timestamp FROM execution_log WHERE session_id = $sid ORDER BY timestamp ASC")
                .bind(("sid", session_id.to_string()))
                .await
                .map_err(|e| CoreError::Internal(format!("Query session logs failed: {e}")))?
                .take(0)
                .map_err(|e| CoreError::Internal(format!("Deserialize session logs failed: {e}")))?;
            Ok::<_, CoreError>(rows)
        })
        .await
        .map_err(|_| CoreError::Internal("Get execution logs by session timed out after 5s".into()))??;

        Ok(rows
            .into_iter()
            .map(|r| ExecutionLogEntry {
                session_id: r.session_id,
                phase: r.phase,
                action: r.action,
                result: r.result,
                timestamp: r.timestamp,
            })
            .collect())
    }

    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}

/// A single execution log entry returned by query methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLogEntry {
    pub session_id: String,
    pub phase: String,
    pub action: String,
    pub result: String,
    pub timestamp: surrealdb::sql::Datetime,
}
