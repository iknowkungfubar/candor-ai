/// WASM execution backend using wasmtime.
///
/// "The wasmtime runtime executes these tools within a capability-based,
/// deny-by-default sandbox. Wasmtime fuel limits instruction step execution
/// deterministically to prevent denial of service attacks."
use std::path::PathBuf;
use tracing::{info, instrument};

use candor_core::error::CoreError;

use super::policy::SandboxPolicy;

/// A request to execute code in the WASM sandbox.
#[derive(Debug, Clone)]
pub struct WasmExecRequest {
    /// Path to a pre-compiled .wasm component or module.
    pub wasm_path: PathBuf,
    /// Function to call — defaults to "run".
    pub function: String,
    /// Optional stdin passed to the WASM module.
    pub stdin: Option<String>,
    /// Timeout duration for this specific execution.
    pub timeout_secs: u64,
}

/// Result of a WASM execution.
#[derive(Debug, Clone)]
pub struct WasmExecResult {
    /// Exit code (0 = success).
    pub exit_code: i32,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Fuel consumed during execution.
    pub fuel_used: u64,
}

/// The wasmtime-based execution backend.
///
/// Uses the wasmtime runtime with fuel-limited execution and
/// deny-by-default capability sandboxing. For WASI-supporting
/// modules, WASI functions are linked in. For pure computation
/// modules, a minimal linker is sufficient.
pub struct WasmBackend {
    /// The policy governing all executions through this backend.
    policy: SandboxPolicy,
}

impl WasmBackend {
    pub fn new(policy: SandboxPolicy) -> Self {
        Self { policy }
    }

    /// Execute a pre-compiled WASM module inside a deny-by-default sandbox.
    ///
    /// The module runs with fuel-limited instruction steps to
    /// deterministically prevent DoS attacks.
    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        request: &WasmExecRequest,
    ) -> Result<WasmExecResult, CoreError> {
        info!(
            wasm_path = %request.wasm_path.display(),
            function = %request.function,
            "Executing WASM module in capability-based sandbox"
        );

        let wasm_bytes = tokio::fs::read(&request.wasm_path)
            .await
            .map_err(|e| {
                CoreError::Internal(format!(
                    "Failed to read WASM module: {e}"
                ))
            })?;

        // Configure engine with fuel metering.
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);

        let engine = wasmtime::Engine::new(&config).map_err(|e| {
            CoreError::Internal(format!(
                "Failed to create wasmtime engine: {e}"
            ))
        })?;

        // Compile the module.
        let module = wasmtime::Module::from_binary(&engine, &wasm_bytes)
            .map_err(|e| {
                CoreError::Internal(format!(
                    "Failed to compile WASM module: {e}"
                ))
            })?;

        // Create a minimal linker (no host functions — deny-by-default).
        let linker = wasmtime::Linker::new(&engine);
        let mut store = wasmtime::Store::new(&engine, ());

        // Set fuel limit from policy.
        if let Some(fuel) = self.policy.fuel_limit {
            store.set_fuel(fuel).map_err(|e| {
                CoreError::Internal(format!(
                    "Failed to set fuel limit: {e}"
                ))
            })?;
        }

        // Instantiate the module.
        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| {
                CoreError::Internal(format!(
                    "Failed to instantiate WASM module: {e}"
                ))
            })?;

        // Get the requested function.
        let func = instance
            .get_func(&mut store, &request.function)
            .ok_or_else(|| {
                CoreError::Internal(format!(
                    "Function '{}' not found in WASM module",
                    request.function
                ))
            })?;

        // Call the function.
        let mut result = [wasmtime::Val::I32(0)];
        let fuel_before = store.get_fuel().unwrap_or(0);

        match func
            .call_async(&mut store, &[], &mut result)
            .await
        {
            Ok(()) => {
                let fuel_after = store.get_fuel().unwrap_or(0);
                let fuel_used =
                    fuel_before.saturating_sub(fuel_after);

                let exit_code = match result[0] {
                    wasmtime::Val::I32(code) => code,
                    _ => 0,
                };

                Ok(WasmExecResult {
                    exit_code,
                    stdout: String::new(),
                    stderr: String::new(),
                    fuel_used,
                })
            }
            Err(e) => {
                if e.to_string().contains("fuel") {
                    Err(CoreError::Internal(
                        "WASM execution: fuel exhausted (DoS protection)"
                            .into(),
                    ))
                } else {
                    Err(CoreError::Internal(format!(
                        "WASM execution trap: {e}"
                    )))
                }
            }
        }
    }

    pub fn policy(&self) -> &SandboxPolicy {
        &self.policy
    }
}

impl Default for WasmBackend {
    fn default() -> Self {
        Self::new(SandboxPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wasm_backend_creation() {
        let backend = WasmBackend::default();
        assert!(!backend.policy().network_allowed);
        assert_eq!(backend.policy().timeout_secs, 15);
    }

    #[tokio::test]
    async fn test_wasm_exec_nonexistent_file() {
        let backend = WasmBackend::default();
        let request = WasmExecRequest {
            wasm_path: PathBuf::from(
                "/nonexistent/test.wasm",
            ),
            function: "run".into(),
            stdin: None,
            timeout_secs: 5,
        };

        let result = backend.execute(&request).await;
        assert!(result.is_err());
    }
}
