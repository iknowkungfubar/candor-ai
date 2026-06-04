<p align="center">
  <img src="assets/banner.png" alt="Candor AI Banner" width="100%">
</p>

[![Version](https://img.shields.io/badge/version-1.0.0-blue.svg)](https://github.com/iknowkungfubar/candor-ai/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![CI](https://github.com/iknowkungfubar/candor-ai/actions/workflows/ci.yml/badge.svg)](https://github.com/iknowkungfubar/candor-ai/actions/workflows/ci.yml)

# Candor AI — Lawful Good Rust Agentic Operating System

A production-grade personal AI agent with voice, memory, PDA capabilities, and a pluggable skill ecosystem.

```bash
# One-shot task
candor task "build a CLI tool"

# Interactive conversation
candor chat

# Voice-activated (whisper-cpp + piper-tts)
candor voice                        # One-shot
candor voice-interactive            # Listen → think → speak loop

# Personal Digital Assistant
candor pda init                     # Initialize ~/.candor/ identity & memory
candor pda digest                   # Morning briefing via TTS
candor pda monitor                  # Scan for stale work sessions

# Diagnostics
candor health                       # Subsystem health
candor doctor                       # Full diagnostic scan
candor serve --port 31337           # REST API daemon
```

## Quick Install

```bash
# Via Cargo (Rust toolchain required)
cargo install candor-ai

# Via install script (auto-downloads pre-built binary)
curl -sfL https://raw.githubusercontent.com/iknowkungfubar/candor-ai/main/install.sh | sh

# Verify
candor doctor
```

## Features

### 🧠 7-Phase Agent Loop
```
Observe → Think → Plan → Build → Execute → Verify → Learn
```
LLM-driven software engineering agent with the **Ideal State Artifact** (ISA) — a 12-section markdown document defining goals, criteria, and constraints for every task.

### 🎤 Voice Interface
- **STT**: Record mic via `arecord`, transcribe via `whisper-cpp`
- **TTS**: Speak responses via `piper-tts` (neural) or `espeak-ng` (fallback)
- **Interactive mode**: Listen → think → speak — conversational loop with exit words

### 🧑 Personal Digital Assistant
- **IDENTITY.md** — who you are (name, goals, preferences, values)
- **DA_IDENTITY.md** — your DA's personality (name, voice, tone, directives)
- **Git-backed memory** — every write auto-commits, full history available
- **Memory triage**: WORK/slugs (ISA tasks), LEARNING/ (meta-patterns), KNOWLEDGE/ (entities)
- **Morning Digest** — daily briefing from identity + work state + TTS
- **Monitor Agent** — scans for stale sessions and knowledge gaps

### 🔒 Security
- **WASM sandbox** (wasmtime) + **bubblewrap** process isolation
- **Sentinel guardrails**: 6 deterministic rules, 10 doctrine principles
- **Force-push blocked**, secrets never logged, deny-by-default posture
- **17 security advisories fixed** (wasmtime 30→36)
- **Zero CVEs** — automated cargo audit in CI

### 🔧 Tools (12 built-in)
| Tool | Description |
|------|-------------|
| `read_file` / `write_file` | File I/O with line limits |
| `list_dir` | Directory listing |
| `search_code` | ripgrep search |
| `search_files` | File name search |
| `shell` | Sandboxed command execution |
| `run_tests` | Cargo test runner |
| `git_branch` / `git_commit` / `git_push` / `git_status` | Git operations (sentinel-gated) |

### 🔌 Integrations
- **LLM backends**: Anthropic, OpenAI, DeepSeek, Gemini, LM Studio, Ollama — auto-detected from env vars
- **MCP servers**: stdio + HTTP transports, auto tool discovery
- **MCP skills**: 400+ bioinformatics skills, browser automation
- **REST API**: axum server on port 31337

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                     OrchestratorEngine                      │
│  Observe → Think → Plan → Build → Execute → Verify → Learn  │
├─────────┬─────────┬─────────┬─────────┬─────────┬──────────┤
│  Graph   │ Sandbox │Cognitive│ Memory  │ Sentinel│   PDA    │
│  Runner  │(WASM +  │ Engine  │(Surreal │ Inter-  │ Identity │
│(petgraph)│  bwrap) │         │  DB)    │ ceptor  │ + Memory │
├─────────┼─────────┼─────────┼─────────┼─────────┼──────────┤
│  Tools   │   MCP   │  Local  │  Skills │ Recovery│ Voice    │
│ (12 tools)│ Client  │ Backend │ System  │  Nodes  │ STT/TTS  │
└─────────┴─────────┴─────────┴─────────┴─────────┴──────────┘
```

### 11 Crate Workspace

| Crate | Purpose | Tests |
|-------|---------|-------|
| `candor-core` | Shared types, AgentState, ISA, errors | 14 |
| `candor-graph` | Petgraph runner, lifecycle hooks, recovery | 17 |
| `candor-sandbox` | wasmtime + bubblewrap, circuit breaker | 12 |
| `candor-cognitive` | LLM inference, embeddings, 4 backends | 29 |
| `candor-memory` | SurrealDB with HNSW vector index | 12 |
| `candor-sentinel` | Guardrails: rules + doctrine | 25 |
| `candor-orchestrator` | 7-phase agent, ISA climbing, skills | 58 |
| `candor-tools` | 12 tools: fs, search, shell, test, git | 27 |
| `candor-mcp` | MCP client, JSON-RPC 2.0, auto-discovery | 8 |
| `candor-daemon` | CLI + REST API + PDA + Voice | 27 |
| `candor-telemetry` | OpenTelemetry tracing | 1 |

**Total**: ~250+ tests, 0 clippy warnings, 0 compiler warnings, 0 CVEs.

> Built with Rust edition 2024. See [crates.io](https://crates.io/crates/candor-ai) for the published package.

## Configuration

```bash
# LLM backends (auto-detected, checked in this order)
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export DEEPSEEK_API_KEY="sk-..."
export GEMINI_API_KEY="..."
export LM_STUDIO_URL="http://localhost:1234/v1"
export OLLAMA_URL="http://localhost:11434/v1"

# Model override
export CANDOR_MODEL="gpt-4o"

# MCP servers
export MCP_SERVERS="http://localhost:3000"

# Audio (voice features)
export CANDOR_AUDIO_DEVICE="default"
export CANDOR_RECORD_SECONDS="5"
export CANDOR_TTS_MODEL="/path/to/piper-model.onnx"
export CANDOR_TTS_VOICE="en-us"
```

## Performance

| Metric | Value |
|--------|-------|
| Clean dev build | ~36s |
| Release build | ~3m15s |
| Binary size | 57MB stripped |
| State append (100 msgs) | 3.2 µs |
| Context compaction | 6.4 µs |
| Token limit check | 94 ps |
| ISA validation (10 criteria) | 41 ns |
| Test suite | ~250+ all passing |

## Development

```bash
git clone https://github.com/iknowkungfubar/candor-ai
cd candor-ai

# Dependencies for voice features (optional)
sudo pacman -S whisper-cpp espeak-ng alsa-utils  # Arch
# brew install whisper-cpp espeak-ng portaudio     # macOS
# apt install whisper-cpp espeak-ng alsa-utils     # Debian/Ubuntu

# Build and test
cargo build --release
cargo test --workspace
```

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines on our development process, coding standards, PR workflow, and code of conduct.

## License

MIT — see [LICENSE](LICENSE)
