Master Project ISA: Agentic Operating System
1. Problem
Current AI coding agents operate without deterministic constraints, resulting in unpredictable execution, semantic degradation over long context windows, and code generation characterized by "slop" (unresolved placeholders, dead code, and unnecessary abstractions). Furthermore, unrestricted host access and multi-agent peer-preservation behaviors introduce severe systemic vulnerabilities.

2. Vision
A locally-hosted, hardware-agnostic, Rust-based agentic operating system. The system enforces a strict "Lawful Good" operating doctrine, utilizes capability-based sandboxing, and mandates programmatic verification before execution. It acts as an uncompromising, high-reliability control plane rather than a conversational chatbot.

3. Out of Scope
Cloud-dependent execution environments requiring proprietary inference for core routing.

Python or Go-based primary control planes.

Terminal User Interfaces (TUI) via ratatui (UI development is consolidated to Tauri).

Recursive self-modification (the agent is strictly prohibited from altering its own core Rust infrastructure code).

4. Principles
Precision Over Persuasion: Claims and generated code must survive adversarial reading.

Systems Before Tools: Infrastructure and deterministic logic supersede prompt engineering. Prompts wrap code; code does not wrap prompts.

Failure Is the Primary Use Case: The system must utilize explicit checkpointing and robust recovery paths for every external interaction.

Reversibility Matters More Than Speed: High-risk actions (e.g., Git commits, filesystem wipes) require human-in-the-loop pauses or strict sandbox validation.

5. Constraints
Must compile and execute securely on Linux, macOS, and Windows without relying on OS-specific kernel modules like eBPF.

Must maintain a memory footprint capable of running on consumer-grade hardware alongside local model inference.

Must enforce the 7-phase Algorithm v6.3.0 sequence (Observe, Think, Plan, Build, Execute, Verify, Learn) for all state mutations.

6. Goal
Deliver a production-ready, highly reliable agentic harness that intercepts all large language model outputs, verifies them against explicit capability policies, and executes them within zero-trust sandbox environments.

7. Criteria (Ideal State Criteria)
The following checkboxes serve as the deterministic system of record. CheckCompleteness validation must return 'empty' for missing components prior to marking the project complete.

Phase 1: Pulse Daemon and Heterogeneous Intelligence

[ ] The Rust workspace compiles cleanly with zero warnings via cargo clippy.

[ ] An Axum server binds successfully to 127.0.0.1:31337.

[ ] The CognitiveEngine successfully routes requests between the Anthropic API and local mistral.rs instances based on payload configuration.

Phase 2: The Portable Isolation Chamber

[ ] The wasmtime engine successfully compiles and executes third-party tools as WebAssembly components.

[ ] Wasmtime fuel limits are configured and actively trap infinite loops.

[ ] The adk-sandbox fallback routes legacy binaries through bubblewrap (Linux) or Seatbelt (macOS) without bypassing network restrictions.

Phase 3: The 7-Phase Orchestration Graph

[ ] The adk-graph runner strictly traverses the 7 nodes (Observe → Think → Plan → Build → Execute → Verify → Learn).

[ ] Graph execution pauses correctly for human-in-the-loop confirmation before the Execute node triggers.

[ ] The graph writes a durable checkpoint to the local disk after every successful node transition.

Phase 4: Context Compaction and Persistence

[ ] The embedded SurrealDB instance initializes via the kv-mem feature.

[ ] The fastembed pipeline accurately vectorizes markdown input into 384-dimensional dense vectors using the ONNX runtime locally.

[ ] A hard 135,000-token limit trigger automatically forces context compaction and saves the summary to a persistent markdown factstore.

Phase 5: Sentinel Application-Layer Interruption and No-Slop Guardrails

[ ] The SentinelInterceptor is registered as a BeforeToolCallback middleware hook.

[ ] The Sentinel automatically halts execution if the generated payload contains git push --force, TODO:, or AI narration comments.

[ ] The Sentinel evaluates payloads using a local quantized model, isolated entirely from the primary agent's conversational history.

[ ] Code failing language-specific test suites inside the adk-sandbox triggers a commit rejection.

Phase 6: Trajectory Extraction and Offline LoRA Fine-Tuning

[ ] The Learn node automatically generates a deterministic .md skill file upon the successful completion of a novel task sequence.

[ ] Daily execution logs are parsed and appended to a local .jsonl file.

[ ] The system provisions the .jsonl file to an offline pipeline for Low-Rank Adaptation (LoRA) weight generation.

8. Test Strategy
All Rust modules require unit tests asserting exact error variants.

Tool execution relies on deterministic adk-sandbox dry-runs against infinite loops and unauthorized directory traversal.

Integration testing must trigger the 7-phase loop against a mock ISA.md and verify the output schema.

9. Features
Background Pulse Daemon.

Hybrid WebAssembly and OS-level sandboxing.

Hardware-agnostic dynamic model routing.

Agent-curated, project-scoped memory indexing via SurrealDB.

Sidecar Sentinel for "slop" and hallucination interception.

10. Decisions
Rust over Python/Go: Selected to eliminate the Global Interpreter Lock bottlenecks, ensure strict memory safety, and minimize cold-start latency.

Tauri over Ratatui: Consolidating UI development to Tauri prevents developer burnout and minimizes the cognitive load required to maintain separate state-management loops.

Application-Layer Interrupts over eBPF: Utilizing adk-graph callbacks ensures the Sentinel agent functions reliably across Linux, macOS, and Windows without kernel dependencies.

No-RAG File Indexing: Relying on the host filesystem and markdown structure rather than external vector databases to prevent retrieval flakiness.

11. Changelog
Init: Master ISA scaffolded encompassing Phase 1 through Phase 6 roadmaps.

12. Verification
Execution is verified through deterministic compilation (cargo build), strict linting (cargo clippy -- -D warnings), and automated test passes (cargo test).

Manual audit required to ensure the absence of AI-generated narration comments and single-use helper functions.
