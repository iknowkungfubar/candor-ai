/// Unified sandbox engine: automatic routing between WASM and OS sandboxes.
///
/// From the design doc: "A unified execution environment managing both
/// WASM and legacy native sandboxes."
use tracing::{info, instrument};

use candor_core::error::CoreError;

use super::policy::SandboxPolicy;
use super::process_exec::{ProcessBackend, ProcessExecRequest, ProcessExecResult};
use super::wasm_exec::{WasmBackend, WasmExecRequest};

/// The language/format of the code being executed.
/// Mirror of process_exec::Language for the unified interface.
pub use super::process_exec::Language as ExecLanguage;

/// A request to execute something through the unified sandbox.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub language: ExecLanguage,
    pub code: String,
    pub stdin: Option<String>,
    pub timeout_secs: u64,
    pub memory_limit_mb: Option<u64>,
    pub args: Vec<String>,
    /// Optional path to a pre-compiled WASM module (for Wasm language).
    pub wasm_path: Option<std::path::PathBuf>,
}

/// A unified execution result.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub wall_time_ms: Option<u64>,
    pub fuel_used: Option<u64>,
}

/// A unified execution environment managing both WASM and legacy native sandboxes.
///
/// Automatically routes:
///   - Language::Wasm → WasmBackend
///   - Everything else → ProcessBackend
pub struct ToolSandbox {
    wasm_engine: WasmBackend,
    native_engine: ProcessBackend,
}

impl ToolSandbox {
    pub fn new() -> Result<Self, CoreError> {
        let policy = SandboxPolicy::default();

        let native_engine = ProcessBackend::new(policy.clone())
            .map_err(|e| CoreError::Internal(format!("ProcessBackend init failed: {e}")))?;

        // WasmBackend intrinsically limits network/files via WASI context.
        let wasm_engine = WasmBackend::new(policy);

        Ok(Self {
            wasm_engine,
            native_engine,
        })
    }

    pub fn with_policy(policy: SandboxPolicy) -> Result<Self, CoreError> {
        let native_engine = ProcessBackend::new(policy.clone())
            .map_err(|e| CoreError::Internal(format!("ProcessBackend init failed: {e}")))?;

        let wasm_engine = WasmBackend::new(policy);

        Ok(Self {
            wasm_engine,
            native_engine,
        })
    }

    /// Execute code/tool in the appropriate sandbox.
    ///
    /// WASM requests go through the capability-based wasmtime sandbox.
    /// All other languages go through the OS-level process sandbox.
    #[instrument(skip(self))]
    pub async fn execute_tool(
        &self,
        code: &str,
        language: ExecLanguage,
    ) -> Result<String, CoreError> {
        info!(
            language = ?language,
            "Executing tool in unified sandbox boundary"
        );

        let request = ExecRequest {
            language,
            code: code.to_string(),
            stdin: None,
            timeout_secs: 15,
            memory_limit_mb: Some(256),
            args: vec![],
            wasm_path: None,
        };

        let result = self.execute(&request).await;

        match result {
            Ok(output) if output.exit_code == 0 => Ok(output.stdout),
            Ok(output) => Err(CoreError::Internal(format!(
                "Execution trap: {}",
                output.stderr
            ))),
            Err(e) => {
                if e.to_string().contains("timeout") || e.to_string().contains("Timed out") {
                    Err(CoreError::Internal("Resource exhausted: timeout".into()))
                } else {
                    Err(e)
                }
            }
        }
    }

    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        request: &ExecRequest,
    ) -> Result<ExecResult, CoreError> {
        match request.language {
            ExecLanguage::Wasm => {
                let wasm_path = request.wasm_path.as_ref().ok_or_else(|| {
                    CoreError::Internal(
                        "wasm_path required for WASM language execution".into(),
                    )
                })?;

                let wasm_req = WasmExecRequest {
                    wasm_path: wasm_path.clone(),
                    function: "run".into(),
                    stdin: request.stdin.clone(),
                    timeout_secs: request.timeout_secs,
                };

                let result = self.wasm_engine.execute(&wasm_req).await?;
                Ok(ExecResult {
                    exit_code: result.exit_code,
                    stdout: result.stdout,
                    stderr: result.stderr,
                    wall_time_ms: None,
                    fuel_used: Some(result.fuel_used),
                })
            }
            _ => {
                let proc_req = ProcessExecRequest {
                    language: request.language.clone(),
                    code: request.code.clone(),
                    stdin: request.stdin.clone(),
                    timeout_secs: request.timeout_secs,
                    memory_limit_mb: request.memory_limit_mb,
                    args: request.args.clone(),
                };

                let ProcessExecResult {
                    exit_code,
                    stdout,
                    stderr,
                    wall_time_ms,
                } = self.native_engine.execute(&proc_req).await?;

                Ok(ExecResult {
                    exit_code,
                    stdout,
                    stderr,
                    wall_time_ms: Some(wall_time_ms),
                    fuel_used: None,
                })
            }
        }
    }

    pub fn native_engine(&self) -> &ProcessBackend {
        &self.native_engine
    }

    pub fn wasm_engine(&self) -> &WasmBackend {
        &self.wasm_engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unified_sandbox_shell() {
        let sandbox = ToolSandbox::new().unwrap();
        let result = sandbox
            .execute_tool("echo sandboxed", ExecLanguage::Shell)
            .await
            .unwrap();
        assert!(result.contains("sandboxed"));
    }
}
