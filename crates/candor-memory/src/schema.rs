/// SurrealDB schema definitions for the memory system.
///
/// Defines tables, fields, and indexes used by MemorySystem.
/// The HNSW index uses a dimensionality matching the embedding model
/// (384 for AllMiniLML6V2Q, the default).

/// SurrealDB queries that define the database schema.
///
/// Returns the raw SurrealQL queries to execute during initialization.
pub fn schema_queries(embedding_dim: usize) -> String {
    format!(
        r#"
        DEFINE TABLE memory_block SCHEMAFULL;
        DEFINE FIELD project_id ON memory_block TYPE string;
        DEFINE FIELD textual_content ON memory_block TYPE string;
        DEFINE FIELD semantic_embedding ON memory_block TYPE array<float>;
        DEFINE FIELD timestamp ON memory_block TYPE datetime;

        -- HNSW index optimized for the embedding model's dimension.
        -- DIST COSINE measures angular similarity (closest to semantic similarity).
        DEFINE INDEX memory_embed_idx ON memory_block
            FIELDS semantic_embedding
            HNSW DIMENSION {embedding_dim} DIST COSINE;

        -- Project metadata table.
        DEFINE TABLE project SCHEMAFULL;
        DEFINE FIELD name ON project TYPE string;
        DEFINE FIELD description ON project TYPE string;
        DEFINE FIELD created_at ON project TYPE datetime;
        DEFINE FIELD skills ON project TYPE array<string>;

        -- Execution log table for trajectory extraction.
        DEFINE TABLE execution_log SCHEMAFULL;
        DEFINE FIELD session_id ON execution_log TYPE string;
        DEFINE FIELD phase ON execution_log TYPE string;
        DEFINE FIELD action ON execution_log TYPE string;
        DEFINE FIELD result ON execution_log TYPE string;
        DEFINE FIELD timestamp ON execution_log TYPE datetime;
    "#
    )
}
