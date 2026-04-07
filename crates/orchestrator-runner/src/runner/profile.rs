use crate::runner::path_expand::expand_path;
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
    /// Additional paths granted read-only access when sandboxing is enabled.
    pub readable_paths: Vec<PathBuf>,
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
    /// Workspace root directory (needed by Linux mount-namespace filesystem isolation).
    pub workspace_root: Option<PathBuf>,
}

impl ResolvedExecutionProfile {
    /// Returns the built-in host execution profile with no sandbox limits.
    pub fn host() -> Self {
        Self {
            name: "host".to_string(),
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
            workspace_root: None,
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
            let expanded = expand_path(path);
            if expanded.is_absolute() {
                expanded
            } else {
                workspace_root.join(expanded)
            }
        }));
        let readable_paths: Vec<PathBuf> = config
            .readable_paths
            .iter()
            .map(|path| {
                let expanded = expand_path(path);
                if expanded.is_absolute() {
                    expanded
                } else {
                    workspace_root.join(expanded)
                }
            })
            .collect();
        Self {
            name: name.to_string(),
            mode: config.mode.clone(),
            fs_mode: config.fs_mode.clone(),
            writable_paths,
            readable_paths,
            network_mode: config.network_mode.clone(),
            network_allowlist: config.network_allowlist.clone(),
            max_memory_mb: config.max_memory_mb,
            max_cpu_seconds: config.max_cpu_seconds,
            max_processes: config.max_processes,
            max_open_files: config.max_open_files,
            workspace_root: Some(workspace_root.to_path_buf()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_config::config::ExecutionProfileConfig;

    fn config_with_paths(writable: Vec<&str>, readable: Vec<&str>) -> ExecutionProfileConfig {
        ExecutionProfileConfig {
            writable_paths: writable.into_iter().map(String::from).collect(),
            readable_paths: readable.into_iter().map(String::from).collect(),
            ..ExecutionProfileConfig::default()
        }
    }

    #[test]
    fn from_config_resolves_absolute_readable_paths_unchanged() {
        let cfg = config_with_paths(vec![], vec!["/absolute/path", "/var/cache"]);
        let resolved = ResolvedExecutionProfile::from_config(
            "test",
            &cfg,
            Path::new("/workspace/proj"),
            &[],
        );
        assert_eq!(
            resolved.readable_paths,
            vec![PathBuf::from("/absolute/path"), PathBuf::from("/var/cache")]
        );
    }

    #[test]
    fn from_config_joins_relative_readable_paths_to_workspace() {
        let cfg = config_with_paths(vec![], vec!["shared/data"]);
        let resolved = ResolvedExecutionProfile::from_config(
            "test",
            &cfg,
            Path::new("/workspace/proj"),
            &[],
        );
        assert_eq!(
            resolved.readable_paths,
            vec![PathBuf::from("/workspace/proj/shared/data")]
        );
    }

    #[test]
    fn from_config_expands_tilde_in_readable_paths() {
        // SAFETY: tests in this crate run sequentially per-binary by default,
        // and we restore HOME after the test.
        let prev = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", "/users/test");
        }
        let cfg = config_with_paths(vec![], vec!["~/.orchestratord/logs"]);
        let resolved = ResolvedExecutionProfile::from_config(
            "test",
            &cfg,
            Path::new("/workspace/proj"),
            &[],
        );
        assert_eq!(
            resolved.readable_paths,
            vec![PathBuf::from("/users/test/.orchestratord/logs")]
        );
        unsafe {
            match prev {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn from_config_empty_readable_paths_yields_empty_vec() {
        let cfg = ExecutionProfileConfig::default();
        let resolved = ResolvedExecutionProfile::from_config(
            "test",
            &cfg,
            Path::new("/workspace/proj"),
            &[],
        );
        assert!(resolved.readable_paths.is_empty());
    }
}
