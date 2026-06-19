# AGENTS.md — Candor AI

> Operating context for AI agents working on this repository. Load at session start.

## Project Identity

**Candor AI** is a production-grade personal AI agent system written in Rust. It features voice input/output, persistent memory, PDA capabilities, and a pluggable skill ecosystem. Built as a unified workspace of specialized crates.

## Tech Stack

- **Language:** Rust (edition 2021), workspace with 11 crates
- **Build:** Cargo (Cargo.toml)
- **Linting:** clippy
- **Testing:** cargo test
- **Formatting:** rustfmt
- **Key crates:** candor-core, candor-graph, candor-memory, candor-sandbox, candor-orchestrator, candor-mcp, candor-telemetry
- **Published:** Not yet on crates.io

## Repository Structure

```
├── crates/                    # Workspace crates
│   ├── candor-core/           # Core types, traits, and abstractions
│   ├── candor-graph/          # Knowledge graph implementation
│   ├── candor-sandbox/        # Sandboxed code execution
│   ├── candor-cognitive/      # Cognitive / reasoning engine
│   ├── candor-memory/         # Long-term memory persistence
│   ├── candor-sentinel/       # Security monitoring / guardrails
│   ├── candor-orchestrator/   # Agent orchestration
│   ├── candor-tools/          # Tool implementations
│   ├── candor-mcp/            # MCP protocol integration
│   └── candor-telemetry/      # Observability and metrics
├── bin/candor-daemon/         # Main daemon binary
├── ARCHITECTURE.md            # System architecture
├── ISA.md                     # Instruction set architecture
├── SYSTEM.md                  # System specification
└── README.md                  # Project documentation
```

## Key Documentation

- **ARCHITECTURE.md** — System architecture and component relationships
- **ISA.md** — Instruction set architecture for agent operations
- **SYSTEM.md** — Detailed system specification
- **candor-ai-design.md** — Design philosophy and decisions

## Conventions

- **Commits:** Conventional commits (`feat:|fix:|refactor:|test:|docs:|chore:`)
- **Rust idioms:** Use Result/Option, avoid unwrap in library code
- **Error handling:** Custom error types with thiserror
- **Async:** tokio runtime
- **Documentation:** Rustdoc on all public items

## Quality Gates

- `cargo clippy -- -D warnings` — 0 warnings
- `cargo test` — all tests pass
- `cargo fmt --check` — formatting clean
- `cargo doc --no-deps` — documentation builds

## Agent Workflow

1. **Read the architecture** — Start with ARCHITECTURE.md and ISA.md
2. **Find the right crate** — Each concern is in its own crate
3. **Tests alongside code** — Rust makes this natural
4. **No unwrap in library code** — Propagate errors up
5. **Verify** — Run `cargo test && cargo clippy` before claiming done
