# Contributing to Candor AI

## Principles

Candor AI follows a "Lawful Good" design philosophy:
- **Radical candor, absolute honesty, procedural integrity**
- **Verifiability over velocity**
- **Precision over persuasion**

## Development Setup

```bash
# Clone
git clone https://github.com/iknowkungfubar/candor-ai
cd candor-ai

# Build
cargo build

# Run tests
cargo test

# Check for warnings
cargo check

# Run the daemon
cargo run -- --port 31337
```

## Project Structure

```
candor-ai/
├── bin/candor-daemon/       # axum server binary
│   └── src/
│       ├── main.rs          # Entry point, CLI, router
│       ├── config.rs        # TOML config via Figment
│       └── routes.rs        # REST API handlers
├── crates/
│   ├── candor-core/         # Shared types
│   ├── candor-graph/        # PetGraph orchestration
│   ├── candor-sandbox/      # WASM + process sandbox
│   ├── candor-cognitive/    # LLM inference + embeddings
│   ├── candor-memory/       # SurrealDB storage
│   ├── candor-sentinel/     # No-slop guardrails
│   └── candor-orchestrator/ # 7-phase state machine
├── candor.toml              # Default config
├── candor-ai-design.md      # Full architecture design doc
├── Cargo.toml               # Workspace manifest
└── Cargo.lock
```

## Coding Standards

### Error Handling
- Use `CoreError` from `candor-core` for all errors
- String conversion pattern: `.map_err(|e| CoreError::Io(e.to_string()))`
- Never use `.unwrap()` in production code — use proper error propagation

### Async Patterns
- Always scope `Mutex` locks to minimum lines — drop before `.await`
- Bind `Arc` to local variable before lock: `let state_arc = runner.state(); let s = state_arc.lock().await;`
- Use `#[async_trait]` on trait definitions AND implementations

### Tests
- One `#[cfg(test)] mod tests` per source file
- Prefer integration tests for cross-crate behavior
- All tests must pass before committing

### Commits
- Follow [Conventional Commits](https://www.conventionalcommits.org/)
- Format: `type(scope): description`
- Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore

## Pull Request Process

1. Create a feature branch
2. Make changes with conventional commit messages
3. Ensure `cargo test` passes
4. Ensure `cargo check` has zero warnings
5. Open PR against `main`
6. No force-push to main — mechanically enforced by Sentinel

## License

MIT — see [LICENSE](LICENSE)
