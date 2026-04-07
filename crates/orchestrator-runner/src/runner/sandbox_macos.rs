#[cfg(target_os = "macos")]
use super::profile::ResolvedExecutionProfile;
#[cfg(target_os = "macos")]
use orchestrator_config::config::{ExecutionFsMode, ExecutionNetworkMode};
#[cfg(target_os = "macos")]
use std::path::Path;

#[cfg(target_os = "macos")]
pub(crate) fn build_macos_sandbox_profile(execution_profile: &ResolvedExecutionProfile) -> String {
    let mut lines = vec![
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        "(import \"system.sb\")".to_string(),
        "(allow process*)".to_string(),
        "(allow file-read*)".to_string(),
        "(allow sysctl-read)".to_string(),
    ];
    if execution_profile.network_mode != ExecutionNetworkMode::Deny {
        lines.push("(allow network*)".to_string());
    }
    match execution_profile.fs_mode {
        ExecutionFsMode::Inherit => {
            lines.push("(allow file-write*)".to_string());
        }
        ExecutionFsMode::WorkspaceReadonly | ExecutionFsMode::WorkspaceRwScoped => {
            if !execution_profile.writable_paths.is_empty() {
                lines.push("(allow file-write*".to_string());
                for path in &execution_profile.writable_paths {
                    lines.push(format!("    (subpath \"{}\")", escape_sb_string(path)));
                }
                lines.push(")".to_string());
            }
        }
    }
    // FR-093: `readable_paths` is currently a no-op on macOS because the
    // profile above unconditionally emits `(allow file-read*)`. The
    // ORCHESTRATOR_READABLE_PATHS env var is still propagated to agent
    // wrapper scripts so that agent-CLI-specific sandboxes can apply it.
    // If the macOS profile ever becomes read-restrictive, emit explicit
    // `(allow file-read* (subpath ...))` rules for readable_paths here.
    let _ = &execution_profile.readable_paths;
    lines.join("\n")
}

#[cfg(target_os = "macos")]
pub(crate) fn escape_sb_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}
