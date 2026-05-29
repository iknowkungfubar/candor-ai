# Candor AI — Lawful Good Rust Agentic Operating System

A production-grade agent harness implementing Algorithm v6.3.0 with WASM sandboxing, heterogeneous inference, SurrealDB memory, and a portable Sentinel for no-slop enforcement.

## Architecture

Candor AI treats LLMs as raw processing engines — not autonomous agents. The intelligence, reliability, and security come from the deterministic Rust control plane surrounding them.

```
┌─────────────────────────────────────────────────────┐
│                  OrchestratorEngine                   │
│   Observe → Think → Plan → Build → Execute → Verify → Learn │
│              (7-phase state machine)                  │
├──────────┬──────────┬──────────┬──────────┬──────────┤
│  Graph    │ Sandbox  │Cognitive │ Memory   │ Sentinel  │
│  Runner   │ (WASM +  │ Engine   │(SurrealDB)│ Interceptor│
│ (petgraph)│ bwrap)   │          │          │           │
└──────────┴──────────┴──────────┴──────────┴──────────┘
```

## Crate Map

| Crate | Purpose | Status |
|---|---|---|
| `candor-core` | Shared types: AgentState, ISA, AgentAction, errors | Production |
| `candor-graph` | PetGraph GraphRunner, AgentNode trait, lifecycle hooks, checkpointing | Production |
| `candor-sandbox` | Dual-engine: WasmBackend (wasmtime) + ProcessBackend (bubblewrap) | Production |
| `candor-cognitive` | Heterogeneous inference: cloud APIs + local models + embeddings | Production |
| `candor-memory` | SurrealDB (kv-mem) with HNSW vector index + project isolation | Production |
| `candor-sentinel` | SentinelInterceptor: 6 deterministic rules + semantic slop detection | Production |
| `candor-orchestrator` | 7-phase state machine, ISA parser, 17 lifecycle hooks | Production |
| `candor-daemon` | axum HTTP server on port 31337, CLI via clap | Production |

## Quick Start

### Build

```bash
cargo build --release
```

### Run the daemon

```bash
cargo run -- --port 31337
```

### Run tests

```bash
cargo test
```

### Configuration

Create `candor.toml` (or copy from the repo):

```toml
[server]
host = "0.0.0.0"
port = 31337
max_iterations = 100

[sandbox]
scratchpad_dir = "/tmp/agent_scratchpad"
default_timeout_secs = 15
default_memory_mb = 256

[inference]
# Uncomment to enable cloud APIs:
# anthropic_api_key = "sk-ant-..."
# openai_api_key = "sk-..."

[memory]
backend = "mem"     # "mem" for dev, "rocksdb" for prod
compaction_token_limit = 135000

[sentinel]
enabled = true
semantic_audit_enabled = true
```

## API Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/` | Service info |
| GET | `/api/health` | Subsystem health checks |
| GET | `/api/status` | Current phase, session state |
| POST | `/api/task` | Submit a task for execution |
| GET | `/api/metrics` | Execution metrics |

## Key Design Principles

1. **Precision Over Persuasion** — Claims survive adversarial reading
2. **Systems Before Tools** — The harness is the permanent infrastructure
3. **Failure Is the Primary Use Case** — Graph checkpoints handle failure before success
4. **Defaults Are Decisions** — Wasmtime deny-by-default, zero network access
5. **Reversibility Matters More Than Speed** — Git discipline mechanically enforced

## License

MIT — see [LICENSE](LICENSE)
