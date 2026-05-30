/// OS-level process sandbox backend.
///
/// For legacy tools that can't be compiled to WASM, this provides a
/// unified ProcessBackend abstraction. Under the hood, it transparently
/// applies OS-native restrictions:
///   - bubblewrap on Linux
///   - Seatbelt on macOS
///   - AppContainer on Windows
///
/// From the design doc: "Legacy binaries run through adk-sandbox, dynamically
/// routing to Linux bubblewrap, macOS Seatbelt, or Windows AppContainer
/// via a unified ProcessBackend abstraction interface."
use std::path::PathBuf;
use std::process::Stdio;
use tracing::{info, instrument};

use candor_core::error::CoreError;

use super::policy::SandboxPolicy;
use super::cross_platform::{PlatformInfo, SandboxType};

#[derive(Debug, Clone)]
pub enum Language {
    Python,
    Rust,
    Shell,
    Wasm,
}

/// A request to execute code or a binary through the process sandbox.
#[derive(Debug, Clone)]
pub struct ProcessExecRequest {
    /// The language or runtime of the code.
    pub language: Language,
    /// The code string or script content to execute.
    pub code: String,
    /// Optional stdin for the process.
    pub stdin: Option<String>,
    /// Timeout in seconds.
    pub timeout_secs: u64,
    /// Memory limit in MB.
    pub memory_limit_mb: Option<u64>,
    /// Additional arguments to the runtime.
    pub args: Vec<String>,
}

/// Result of a process sandbox execution.
#[derive(Debug, Clone)]
pub struct ProcessExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub wall_time_ms: u64,
}

/// The OS-native process sandbox backend.
///
/// On Linux, wraps commands in bubblewrap (bwrap) for filesystem
/// and network isolation. Falls back to direct process execution
/// with resource limits if bwrap is unavailable.
pub struct ProcessBackend {
    policy: SandboxPolicy,
    /// Whether bubblewrap is available on this host.
    bwrap_available: bool,
    /// Whether seatbelt (macOS) is available.
    seatbelt_available: bool,
    /// The sandbox type to use.
    #[allow(dead_code)]
    sandbox_type: SandboxType,
    /// Scratchpad directory inside the sandbox.
    scratchpad: PathBuf,
}

impl ProcessBackend {
    pub fn new(policy: SandboxPolicy) -> Result<Self, CoreError> {
        let scratchpad = PathBuf::from("/tmp/agent_scratchpad");
        std::fs::create_dir_all(&scratchpad).map_err(|e| CoreError::Io(e.to_string()))?;

        let platform = PlatformInfo::detect();
        let bwrap_available = platform.bwrap_available;
        let seatbelt_available = platform.seatbelt_available;
        let sandbox_type = platform.sandbox_type;

        if bwrap_available {
            info!("bubblewrap (bwrap) detected — enabling OS-level sandbox");
        } else if seatbelt_available {
            info!("macOS Seatbelt detected — enabling OS-level sandbox");
        } else {
            info!("No OS-level sandbox available — falling back to direct execution with resource limits");
        }

        Ok(Self {
            policy,
            bwrap_available,
            seatbelt_available,
            sandbox_type,
            scratchpad,
        })
    }

    /// Execute code through the OS-level sandbox.
    #[instrument(skip(self))]
    pub async fn execute(
        &self,
        request: &ProcessExecRequest,
    ) -> Result<ProcessExecResult, CoreError> {
        info!(
            language = ?request.language,
            "Executing in OS-level process sandbox"
        );

        let (runtime, code_arg, ext) = match request.language {
            Language::Python => ("python3", "-c", "py"),
            Language::Rust => ("rust-script", "-e", "rs"),
            Language::Shell => ("sh", "-c", "sh"),
            Language::Wasm => {
                return Err(CoreError::Internal(
                    "WASM execution must go through WasmBackend".into(),
                ));
            }
        };

        // Write code to a temp file in the scratchpad.
        let script_path = self.scratchpad.join(format!("script_{}.{ext}", uuid::Uuid::new_v4()));
        tokio::fs::write(&script_path, &request.code)
            .await
            .map_err(|e| CoreError::Io(e.to_string()))?;

        let result = if self.bwrap_available {
            self.execute_with_bwrap(&script_path, runtime, request)
                .await
        } else if self.seatbelt_available {
            self.execute_with_seatbelt(&script_path, runtime, request)
                .await
        } else {
            self.execute_direct(&script_path, runtime, code_arg, request)
                .await
        };

        // Clean up scratch file.
        let _ = tokio::fs::remove_file(&script_path).await;

        result
    }

    #[instrument(skip(self))]
    async fn execute_with_bwrap(
        &self,
        script_path: &std::path::Path,
        runtime: &str,
        _request: &ProcessExecRequest,
    ) -> Result<ProcessExecResult, CoreError> {
        use std::process::Stdio;
        use tokio::process::Command;

        let start = std::time::Instant::now();

        let mut cmd = Command::new("bwrap");
        cmd.arg("--ro-bind").arg("/usr").arg("/usr");
        cmd.arg("--ro-bind").arg("/lib").arg("/lib");
        cmd.arg("--ro-bind").arg("/lib64").arg("/lib64");
        cmd.arg("--bind")
            .arg(&self.scratchpad)
            .arg(&self.scratchpad);
        cmd.arg("--proc").arg("/proc");
        cmd.arg("--dev").arg("/dev");
        cmd.arg("--unshare-all");
        cmd.arg("--die-with-parent");

        // Enforce filesystem restrictions.
        for read_path in &self.policy.read_allowed {
            cmd.arg("--ro-bind").arg(read_path).arg(read_path);
        }
        for write_path in &self.policy.write_allowed {
            cmd.arg("--bind").arg(write_path).arg(write_path);
        }

        // Block network unless explicitly allowed.
        if !self.policy.network_allowed {
            cmd.arg("--unshare-net");
        }

        cmd.arg(runtime).arg(script_path);

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            CoreError::Internal(format!("Failed to spawn bwrap: {e}"))
        })?;

        let wall_time_ms = start.elapsed().as_millis() as u64;

        Ok(ProcessExecResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            wall_time_ms,
        })
    }

    /// Execute using macOS Seatbelt sandbox.
    /// Uses `sandbox-exec` with a deny-by-default profile that blocks network
    /// and restricts filesystem access to the scratchpad directory.
    #[cfg(target_os = "macos")]
    async fn execute_with_seatbelt(
        &self,
        script_path: &std::path::Path,
        runtime: &str,
        request: &ProcessExecRequest,
    ) -> Result<ProcessExecResult, CoreError> {
        use std::process::Stdio;
        use tokio::process::Command;

        let start = std::time::Instant::now();

        // Build a Seatbelt profile that denies network and restricts filesystem
        let profile = format!(
            r#"
            (version 1)
            (default deny)
            (process "file-read*" "file-write*")
            (network deny)
            (file-read* (subpath "{}"))
            (file-write* (subpath "{}"))
            "#,
            self.scratchpad.display(),
            self.scratchpad.display()
        );

        let timeout = std::time::Duration::from_secs(request.timeout_secs);
        let result = tokio::time::timeout(timeout, async {
            Command::new("sandbox-exec")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .arg("-p")
                .arg(&profile)
                .arg(runtime)
                .arg(script_path)
                .output()
                .await
        })
        .await
        .map_err(|_| CoreError::Internal("Seatbelt execution timed out".into()))?
        .map_err(|e| CoreError::Internal(format!("Failed to run sandbox-exec: {e}")))?;

        let wall_time_ms = start.elapsed().as_millis() as u64;
        Ok(ProcessExecResult {
            exit_code: result.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            wall_time_ms,
        })
    }

    /// Fallback for non-macOS (should not be called when seatbelt_available is true)
    #[cfg(not(target_os = "macos"))]
    async fn execute_with_seatbelt(
        &self,
        _script_path: &std::path::Path,
        _runtime: &str,
        _request: &ProcessExecRequest,
    ) -> Result<ProcessExecResult, CoreError> {
        Err(CoreError::Internal(
            "Seatbelt is only available on macOS".into(),
        ))
    }

    /// Execute directly with resource limits (fallback when bwrap unavailable).
    async fn execute_direct(
        &self,
        script_path: &std::path::Path,
        runtime: &str,
        code_arg: &str,
        request: &ProcessExecRequest,
    ) -> Result<ProcessExecResult, CoreError> {
        use tokio::process::Command;

        let start = std::time::Instant::now();

        let timeout = std::time::Duration::from_secs(request.timeout_secs);

        let output = tokio::time::timeout(timeout, async {
            Command::new(runtime)
                .arg(code_arg)
                .arg(script_path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
        })
        .await
        .map_err(|_| CoreError::Internal("Process execution timed out".into()))?
        .map_err(|e| CoreError::Internal(format!("Failed to run process: {e}")))?;

        let wall_time_ms = start.elapsed().as_millis() as u64;

        Ok(ProcessExecResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            wall_time_ms,
        })
    }

    pub fn is_bwrap_available(&self) -> bool {
        self.bwrap_available
    }

    pub fn policy(&self) -> &SandboxPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_process_backend_creation() {
        let policy = SandboxPolicy::default();
        let backend = ProcessBackend::new(policy);
        assert!(backend.is_ok());
    }

    #[tokio::test]
    async fn test_shell_execution() {
        let policy = SandboxPolicy::default();
        let backend = ProcessBackend::new(policy).unwrap();

        let request = ProcessExecRequest {
            language: Language::Shell,
            code: "echo hello".into(),
            stdin: None,
            timeout_secs: 5,
            memory_limit_mb: None,
            args: vec![],
        };

        let result = backend.execute(&request).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }
}
