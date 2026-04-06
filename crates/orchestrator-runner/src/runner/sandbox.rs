use super::profile::ResolvedExecutionProfile;
use anyhow::Result;
#[cfg(target_os = "linux")]
use orchestrator_config::config::ExecutionFsMode;
use orchestrator_config::config::{ExecutionNetworkMode, ExecutionProfileMode, RunnerConfig};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Variants are platform-specific; not all used on every OS.
pub(crate) enum SandboxBackend {
    Host,
    MacosSeatbelt,
    LinuxNative,
    Unavailable,
}

impl SandboxBackend {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::MacosSeatbelt => "macos_seatbelt",
            Self::LinuxNative => "linux_native",
            Self::Unavailable => "sandbox_unavailable",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LinuxSandboxSupport {
    pub(crate) backend: SandboxBackend,
    pub(crate) missing_requirements: Vec<String>,
}

impl LinuxSandboxSupport {
    pub(crate) fn available(&self) -> bool {
        self.backend == SandboxBackend::LinuxNative && self.missing_requirements.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Resource limits that can trigger sandbox spawn failures.
pub enum SandboxResourceKind {
    /// Memory limit exhaustion.
    Memory,
    /// CPU time limit exhaustion.
    Cpu,
    /// Process-count limit exhaustion.
    Processes,
    /// File-descriptor limit exhaustion.
    OpenFiles,
}

impl SandboxResourceKind {
    /// Returns the stable event payload label for the resource kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Cpu => "cpu",
            Self::Processes => "processes",
            Self::OpenFiles => "open_files",
        }
    }
}

#[derive(Debug)]
/// Structured error emitted when sandbox backend selection or execution fails.
pub struct SandboxBackendError {
    /// Name of the execution profile that triggered the error.
    pub execution_profile: String,
    /// Label of the selected or attempted sandbox backend.
    pub backend: &'static str,
    /// Event type emitted to observability and callers.
    pub event_type: &'static str,
    /// Stable reason code for programmatic handling.
    pub reason_code: &'static str,
    /// Resource limit kind when the error was caused by resource exhaustion.
    pub resource_kind: Option<SandboxResourceKind>,
    message: String,
}

impl SandboxBackendError {
    pub(crate) fn unsupported_network_allowlist(
        execution_profile: &ResolvedExecutionProfile,
        backend: SandboxBackend,
    ) -> Self {
        Self {
            execution_profile: execution_profile.name.clone(),
            backend: backend.label(),
            event_type: "sandbox_network_blocked",
            reason_code: "unsupported_backend_feature",
            resource_kind: None,
            message: format!(
                "sandbox backend '{}' does not support network allowlists for execution profile '{}'",
                backend.label(),
                execution_profile.name
            ),
        }
    }

    pub(crate) fn backend_unavailable(
        execution_profile: &ResolvedExecutionProfile,
        backend: SandboxBackend,
        detail: Option<&str>,
    ) -> Self {
        let suffix = detail
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!(": {value}"))
            .unwrap_or_default();
        Self {
            execution_profile: execution_profile.name.clone(),
            backend: backend.label(),
            event_type: "sandbox_denied",
            reason_code: "sandbox_backend_unavailable",
            resource_kind: None,
            message: format!(
                "sandbox backend '{}' is unavailable for execution profile '{}'{}",
                backend.label(),
                execution_profile.name,
                suffix
            ),
        }
    }

    pub(crate) fn resource_exhausted(
        execution_profile: &ResolvedExecutionProfile,
        resource_kind: SandboxResourceKind,
        source: &io::Error,
    ) -> Self {
        let reason_code = match resource_kind {
            SandboxResourceKind::Memory => "memory_limit_exceeded",
            SandboxResourceKind::Cpu => "cpu_limit_exceeded",
            SandboxResourceKind::Processes => "processes_limit_exceeded",
            SandboxResourceKind::OpenFiles => "open_files_limit_exceeded",
        };
        Self {
            execution_profile: execution_profile.name.clone(),
            backend: sandbox_backend_label(execution_profile),
            event_type: "sandbox_resource_exceeded",
            reason_code,
            resource_kind: Some(resource_kind),
            message: format!(
                "sandbox process spawn failed under execution profile '{}': {}",
                execution_profile.name, source
            ),
        }
    }
}

impl std::fmt::Display for SandboxBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SandboxBackendError {}

/// Returns the effective sandbox backend label for an execution profile.
pub fn sandbox_backend_label(execution_profile: &ResolvedExecutionProfile) -> &'static str {
    select_sandbox_backend(execution_profile).label()
}

/// Validates that the current host can satisfy the requested execution profile.
pub fn validate_execution_profile_support(
    execution_profile: &ResolvedExecutionProfile,
) -> Result<()> {
    if execution_profile.mode != ExecutionProfileMode::Sandbox {
        return Ok(());
    }
    let backend = select_sandbox_backend(execution_profile);
    match backend {
        SandboxBackend::Host => Ok(()),
        SandboxBackend::MacosSeatbelt => {
            if execution_profile.network_mode == ExecutionNetworkMode::Allowlist {
                return Err(SandboxBackendError::unsupported_network_allowlist(
                    execution_profile,
                    backend,
                )
                .into());
            }
            Ok(())
        }
        SandboxBackend::LinuxNative => {
            let support = detect_linux_sandbox_support(execution_profile);
            if support.available() {
                Ok(())
            } else {
                Err(SandboxBackendError::backend_unavailable(
                    execution_profile,
                    support.backend,
                    Some(&support.missing_requirements.join(", ")),
                )
                .into())
            }
        }
        SandboxBackend::Unavailable => {
            Err(SandboxBackendError::backend_unavailable(execution_profile, backend, None).into())
        }
    }
}

/// Returns non-fatal preflight issues for the execution profile's sandbox backend.
pub fn sandbox_backend_preflight_issues(
    execution_profile: &ResolvedExecutionProfile,
) -> Vec<String> {
    if execution_profile.mode != ExecutionProfileMode::Sandbox {
        return Vec::new();
    }
    match select_sandbox_backend(execution_profile) {
        SandboxBackend::LinuxNative => {
            detect_linux_sandbox_support(execution_profile).missing_requirements
        }
        SandboxBackend::MacosSeatbelt
            if execution_profile.network_mode == ExecutionNetworkMode::Allowlist =>
        {
            vec!["macos_seatbelt does not support network_mode=allowlist".to_string()]
        }
        SandboxBackend::Unavailable => {
            vec!["sandbox backend is unavailable on this platform".to_string()]
        }
        _ => Vec::new(),
    }
}

pub(crate) fn select_sandbox_backend(
    execution_profile: &ResolvedExecutionProfile,
) -> SandboxBackend {
    match execution_profile.mode {
        ExecutionProfileMode::Host => SandboxBackend::Host,
        ExecutionProfileMode::Sandbox => {
            #[cfg(target_os = "macos")]
            {
                SandboxBackend::MacosSeatbelt
            }
            #[cfg(target_os = "linux")]
            {
                SandboxBackend::LinuxNative
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                SandboxBackend::Unavailable
            }
        }
    }
}

pub(crate) fn detect_linux_sandbox_support(
    execution_profile: &ResolvedExecutionProfile,
) -> LinuxSandboxSupport {
    #[cfg(target_os = "linux")]
    {
        use super::sandbox_linux::command_exists;

        let mut missing = Vec::new();
        for binary in ["ip", "nft"] {
            if !command_exists(binary) {
                missing.push(format!("missing '{binary}' in PATH"));
            }
        }
        if execution_profile.fs_mode != ExecutionFsMode::Inherit {
            for binary in ["unshare", "mount"] {
                if !command_exists(binary) {
                    missing.push(format!(
                        "linux_native fs_mode={:?} requires '{binary}' in PATH",
                        execution_profile.fs_mode
                    ));
                }
            }
        }
        if nix::unistd::geteuid().as_raw() != 0 {
            missing.push("linux_native requires the daemon to run as root".to_string());
        }
        LinuxSandboxSupport {
            backend: SandboxBackend::LinuxNative,
            missing_requirements: missing,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = execution_profile;
        LinuxSandboxSupport {
            backend: SandboxBackend::Unavailable,
            missing_requirements: vec![
                "linux_native backend is only available on Linux".to_string(),
            ],
        }
    }
}

pub(crate) fn classify_sandbox_spawn_error(
    execution_profile: &ResolvedExecutionProfile,
    err: &io::Error,
) -> Option<SandboxBackendError> {
    if execution_profile.mode != ExecutionProfileMode::Sandbox {
        return None;
    }
    let lower = err.to_string().to_lowercase();
    if execution_profile.max_memory_mb.is_some()
        && (lower.contains("cannot allocate memory")
            || lower.contains("not enough space")
            || lower.contains("not enough memory")
            || lower.contains("memory"))
    {
        return Some(SandboxBackendError::resource_exhausted(
            execution_profile,
            SandboxResourceKind::Memory,
            err,
        ));
    }
    if execution_profile.max_processes.is_some()
        && lower.contains("resource temporarily unavailable")
    {
        return Some(SandboxBackendError::resource_exhausted(
            execution_profile,
            SandboxResourceKind::Processes,
            err,
        ));
    }
    if execution_profile.max_open_files.is_some() && lower.contains("too many open files") {
        return Some(SandboxBackendError::resource_exhausted(
            execution_profile,
            SandboxResourceKind::OpenFiles,
            err,
        ));
    }
    let mut configured_limits = Vec::new();
    if execution_profile.max_memory_mb.is_some() {
        configured_limits.push(SandboxResourceKind::Memory);
    }
    if execution_profile.max_processes.is_some() {
        configured_limits.push(SandboxResourceKind::Processes);
    }
    if execution_profile.max_open_files.is_some() {
        configured_limits.push(SandboxResourceKind::OpenFiles);
    }
    if execution_profile.max_cpu_seconds.is_some() {
        configured_limits.push(SandboxResourceKind::Cpu);
    }
    if configured_limits.len() == 1 {
        return Some(SandboxBackendError::resource_exhausted(
            execution_profile,
            configured_limits.remove(0),
            err,
        ));
    }
    None
}

/// Builds a `tokio::process::Command` that runs the given command string under
/// the sandbox backend selected by the execution profile (Host, macOS Seatbelt,
/// or Linux Native).
pub fn build_command_for_profile(
    runner: &RunnerConfig,
    command: &str,
    cwd: &std::path::Path,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<tokio::process::Command> {
    let mut cmd = match execution_profile.mode {
        ExecutionProfileMode::Host => {
            let mut cmd = tokio::process::Command::new(&runner.shell);
            cmd.arg(&runner.shell_arg).arg(command);
            cmd
        }
        ExecutionProfileMode::Sandbox => build_sandbox_command(runner, command, execution_profile)?,
    };
    cmd.current_dir(cwd);
    Ok(cmd)
}

pub(crate) fn build_sandbox_command(
    runner: &RunnerConfig,
    command: &str,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<tokio::process::Command> {
    let backend = select_sandbox_backend(execution_profile);
    match backend {
        SandboxBackend::MacosSeatbelt => {
            #[cfg(target_os = "macos")]
            {
                use super::sandbox_macos::build_macos_sandbox_profile;
                let mut cmd = tokio::process::Command::new("/usr/bin/sandbox-exec");
                cmd.arg("-p")
                    .arg(build_macos_sandbox_profile(execution_profile))
                    .arg(&runner.shell)
                    .arg(&runner.shell_arg)
                    .arg(command);
                Ok(cmd)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (runner, command);
                Err(
                    SandboxBackendError::backend_unavailable(execution_profile, backend, None)
                        .into(),
                )
            }
        }
        SandboxBackend::LinuxNative => {
            #[cfg(target_os = "linux")]
            {
                use super::sandbox_linux::build_linux_sandbox_command;
                build_linux_sandbox_command(runner, command, execution_profile)
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (runner, command);
                Err(
                    SandboxBackendError::backend_unavailable(execution_profile, backend, None)
                        .into(),
                )
            }
        }
        _ => Err(SandboxBackendError::backend_unavailable(execution_profile, backend, None).into()),
    }
}
