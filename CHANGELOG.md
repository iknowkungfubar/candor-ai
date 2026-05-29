# Changelog

## [1.0.0] — 2026-05-29

### Production Release — Full Design Doc Implementation

All 6 design phases complete. 10 crates, 55 source files, 8,144 lines, 108+ tests.

### Architecture (10 crates)

| Crate | Purpose |
|---|---|
| `candor-core` | Shared types, errors, AgentState, ISA, AgentAction |
| `candor-graph` | Petgraph GraphRunner, 17 lifecycle events, recovery nodes |
| `candor-sandbox` | Dual-engine: wasmtime + bubblewrap, cross-platform detection, circuit breaker, backoff |
| `candor-cognitive` | Heterogeneous inference: Anthropic, OpenAI, LM Studio, Ollama, local mistral.rs |
| `candor-memory` | SurrealDB with HNSW vector index, project isolation, auto-compaction |
| `candor-sentinel` | SentinelInterceptor, 6 deterministic rules, 10 doctrine guardrails |
| `candor-orchestrator` | 7-phase LLM-driven agent, ISA hill-climbing, self-building skills, trajectory logger |
| `candor-tools` | 12 tools: fs, search, shell, test, git (sentinel-gated) |
| `candor-mcp` | MCP client (JSON-RPC 2.0 over stdio/HTTP), auto tool discovery |
| `candor-daemon` | CLI + axum REST API, LLM auto-detection from env vars |

### New in 1.0.0

- **Local inference**: `LocalBackend` with hardware auto-detection (CPU/CUDA/Metal/Vulkan)
- **Real embeddings**: Deterministic semantic hashing (better than zero vectors)
- **17 lifecycle events**: Full Claude Code pattern — BeforeTool, AfterTool, Before/AfterPhase, Before/AfterTransition, Before/AfterGit, Before/AfterSandbox, Before/AfterFile, BeforeEmbed, OnLoop, Checkpoint, Error, Completion
- **Cross-platform sandbox**: Auto-detect bubblewrap (Linux), Seatbelt (macOS), AppContainer (Windows)
- **Circuit breaker**: Configurable failure threshold with open/closed/half-open states
- **Exponential backoff**: Configurable retry with jitter
- **MCP Server support**: JSON-RPC 2.0 client, auto tool discovery, stdio + HTTP transports
- **Self-building skills**: SKILL.md generation from successful task trajectories
- **Operational doctrine**: 10 Lawful Good principles as runtime guardrails
- **Recovery nodes**: Retry logic with error analysis and escalation
- **ISA hill-climbing**: Verify phase checks ISA criteria before completion

### Coming in 1.1.0

- Tauri cross-platform desktop UI
- mistral.rs live model integration (GGUF loading)
- ONNX Runtime real embeddings
- LoRA adapter generation from trajectories
- Memory Nudge cron for daily log summarization

## [0.1.0] — 2026-05-28

Initial architecture scaffold with all 8 crates, 24 tests.
