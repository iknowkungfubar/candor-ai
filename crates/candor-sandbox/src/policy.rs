/// Sandbox policy: deny-by-default with explicit capabilities.
///
/// From the design doc: "Defaults are Decisions: The Wasmtime sandbox
/// enforces a deny-by-default posture. By default, the agent has zero access
/// to the host network and is restricted entirely to /tmp/agent_scratchpad."
use std::path::{Path, PathBuf};

/// The security policy governing what a sandboxed execution can do.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    /// Allowed directories for read access.
    pub read_allowed: Vec<PathBuf>,
    /// Allowed directories for write access.
    pub write_allowed: Vec<PathBuf>,
    /// Whether network access is permitted.
    pub network_allowed: bool,
    /// Maximum execution time in seconds.
    pub timeout_secs: u64,
    /// Maximum memory in MB.
    pub memory_limit_mb: Option<u64>,
    /// Maximum WASM fuel units.
    pub fuel_limit: Option<u64>,
    /// Whether to allow environment variable passthrough.
    pub env_allowed: bool,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            read_allowed: vec![PathBuf::from("/tmp/agent_scratchpad")],
            write_allowed: vec![PathBuf::from("/tmp/agent_scratchpad")],
            network_allowed: false,
            timeout_secs: 15,
            memory_limit_mb: Some(256),
            fuel_limit: Some(1_000_000),
            env_allowed: false,
        }
    }
}

/// Builder for constructing SandboxPolicies with a fluent API.
pub struct SandboxPolicyBuilder {
    policy: SandboxPolicy,
}

impl SandboxPolicyBuilder {
    pub fn new() -> Self {
        Self {
            policy: SandboxPolicy::default(),
        }
    }

    pub fn allow_read(mut self, path: &Path) -> Self {
        self.policy.read_allowed.push(path.to_path_buf());
        self
    }

    pub fn allow_write(mut self, path: &Path) -> Self {
        self.policy.write_allowed.push(path.to_path_buf());
        self
    }

    pub fn allow_network(mut self) -> Self {
        self.policy.network_allowed = true;
        self
    }

    pub fn deny_network(mut self) -> Self {
        self.policy.network_allowed = false;
        self
    }

    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.policy.timeout_secs = secs;
        self
    }

    pub fn memory_limit_mb(mut self, mb: u64) -> Self {
        self.policy.memory_limit_mb = Some(mb);
        self
    }

    pub fn fuel_limit(mut self, fuel: u64) -> Self {
        self.policy.fuel_limit = Some(fuel);
        self
    }

    pub fn build(self) -> SandboxPolicy {
        self.policy
    }
}

impl Default for SandboxPolicyBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy_is_deny_by_default() {
        let policy = SandboxPolicy::default();
        assert!(!policy.network_allowed);
        assert!(!policy.env_allowed);
        assert_eq!(policy.timeout_secs, 15);
    }

    #[test]
    fn test_builder_sets_capabilities() {
        let policy = SandboxPolicyBuilder::new()
            .allow_network()
            .timeout_secs(60)
            .memory_limit_mb(512)
            .build();

        assert!(policy.network_allowed);
        assert_eq!(policy.timeout_secs, 60);
        assert_eq!(policy.memory_limit_mb, Some(512));
    }
}
