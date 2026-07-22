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
        "(allow sysctl-read)".to_string(),
    ];
    if execution_profile.strict_read_paths {
        lines.push("(allow file-read*".to_string());
        for system_path in [
            "/System",
            "/usr",
            "/bin",
            "/sbin",
            "/Library/Apple",
            "/private/etc",
            "/private/var/db",
            "/dev",
            "/opt/homebrew",
        ] {
            lines.push(format!("    (subpath \"{system_path}\")"));
        }
        if let Some(workspace) = execution_profile.workspace_root.as_deref() {
            lines.push(format!("    (subpath \"{}\")", escape_sb_string(workspace)));
        }
        for path in &execution_profile.readable_paths {
            lines.push(format!("    (subpath \"{}\")", escape_sb_string(path)));
        }
        lines.push(")".to_string());
    } else {
        lines.push("(allow file-read*)".to_string());
    }
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
    if execution_profile.strict_read_paths && !execution_profile.readable_paths.is_empty() {
        lines.push("(deny file-write*".to_string());
        for path in &execution_profile.readable_paths {
            lines.push(format!("    (subpath \"{}\")", escape_sb_string(path)));
        }
        lines.push(")".to_string());
    }
    lines.join("\n")
}

#[cfg(target_os = "macos")]
pub(crate) fn escape_sb_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}
