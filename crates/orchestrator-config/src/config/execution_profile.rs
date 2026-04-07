use serde::{Deserialize, Serialize};

/// Selects whether a step runs directly on the host or inside a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileMode {
    /// Run commands with the ambient host execution environment.
    #[default]
    Host,
    /// Run commands through the configured sandbox executor.
    Sandbox,
}

/// Filesystem access mode enforced for an execution profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFsMode {
    /// Reuse the caller's default filesystem permissions.
    #[default]
    Inherit,
    /// Mount the workspace as read-only.
    WorkspaceReadonly,
    /// Grant read-write access only to explicit workspace-scoped paths.
    WorkspaceRwScoped,
}

/// Network access mode enforced for an execution profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionNetworkMode {
    /// Reuse the caller's default network policy.
    #[default]
    Inherit,
    /// Block outbound network access.
    Deny,
    /// Permit outbound access only to the configured allowlist.
    Allowlist,
}

/// Resource and isolation limits applied to step execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionProfileConfig {
    /// Chooses the underlying execution environment.
    #[serde(default)]
    pub mode: ExecutionProfileMode,
    /// Defines the filesystem visibility granted to the step.
    #[serde(default)]
    pub fs_mode: ExecutionFsMode,
    /// Additional writable paths when `fs_mode` uses scoped write access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_paths: Vec<String>,
    /// Additional read-only paths granted to sandboxed agents.
    /// Supports `~` (home dir) and `$VAR`/`${VAR}` (env var) expansion.
    /// Only meaningful when `fs_mode` is `WorkspaceReadonly` or `WorkspaceRwScoped`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readable_paths: Vec<String>,
    /// Defines the network reachability granted to the step.
    #[serde(default)]
    pub network_mode: ExecutionNetworkMode,
    /// Explicit network destinations allowed when `network_mode` is `Allowlist`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_allowlist: Vec<String>,
    /// Maximum resident memory in MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u64>,
    /// Maximum accumulated CPU time in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu_seconds: Option<u64>,
    /// Maximum number of child processes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_processes: Option<u64>,
    /// Maximum number of open file descriptors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_open_files: Option<u64>,
}

impl Default for ExecutionProfileConfig {
    fn default() -> Self {
        Self {
            mode: ExecutionProfileMode::Host,
            fs_mode: ExecutionFsMode::Inherit,
            writable_paths: Vec::new(),
            readable_paths: Vec::new(),
            network_mode: ExecutionNetworkMode::Inherit,
            network_allowlist: Vec::new(),
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        }
    }
}

impl ExecutionProfileConfig {
    /// Returns the implicit host profile used when a workflow does not specify one.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orchestrator_config::config::{ExecutionProfileConfig, ExecutionProfileMode};
    ///
    /// let profile = ExecutionProfileConfig::implicit_host();
    /// assert_eq!(profile.mode, ExecutionProfileMode::Host);
    /// ```
    pub fn implicit_host() -> Self {
        Self::default()
    }
}
