use super::policy::enforce_runner_policy;
use super::profile::ResolvedExecutionProfile;
use super::sandbox::{build_command_for_profile, classify_sandbox_spawn_error};
use crate::config::{RunnerConfig, RunnerExecutorKind, RunnerPolicy};
use crate::output_capture::{spawn_sanitized_output_capture, OutputCaptureHandles};
use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;
use std::process::Stdio;

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
            use super::resource_limits::apply_unix_resource_limits_to_command;
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
