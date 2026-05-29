# Architecture

Candor AI is a production-grade agent harness — a Rust-native Agentic Operating System. This document describes the architecture at a high level. For the full design rationale, see `candor-ai-design.md`.

## Philosophy

The underlying LLM is not an autonomous agent. It is a raw, non-deterministic processing engine. The intelligence, reliability, and security derive entirely from the deterministic Rust control plane surrounding it.

Recent research shows that 98.4% of a stable agentic codebase is deterministic infrastructure — context management, tool routing, permission gating, loop enforcement, and failure recovery.

## Control Plane

### 1. Graph Runner (`candor-graph`)

The orchestration heart. Uses `petgraph::DiGraph` where:
- **Nodes** implement the `AgentNode` trait (async execute method)
- **Edges** represent conditional routing with labeled transitions
- **State** is `Arc<Mutex<AgentState>>` — shared, thread-safe, tightly scoped

Iteration safety: `max_iterations` guard prevents infinite loops. State is checkpointed to disk after every transition via `CheckpointManager`.

### 2. Seven-Phase State Machine (`candor-orchestrator`)

Implements Algorithm v6.3.0:

```
Observe → Think → Plan → Build → Execute → Verify → Learn
```

Each phase is a petgraph node. The graph enforces strict sequential progression with no backward edges. The Verify phase checks against an `IdealStateArtifact` (ISA) that defines exact, programmatic success criteria.

### 3. Sandbox (`candor-sandbox`)

Dual-engine architecture:
- **WASM-first**: wasmtime with deny-by-default capability sandboxing. Fuel-limited instruction steps prevent DoS.
- **OS-level fallback**: bubblewrap on Linux (`--unshare-all`, filesystem isolation, network gating) for legacy tools.

The `ToolSandbox` provides a unified interface that auto-routes WASM requests to wasmtime and everything else to the process sandbox.

### 4. Cognitive Engine (`candor-cognitive`)

Heterogeneous inference plane:
- **Cloud frontier**: External API routing (Anthropic, OpenAI) for complex reasoning
- **Local tier**: Quantized models via mistral.rs for high-volume, privacy-sensitive tasks
- **Embeddings**: TextEmbedding engine for semantic vector generation

Dynamic routing: frontier first, local fallback. Circuit breaker pattern on API degradation.

### 5. Memory (`candor-memory`)

SurrealDB embedded (kv-mem for dev, RocksDB for prod):
- HNSW vector index for semantic similarity search
- Project-scoped isolation — queries never leak across projects
- 135K token hard limit with compaction monitoring
- Execution log storage for trajectory extraction

### 6. Sentinel (`candor-sentinel`)

Portable sidecar pattern — architecturally isolated from the primary agent:
- **Receives**: only `<proposed_action>` + `<valid_scopes>` — no conversation history, no persona
- **Deterministic rules** (sync, fail-fast): force-push blocks, rm -rf traps, TODO detection, narration comments, dead code, scope violations
- **Semantic audit** (async): routes to local inference tier for slop/hallucination detection

Eliminates "peer preservation" — the Sentinel cannot develop social alignment with the primary agent.

### 7. Daemon (`candor-daemon`)

axum HTTP server on port 31337:
- `GET /` — service info
- `GET /api/health` — subsystem health
- `GET /api/status` — current phase / session state
- `POST /api/task` — submit ISA-driven task
- `GET /api/metrics` — Prometheus-compatible metrics

Configuration via `candor.toml` using Figment (TOML + env var overrides).

## Cross-Cutting Concerns

- **Telemetry**: OpenTelemetry tracing on every node transition and tool execution
- **Checkpoints**: State persisted to JSON after every transition — durable resume on crash
- **Lifecycle Hooks**: 7 hook traits (BeforeToolCallback, AfterToolCallback, Before/AfterNodeTransition, CheckpointCallback, ErrorCallback, CompletionCallback) for extensibility
- **Conventional Commits**: Mechanically enforced by Sentinel
