use crate::config::{
    ExecutionFsMode, ExecutionNetworkMode, ExecutionProfileConfig, ExecutionProfileMode,
    RunnerConfig, RunnerExecutorKind, RunnerPolicy,
};
use crate::output_capture::{spawn_sanitized_output_capture, OutputCaptureHandles};
use anyhow::{anyhow, Context, Result};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_runner_config() -> RunnerConfig {
        RunnerConfig {
            shell: "/bin/bash".to_string(),
            shell_arg: "-lc".to_string(),
            policy: RunnerPolicy::Unsafe,
            executor: RunnerExecutorKind::Shell,
            allowed_shells: vec!["/bin/bash".to_string()],
            allowed_shell_args: vec!["-lc".to_string()],
            env_allowlist: vec!["PATH".to_string()],
            redaction_patterns: vec!["password".to_string()],
        }
    }

    #[test]
    fn test_enforce_runner_policy_allows_valid_command() {
        let runner = make_runner_config();
        let result = enforce_runner_policy(&runner, "echo hello");
        assert!(result.is_ok());
    }

    #[test]
    fn test_enforce_runner_policy_rejects_empty_command() {
        let runner = make_runner_config();
        let result = enforce_runner_policy(&runner, "");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("cannot be empty"));
    }

    #[test]
    fn test_enforce_runner_policy_allows_newline_in_command() {
        let runner = make_runner_config();
        let result = enforce_runner_policy(&runner, "echo hello\nwhoami");
        assert!(result.is_ok(), "newlines are valid in bash -c commands");
    }

    #[test]
    fn test_enforce_runner_policy_rejects_cr_in_command() {
        let runner = make_runner_config();
        let result = enforce_runner_policy(&runner, "echo hello\rwhoami");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("control characters"));
    }

    #[test]
    fn test_enforce_runner_policy_rejects_too_long_command() {
        let runner = make_runner_config();
        let long_command = "x".repeat(131_073);
        let result = enforce_runner_policy(&runner, &long_command);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("too long"));
    }

    #[test]
    fn test_enforce_runner_policy_rejects_disallowed_shell() {
        let mut runner = make_runner_config();
        runner.policy = RunnerPolicy::Allowlist;
        runner.shell = "/bin/sh".to_string();

        let result = enforce_runner_policy(&runner, "echo hello");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("runner.shell"));
    }

    #[test]
    fn test_enforce_runner_policy_rejects_disallowed_shell_arg() {
        let mut runner = make_runner_config();
        runner.policy = RunnerPolicy::Allowlist;
        runner.shell_arg = "-c".to_string();

        let result = enforce_runner_policy(&runner, "echo hello");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("runner.shell_arg"));
    }

    #[test]
    fn test_redact_text_removes_matching_patterns() {
        let patterns = vec!["password".to_string(), "token".to_string()];
        let input = "my password is [REDACTED] and token is [REDACTED]";
        let result = redact_text(input, &patterns);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("password"));
        assert!(!result.contains("token"));
    }

    #[test]
    fn test_redact_text_handles_uppercase_patterns() {
        let patterns = vec!["password".to_string()];
        let input = "PASSWORD is secret";
        let result = redact_text(input, &patterns);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("PASSWORD"));
    }

    #[test]
    fn test_redact_text_case_insensitive() {
        let patterns = vec!["secret".to_string()];
        let input = "My SeCrEt value";
        let result = redact_text(input, &patterns);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("SeCrEt"));
    }

    #[test]
    fn test_redact_text_multiple_case_variants() {
        let patterns = vec!["token".to_string()];
        let input = "token TOKEN Token all here";
        let result = redact_text(input, &patterns);
        assert!(!result.contains("token"));
        assert!(!result.contains("TOKEN"));
        assert!(!result.contains("Token"));
        assert_eq!(result, "[REDACTED] [REDACTED] [REDACTED] all here");
    }

    #[test]
    fn test_redact_text_secret_value_redaction() {
        let patterns = vec!["sk-abc123".to_string()];
        let input = "api key is sk-abc123 in output";
        let result = redact_text(input, &patterns);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("sk-abc123"));
    }

    #[test]
    fn test_redact_text_ignores_empty_patterns() {
        let patterns = vec!["".to_string()];
        let input = "hello world";
        let result = redact_text(input, &patterns);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_classify_sandbox_spawn_error_for_memory_limit() {
        let mut profile = ResolvedExecutionProfile::host();
        profile.name = "sandbox_memory_limit".to_string();
        profile.mode = ExecutionProfileMode::Sandbox;
        profile.max_memory_mb = Some(256);

        let err = io::Error::other("Cannot allocate memory");
        let classified =
            classify_sandbox_spawn_error(&profile, &err).expect("memory spawn error classified");

        assert_eq!(classified.event_type, "sandbox_resource_exceeded");
        assert_eq!(classified.reason_code, "memory_limit_exceeded");
        assert_eq!(
            classified
                .resource_kind
                .as_ref()
                .map(|value| value.as_str()),
            Some("memory")
        );
    }

    #[test]
    fn test_classify_sandbox_spawn_error_uses_single_configured_limit_fallback() {
        let mut profile = ResolvedExecutionProfile::host();
        profile.name = "sandbox_memory_limit".to_string();
        profile.mode = ExecutionProfileMode::Sandbox;
        profile.max_memory_mb = Some(256);

        let err = io::Error::other("spawn failed");
        let classified =
            classify_sandbox_spawn_error(&profile, &err).expect("single-limit fallback");

        assert_eq!(classified.reason_code, "memory_limit_exceeded");
        assert_eq!(
            classified
                .resource_kind
                .as_ref()
                .map(|value| value.as_str()),
            Some("memory")
        );
    }

    #[tokio::test]
    async fn test_spawn_with_runner_allowlist_filters_environment() {
        let temp = tempdir().expect("create tempdir");
        let stdout_path = temp.path().join("stdout.log");
        let stderr_path = temp.path().join("stderr.log");
        let stdout = File::create(&stdout_path).expect("create stdout file");
        let stderr = File::create(&stderr_path).expect("create stderr file");

        let mut runner = make_runner_config();
        runner.policy = RunnerPolicy::Allowlist;
        runner.env_allowlist = vec!["RUNNER_ALLOWED_TEST".to_string()];

        std::env::set_var("RUNNER_ALLOWED_TEST", "visible");
        std::env::set_var("RUNNER_BLOCKED_TEST", "hidden");
        std::env::set_var("CLAUDECODE", "nested-session");

        let mut child = spawn_with_runner(
            &runner,
            "printf '%s|%s|%s' \"${RUNNER_ALLOWED_TEST:-missing}\" \"${RUNNER_BLOCKED_TEST:-missing}\" \"${CLAUDECODE:-missing}\"",
            temp.path(),
            stdout,
            stderr,
            &std::collections::HashMap::new(),
            false,
            &ResolvedExecutionProfile::host(),
        )
        .expect("spawn with allowlist");

        let status = child.wait().await.expect("wait for child");
        std::env::remove_var("RUNNER_ALLOWED_TEST");
        std::env::remove_var("RUNNER_BLOCKED_TEST");
        std::env::remove_var("CLAUDECODE");

        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&stdout_path).expect("read stdout"),
            "visible|missing|missing"
        );
        let stderr_output = std::fs::read_to_string(&stderr_path).expect("read stderr");
        assert!(!stderr_output.contains("RUNNER_ALLOWED_TEST"));
        assert!(!stderr_output.contains("RUNNER_BLOCKED_TEST"));
    }

    #[test]
    fn test_spawn_with_runner_wraps_spawn_errors() {
        let temp = tempdir().expect("create tempdir");
        let stdout_path = temp.path().join("stdout.log");
        let stderr_path = temp.path().join("stderr.log");
        let stdout = File::create(&stdout_path).expect("create stdout file");
        let stderr = File::create(&stderr_path).expect("create stderr file");

        let mut runner = make_runner_config();
        runner.shell = "/definitely/missing-shell".to_string();

        let err = spawn_with_runner(
            &runner,
            "echo hello",
            temp.path(),
            stdout,
            stderr,
            &std::collections::HashMap::new(),
            false,
            &ResolvedExecutionProfile::host(),
        )
        .expect_err("missing shell should fail");
        assert!(err.to_string().contains("failed to spawn runner"));
    }

    #[tokio::test]
    async fn test_spawn_with_extra_env_injects_variables() {
        let temp = tempdir().expect("create tempdir");
        let stdout_path = temp.path().join("stdout.log");
        let stderr_path = temp.path().join("stderr.log");
        let stdout = File::create(&stdout_path).expect("create stdout file");
        let stderr = File::create(&stderr_path).expect("create stderr file");

        let runner = make_runner_config();
        let mut extra_env = std::collections::HashMap::new();
        extra_env.insert("EXTRA_TEST_VAR".to_string(), "injected_value".to_string());

        let mut child = spawn_with_runner(
            &runner,
            "printf '%s' \"${EXTRA_TEST_VAR:-missing}\"",
            temp.path(),
            stdout,
            stderr,
            &extra_env,
            false,
            &ResolvedExecutionProfile::host(),
        )
        .expect("spawn with extra env");

        let status = child.wait().await.expect("wait for child");
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&stdout_path).expect("read stdout"),
            "injected_value"
        );
    }

    #[tokio::test]
    async fn test_spawn_with_runner_and_capture_redacts_persisted_output() {
        let temp = tempdir().expect("create tempdir");
        let stdout_path = temp.path().join("stdout.log");
        let stderr_path = temp.path().join("stderr.log");
        let stdout = File::create(&stdout_path).expect("create stdout file");
        let stderr = File::create(&stderr_path).expect("create stderr file");

        let runner = make_runner_config();
        let captured = spawn_with_runner_and_capture(
            &runner,
            "printf 'api=sk-test-123'; printf ' secret=super-secret-value' >&2",
            temp.path(),
            stdout,
            stderr,
            vec!["sk-test-123".to_string(), "super-secret-value".to_string()],
            &std::collections::HashMap::new(),
            false,
            &ResolvedExecutionProfile::host(),
        )
        .expect("spawn with capture");
        let mut child = captured.child;
        let output_capture = captured.output_capture;

        let status = child.wait().await.expect("wait for child");
        assert!(status.success());
        output_capture
            .wait()
            .await
            .expect("wait for output capture");

        let stdout_output = std::fs::read_to_string(&stdout_path).expect("read stdout");
        let stderr_output = std::fs::read_to_string(&stderr_path).expect("read stderr");
        assert!(!stdout_output.contains("sk-test-123"));
        assert!(stdout_output.contains("[REDACTED]"));
        assert!(!stderr_output.contains("super-secret-value"));
        assert!(stderr_output.contains("[REDACTED]"));
    }
}

pub struct SpawnParams<'a> {
    pub runner: &'a RunnerConfig,
    pub command: &'a str,
    pub cwd: &'a Path,
    pub stdio_mode: RunnerStdioMode,
    pub extra_env: &'a std::collections::HashMap<String, String>,
    pub pipe_stdin: bool,
    pub execution_profile: &'a ResolvedExecutionProfile,
}

pub enum RunnerStdioMode {
    Files { stdout: File, stderr: File },
    Piped,
}

#[derive(Debug, Clone)]
pub struct ResolvedExecutionProfile {
    pub name: String,
    pub mode: ExecutionProfileMode,
    pub fs_mode: ExecutionFsMode,
    pub writable_paths: Vec<PathBuf>,
    pub network_mode: ExecutionNetworkMode,
    pub network_allowlist: Vec<String>,
    pub max_memory_mb: Option<u64>,
    pub max_cpu_seconds: Option<u64>,
    pub max_processes: Option<u64>,
    pub max_open_files: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxResourceKind {
    Memory,
    Cpu,
    Processes,
    OpenFiles,
}

impl SandboxResourceKind {
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
pub struct SandboxBackendError {
    pub execution_profile: String,
    pub backend: &'static str,
    pub event_type: &'static str,
    pub reason_code: &'static str,
    pub resource_kind: Option<SandboxResourceKind>,
    message: String,
}

impl SandboxBackendError {
    #[cfg(target_os = "macos")]
    fn unsupported_network_allowlist(execution_profile: &ResolvedExecutionProfile) -> Self {
        Self {
            execution_profile: execution_profile.name.clone(),
            backend: sandbox_backend_label(execution_profile),
            event_type: "sandbox_network_blocked",
            reason_code: "unsupported_backend_feature",
            resource_kind: None,
            message: format!(
                "sandbox backend '{}' does not support network allowlists for execution profile '{}'",
                sandbox_backend_label(execution_profile),
                execution_profile.name
            ),
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn backend_unavailable(execution_profile: &ResolvedExecutionProfile) -> Self {
        Self {
            execution_profile: execution_profile.name.clone(),
            backend: sandbox_backend_label(execution_profile),
            event_type: "sandbox_denied",
            reason_code: "sandbox_backend_unavailable",
            resource_kind: None,
            message: format!(
                "sandbox execution is not implemented on this platform for execution profile '{}'",
                execution_profile.name
            ),
        }
    }

    fn resource_exhausted(
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

#[derive(Debug, Clone)]
struct UnixResourceLimits {
    max_memory_bytes: Option<u64>,
    max_cpu_seconds: Option<u64>,
    max_processes: Option<u64>,
    max_open_files: Option<u64>,
}

impl UnixResourceLimits {
    fn from_execution_profile(execution_profile: &ResolvedExecutionProfile) -> Option<Self> {
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

impl ResolvedExecutionProfile {
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

pub fn sandbox_backend_label(execution_profile: &ResolvedExecutionProfile) -> &'static str {
    match execution_profile.mode {
        ExecutionProfileMode::Host => "host",
        ExecutionProfileMode::Sandbox => {
            #[cfg(target_os = "macos")]
            {
                "macos_seatbelt"
            }
            #[cfg(not(target_os = "macos"))]
            {
                "sandbox_unavailable"
            }
        }
    }
}

pub fn validate_execution_profile_support(
    execution_profile: &ResolvedExecutionProfile,
) -> Result<()> {
    if execution_profile.mode != ExecutionProfileMode::Sandbox {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        if execution_profile.network_mode == ExecutionNetworkMode::Allowlist {
            return Err(
                SandboxBackendError::unsupported_network_allowlist(execution_profile).into(),
            );
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(SandboxBackendError::backend_unavailable(execution_profile).into())
    }
}

pub trait RunnerExecutor {
    fn spawn(&self, params: SpawnParams<'_>) -> Result<tokio::process::Child>;
}

#[derive(Debug, Default)]
pub struct ShellRunnerExecutor;

impl RunnerExecutor for ShellRunnerExecutor {
    fn spawn(&self, params: SpawnParams<'_>) -> Result<tokio::process::Child> {
        let SpawnParams {
            runner,
            command,
            cwd,
            stdio_mode,
            extra_env,
            pipe_stdin,
            execution_profile,
        } = params;

        enforce_runner_policy(runner, command)?;
        let mut cmd = build_command_for_profile(runner, command, cwd, execution_profile)?;

        match stdio_mode {
            RunnerStdioMode::Files { stdout, stderr } => {
                cmd.stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
            }
            RunnerStdioMode::Piped => {
                cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            }
        }

        cmd.kill_on_drop(true);

        if pipe_stdin {
            cmd.stdin(Stdio::piped());
        }

        #[cfg(unix)]
        {
            cmd.process_group(0); // child becomes its own process group leader
            apply_unix_resource_limits_to_command(&mut cmd, execution_profile)?;
        }

        if runner.policy == RunnerPolicy::Allowlist {
            cmd.env_clear();
            for key in &runner.env_allowlist {
                if let Ok(value) = std::env::var(key) {
                    cmd.env(key, value);
                }
            }
        }

        // Inject agent-specific extra env vars (from EnvStore/SecretStore/direct)
        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        // Remove CLAUDECODE env var so spawned `claude -p` processes don't
        // refuse to start due to nested session detection.
        cmd.env_remove("CLAUDECODE");

        match cmd.spawn() {
            Ok(child) => Ok(child),
            Err(err) => {
                if let Some(sandbox_err) = classify_sandbox_spawn_error(execution_profile, &err) {
                    return Err(sandbox_err.into());
                }
                Err(err).with_context(|| {
                    format!(
                        "failed to spawn runner shell={} shell_arg={}",
                        runner.shell, runner.shell_arg
                    )
                })
            }
        }
    }
}

fn build_command_for_profile(
    runner: &RunnerConfig,
    command: &str,
    cwd: &Path,
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

fn classify_sandbox_spawn_error(
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

fn build_sandbox_command(
    runner: &RunnerConfig,
    command: &str,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<tokio::process::Command> {
    #[cfg(target_os = "macos")]
    {
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
        let _ = (runner, command, execution_profile);
        Err(SandboxBackendError::backend_unavailable(execution_profile).into())
    }
}

#[cfg(target_os = "macos")]
fn build_macos_sandbox_profile(execution_profile: &ResolvedExecutionProfile) -> String {
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
    lines.join("\n")
}

#[cfg(target_os = "macos")]
fn escape_sb_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(unix)]
fn apply_unix_resource_limits_to_command(
    cmd: &mut tokio::process::Command,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<()> {
    let Some(limits) = UnixResourceLimits::from_execution_profile(execution_profile) else {
        return Ok(());
    };
    // Apply rlimits in the child just before exec so the sandbox wrapper and
    // the eventual agent process inherit the same enforcement boundary.
    unsafe {
        cmd.pre_exec(move || apply_unix_resource_limits(&limits).map_err(io::Error::other));
    }
    Ok(())
}

#[cfg(unix)]
fn apply_unix_resource_limits(limits: &UnixResourceLimits) -> Result<()> {
    if let Some(value) = limits.max_memory_bytes {
        set_rlimit(rlimit_resource(libc::RLIMIT_AS as u64)?, value)?;
    }
    if let Some(value) = limits.max_cpu_seconds {
        set_rlimit(rlimit_resource(libc::RLIMIT_CPU as u64)?, value)?;
    }
    if let Some(value) = limits.max_processes {
        set_rlimit(rlimit_resource(libc::RLIMIT_NPROC as u64)?, value)?;
    }
    if let Some(value) = limits.max_open_files {
        set_rlimit(rlimit_resource(libc::RLIMIT_NOFILE as u64)?, value)?;
    }
    Ok(())
}

#[cfg(unix)]
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type RlimitResource = libc::__rlimit_resource_t;

#[cfg(unix)]
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type RlimitResource = libc::c_int;

#[cfg(unix)]
fn rlimit_resource(resource: u64) -> Result<RlimitResource> {
    // libc exposes RLIMIT_* with target-specific integer types. Convert via a
    // wide intermediate so Linux GNU x86/u32 and Darwin/i32 both type-check.
    RlimitResource::try_from(resource)
        .map_err(|_| anyhow!("unsupported rlimit resource selector: {resource}"))
}

#[cfg(unix)]
fn set_rlimit(resource: RlimitResource, value: u64) -> Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: `setrlimit` is called in the child process before exec with a
    // valid resource selector and initialized `rlimit` struct.
    let rc = unsafe { libc::setrlimit(resource, &limit) };
    if rc == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "setrlimit({resource}) failed: {}",
            io::Error::last_os_error()
        ))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_with_runner(
    runner: &RunnerConfig,
    command: &str,
    cwd: &Path,
    stdout: File,
    stderr: File,
    extra_env: &std::collections::HashMap<String, String>,
    pipe_stdin: bool,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<tokio::process::Child> {
    match runner.executor {
        RunnerExecutorKind::Shell => ShellRunnerExecutor.spawn(SpawnParams {
            runner,
            command,
            cwd,
            stdio_mode: RunnerStdioMode::Files { stdout, stderr },
            extra_env,
            pipe_stdin,
            execution_profile,
        }),
    }
}

pub struct CapturedChild {
    pub child: tokio::process::Child,
    pub output_capture: OutputCaptureHandles,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_with_runner_and_capture(
    runner: &RunnerConfig,
    command: &str,
    cwd: &Path,
    stdout: File,
    stderr: File,
    redaction_patterns: Vec<String>,
    extra_env: &std::collections::HashMap<String, String>,
    pipe_stdin: bool,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<CapturedChild> {
    let mut child = match runner.executor {
        RunnerExecutorKind::Shell => ShellRunnerExecutor.spawn(SpawnParams {
            runner,
            command,
            cwd,
            stdio_mode: RunnerStdioMode::Piped,
            extra_env,
            pipe_stdin,
            execution_profile,
        })?,
    };
    let child_stdout = child
        .stdout
        .take()
        .context("captured runner child missing stdout pipe")?;
    let child_stderr = child
        .stderr
        .take()
        .context("captured runner child missing stderr pipe")?;
    let output_capture = spawn_sanitized_output_capture(
        child_stdout,
        child_stderr,
        stdout,
        stderr,
        redaction_patterns,
    );
    Ok(CapturedChild {
        child,
        output_capture,
    })
}

pub fn enforce_runner_policy(runner: &RunnerConfig, command: &str) -> Result<()> {
    if command.trim().is_empty() {
        return Err(anyhow!("runner command cannot be empty"));
    }
    if command.contains('\0') || command.contains('\r') {
        return Err(anyhow!(
            "runner command contains blocked control characters (NUL/CR)"
        ));
    }
    if command.len() > 131_072 {
        return Err(anyhow!("runner command too long (>131072 bytes)"));
    }

    if runner.policy == RunnerPolicy::Allowlist {
        if !runner
            .allowed_shells
            .iter()
            .any(|item| item == &runner.shell)
        {
            return Err(anyhow!(
                "runner.shell '{}' is not in runner.allowed_shells",
                runner.shell
            ));
        }
        if !runner
            .allowed_shell_args
            .iter()
            .any(|item| item == &runner.shell_arg)
        {
            return Err(anyhow!(
                "runner.shell_arg '{}' is not in runner.allowed_shell_args",
                runner.shell_arg
            ));
        }
    }
    Ok(())
}

/// Kill the entire process group rooted at the child process.
///
/// Because we spawn children with `process_group(0)`, the child PID equals
/// its PGID.  Sending `SIGKILL` to the negated PID kills every process in
/// that group (child + all descendants).  On non-Unix platforms we fall back
/// to the regular per-process kill.
pub async fn kill_child_process_group(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(unix)]
        {
            // SAFETY: kill(-pid, SIGKILL) is a POSIX syscall that sends a
            // signal to a process group.  The pid was obtained from a child we
            // spawned, so the group exists and belongs to us.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = child.kill().await;
        }
    } else {
        let _ = child.kill().await;
    }
}

pub fn redact_text(raw: &str, patterns: &[String]) -> String {
    let mut out = raw.to_string();
    for token in patterns {
        if token.trim().is_empty() {
            continue;
        }
        let lower_token = token.to_lowercase();
        let mut result = String::with_capacity(out.len());
        let lower_out = out.to_lowercase();
        let mut last = 0;
        for (idx, _) in lower_out.match_indices(lower_token.as_str()) {
            result.push_str(&out[last..idx]);
            result.push_str("[REDACTED]");
            last = idx + token.len();
        }
        result.push_str(&out[last..]);
        out = result;
    }
    out
}
