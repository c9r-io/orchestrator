use orchestrator_config::config::{
    ExecutionFsMode, ExecutionNetworkMode, ExecutionProfileConfig, ExecutionProfileMode,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
/// Execution profile after resolving workspace-relative paths and inherited defaults.
pub struct ResolvedExecutionProfile {
    /// Name of the execution profile that produced this resolved view.
    pub name: String,
    /// Whether commands run on the host or inside a sandbox backend.
    pub mode: ExecutionProfileMode,
    /// Filesystem policy enforced by the execution backend.
    pub fs_mode: ExecutionFsMode,
    /// Paths that remain writable when sandboxing is enabled.
    pub writable_paths: Vec<PathBuf>,
    /// Network policy enforced by the execution backend.
    pub network_mode: ExecutionNetworkMode,
    /// Raw allowlist entries used when `network_mode=allowlist`.
    pub network_allowlist: Vec<String>,
    /// Optional memory limit in MiB.
    pub max_memory_mb: Option<u64>,
    /// Optional CPU time limit in seconds.
    pub max_cpu_seconds: Option<u64>,
    /// Optional maximum process count.
    pub max_processes: Option<u64>,
    /// Optional file-descriptor limit.
    pub max_open_files: Option<u64>,
}

impl ResolvedExecutionProfile {
    /// Returns the built-in host execution profile with no sandbox limits.
    pub fn host() -> Self {
        Self {
            name: "host".to_string(),
            mode: ExecutionProfileMode::Host,
            fs_mode: ExecutionFsMode::Inherit,
            writable_paths: Vec::new(),
            network_mode: ExecutionNetworkMode::Inherit,
            network_allowlist: Vec::new(),
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        }
    }

    /// Resolves a configured execution profile against the workspace root.
    pub fn from_config(
        name: &str,
        config: &ExecutionProfileConfig,
        workspace_root: &Path,
        always_writable: &[PathBuf],
    ) -> Self {
        let mut writable_paths = always_writable.to_vec();
        writable_paths.extend(config.writable_paths.iter().map(|path| {
            let raw = PathBuf::from(path);
            if raw.is_absolute() {
                raw
            } else {
                workspace_root.join(raw)
            }
        }));
        Self {
            name: name.to_string(),
            mode: config.mode.clone(),
            fs_mode: config.fs_mode.clone(),
            writable_paths,
            network_mode: config.network_mode.clone(),
            network_allowlist: config.network_allowlist.clone(),
            max_memory_mb: config.max_memory_mb,
            max_cpu_seconds: config.max_cpu_seconds,
            max_processes: config.max_processes,
            max_open_files: config.max_open_files,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UnixResourceLimits {
    pub(crate) max_memory_bytes: Option<u64>,
    pub(crate) max_cpu_seconds: Option<u64>,
    pub(crate) max_processes: Option<u64>,
    pub(crate) max_open_files: Option<u64>,
}

impl UnixResourceLimits {
    pub(crate) fn from_execution_profile(
        execution_profile: &ResolvedExecutionProfile,
    ) -> Option<Self> {
        let limits = Self {
            max_memory_bytes: execution_profile
                .max_memory_mb
                .map(|value| value.saturating_mul(1024 * 1024)),
            max_cpu_seconds: execution_profile.max_cpu_seconds,
            max_processes: execution_profile.max_processes,
            max_open_files: execution_profile.max_open_files,
        };
        if limits.max_memory_bytes.is_none()
            && limits.max_cpu_seconds.is_none()
            && limits.max_processes.is_none()
            && limits.max_open_files.is_none()
        {
            None
        } else {
            Some(limits)
        }
    }
}
