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
| `candor-ai` | CLI + axum REST API, LLM auto-detection from env vars |

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

## [1.1.0] — 2026-05-30

### Voice Features
- **STT module** (stt.rs): Microphone recording via arecord + whisper-cpp CLI transcription
- **TTS module** (tts.rs): Open-source TTS with piper-tts (preferred, neural) and espeak-ng (fallback)
- **Voice Interactive mode**: `candor voice-interactive` — conversational loop: listen → think → speak (TTS)
- **Voice Task mode**: `candor voice` — one-shot record → transcribe → run as agent task
- `find_on_path` extracted to shared `util.rs` module (deduplicated from stt.rs + tts.rs)

### Personal Digital Assistant (PDA)
- **PDA home** (`~/.candor/`): Git-backed identity + memory system
- **IDENTITY.md + DA_IDENTITY.md**: Define who you are and your DA's personality
- **Memory triage**: WORK/&lt;slug&gt;/ISA.md, LEARNING/, KNOWLEDGE/ directories
- **Git auto-commit**: Every memory write is version-controlled
- **CLI subcommands**: `candor pda {init|status|identity|da-identity|work|work-start|digest|monitor}`
- **Morning Digest**: Daily briefing generated from identity + work state, spoken via TTS
- **Monitor Agent**: Scans for stale work sessions and knowledge gaps
- **PDA check in `candor doctor`** diagnostics

### Performance & Build
- **Build speed**: Clean dev build 1m46s → 19.66s (-81.5%) — removed 7 dead deps (figment, ndarray, tokenizers, rstest, metrics, metrics-exporter, wasmtime-wasi)
- **Release build**: >5min → 3m15s — thin LTO instead of fat LTO
- **Clippy warnings**: 36 → 0
- **Test suite**: Fixed 2 hanging tests (recursive cargo test invocation via env guard)
- **Benchmarks**: Criterion baseline suite — 6 benchmarks for AgentState, ISA, error construction
- **Code duplication**: find_on_path extracted, 5-level nested if flattened with let-chains
- **PDA test coverage**: 0 → 15 unit tests (init, identity, work, memory, git auto-commit)

### Security
- **wasmtime bump**: 30 → 36 — fixes 17 security advisories (2 critical sandbox escapes)
- **Desktop npm**: vite 5 → 6, esbuild override — 2 moderate vulns fixed
- **cargo audit in CI**: Automated dependency vulnerability scanning
- **Subprocess exit-code audit**: Fixed git config detection (is_err → success() pattern)
- **API key Debug redaction**: All 4 backends use finish_non_exhaustive() to hide secrets

### Testing & Code Quality
- **Recursive test guard**: CANDOR_SKIP_TEST_EXECUTION env var prevents deadlock
- **Test warnings**: 8+ unused imports cleaned across 7 test files
- **Zero compiler warnings**, zero clippy warnings
- **Status() decomposition**: Extracted count_memory_files() and git_uncommitted_count() helpers
- **Trailing whitespace**: Cleaned 16 lines across 4 source files
- mistral.rs live model integration (GGUF loading)
- ONNX Runtime real embeddings
- LoRA adapter generation from trajectories
- Memory Nudge cron for daily log summarization

## [0.1.0] — 2026-05-28

Initial architecture scaffold with all 8 crates, 24 tests.
