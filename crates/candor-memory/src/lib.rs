// candor-memory: unified storage with SurrealDB + vector search.
//
// From the design doc: "SurrealDB serves as the core database engine.
// Written natively in Rust, SurrealDB acts as a unified data store
// capable of relational queries, graph connections, and native vector
// similarity search."
//
// Uses kv-mem backend for embedded, zero-dependency operation in
// development. Migrates to RocksDB for production persistence.

pub mod store;
pub mod schema;

pub use store::MemorySystem;
