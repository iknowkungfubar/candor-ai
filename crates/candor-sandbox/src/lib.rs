// candor-sandbox: dual-engine secure execution boundary.
//
// From the design doc: "To achieve cross-platform hardware abstraction
// without compromising security, the system utilizes a dual-engine
// architecture governed by the adk-sandbox crate."
//
// Two execution pathways:
// 1. WASM-first: wasmtime with deny-by-default capability sandboxing
// 2. OS-level: process sandboxing via bubblewrap/Seatbelt/AppContainer

pub mod wasm_exec;
pub mod process_exec;
pub mod unified;
pub mod policy;
pub mod cross_platform;

pub use cross_platform::{CircuitBreaker, CircuitState, Backoff, PlatformInfo, SandboxType, with_retry};
pub use policy::SandboxPolicy;
pub use unified::ToolSandbox;
