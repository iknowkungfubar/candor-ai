# Candor AI — Lawful Good Rust Agentic Operating System

A production-grade agent harness implementing Algorithm v6.3.0 with WASM sandboxing, heterogeneous inference, SurrealDB memory, a portable sentinel, 17 lifecycle hooks, MCP server support, and self-building skills.

## Quick Start

```bash
# Build
cargo build --release

# Run with local model
LM_STUDIO_URL="http://localhost:1234/v1" cargo run -- --task "build a Rust CLI tool"

# Run with cloud API
OPENAI_API_KEY="sk-..." cargo run -- --task "add error handling to the API"

# Health check
cargo run -- --health

# Run all tests
cargo test

# Run with MCP servers
MCP_SERVERS="http://localhost:3000" cargo run -- --task "search the web and summarize"
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                  OrchestratorEngine                    │
│   Observe → Think → Plan → Build → Execute → Verify → Learn │
│              (7-phase LLM-driven agent)               │
├──────────┬──────────┬──────────┬──────────┬──────────┤
│  Graph    │ Sandbox  │Cognitive │ Memory   │ Sentinel  │
│  Runner   │ (WASM +  │ Engine   │(SurrealDB)│ Interceptor│
│ (petgraph)│ bwrap)   │          │          │           │
├──────────┼──────────┼──────────┼──────────┼──────────┤
│  Tools    │   MCP    │  Local   │ Skills   │ Recovery  │
│ (12 tools)│  Client  │ Backend  │ System   │ Nodes     │
└──────────┴──────────┴──────────┴──────────┴──────────┘
```

### 10 Crate Workspace

| Crate | Purpose |
|---|---|
| `candor-core` | Shared types: AgentState, ISA, AgentAction, errors |
| `candor-graph` | Petgraph GraphRunner, 17 lifecycle hooks, recovery nodes |
| `candor-sandbox` | Dual-engine: wasmtime + bubblewrap, circuit breaker, backoff |
| `candor-cognitive` | Heterogeneous inference: Anthropic, OpenAI, LM Studio, Ollama, local |
| `candor-memory` | SurrealDB with HNSW vector index, auto-compaction |
| `candor-sentinel` | SentinelInterceptor, 6 deterministic rules, 10 doctrine guardrails |
| `candor-orchestrator` | 7-phase LLM-driven agent, ISA hill-climbing, self-building skills |
| `candor-tools` | 12 tools: fs, search, shell, test, git (sentinel-gated) |
| `candor-mcp` | MCP client (JSON-RPC 2.0), auto tool discovery |
| `candor-daemon` | CLI + axum REST API, LLM auto-detection |

### CLI

```bash
candor --task "description"     # Run a full 7-phase agent task
candor --health                 # Check all subsystems
candor --port 31337             # Start REST daemon
candor --model "gpt-4o"         # Override model
candor --mcp "http://localhost:3000"  # Connect MCP servers
```

### API Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/` | Service info |
| GET | `/api/health` | Subsystem health |
| GET | `/api/status` | Current phase, session state |
| POST | `/api/task` | Submit a task for agent execution |
| GET | `/api/metrics` | Execution metrics |

### Tool System (12 tools)

| Tool | Description |
|---|---|
| `read_file` | Read file contents with line limit |
| `write_file` | Write content to file |
| `list_dir` | List directory contents |
| `search_code` | grep with ripgrep |
| `search_files` | Find files by name pattern |
| `shell` | Execute command in sandbox |
| `run_tests` | Run cargo test suite |
| `git_branch` | Create feature branch (force blocked) |
| `git_commit` | Conventional commit (format validated) |
| `git_push` | Push (force-push blocked by sentinel) |
| `git_status` | Working tree status |

### LLM Auto-Detection

The daemon auto-detects backends in this priority order:
1. `ANTHROPIC_API_KEY` → Anthropic
2. `OPENAI_API_KEY` → OpenAI (or compatible via `OPENAI_BASE_URL`)
3. `LM_STUDIO_URL` → LM Studio local
4. `OLLAMA_URL` → Ollama local
5. `CANDOR_MODEL` → Override model name
6. Fallback → Mock backend (for testing)

### 17 Lifecycle Hooks

1. `BeforeTool` — before any tool execution
2. `AfterTool` — after any tool execution
3. `BeforeTransition` — before node transition
4. `AfterTransition` — after node transition
5. `Checkpoint` — every N iterations
6. `OnError` — on execution error
7. `OnComplete` — on successful completion
8. `BeforePhaseEntry` — before phase entry
9. `AfterPhaseExit` — after phase exit
10. `BeforeFileRead` — before file read
11. `AfterFileWrite` — after file write
12. `BeforeGitOp` — before git operation
13. `AfterGitOp` — after git operation
14. `BeforeSandboxExec` — before sandbox execution
15. `AfterSandboxExec` — after sandbox execution
16. `BeforeEmbedding` — before embedding generation
17. `OnLoopIteration` — every cycle of the execution loop

### Configuration

Environment variables:
- `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` / `LM_STUDIO_URL` / `OLLAMA_URL`
- `CANDOR_MODEL` — model override
- `MCP_SERVERS` — comma-separated MCP server URLs

### Development

```bash
cargo test                   # All tests
cargo test -p candor-core    # Specific crate
cargo check --workspace      # Fast check
cargo build --release        # Release build
```

### License

MIT — see [LICENSE](LICENSE)
