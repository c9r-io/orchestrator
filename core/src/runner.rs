use crate::config::{RunnerConfig, RunnerExecutorKind, RunnerPolicy};
use anyhow::{anyhow, Context, Result};
use std::fs::File;
use std::path::Path;
use std::process::Stdio;

pub trait RunnerExecutor {
    fn spawn(
        &self,
        runner: &RunnerConfig,
        command: &str,
        cwd: &Path,
        stdout: File,
        stderr: File,
    ) -> Result<tokio::process::Child>;
}

#[derive(Debug, Default)]
pub struct ShellRunnerExecutor;

impl RunnerExecutor for ShellRunnerExecutor {
    fn spawn(
        &self,
        runner: &RunnerConfig,
        command: &str,
        cwd: &Path,
        stdout: File,
        stderr: File,
    ) -> Result<tokio::process::Child> {
        enforce_runner_policy(runner, command)?;

        let mut cmd = tokio::process::Command::new(&runner.shell);
        cmd.arg(&runner.shell_arg)
            .arg(command)
            .current_dir(cwd)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);

        if runner.policy == RunnerPolicy::Allowlist {
            cmd.env_clear();
            for key in &runner.env_allowlist {
                if let Ok(value) = std::env::var(key) {
                    cmd.env(key, value);
                }
            }
        }

        cmd.spawn().with_context(|| {
            format!(
                "failed to spawn runner shell={} shell_arg={}",
                runner.shell, runner.shell_arg
            )
        })
    }
}

pub fn spawn_with_runner(
    runner: &RunnerConfig,
    command: &str,
    cwd: &Path,
    stdout: File,
    stderr: File,
) -> Result<tokio::process::Child> {
    match runner.executor {
        RunnerExecutorKind::Shell => {
            ShellRunnerExecutor.spawn(runner, command, cwd, stdout, stderr)
        }
    }
}

pub fn enforce_runner_policy(runner: &RunnerConfig, command: &str) -> Result<()> {
    if command.trim().is_empty() {
        return Err(anyhow!("runner command cannot be empty"));
    }
    if command.contains('\0') || command.contains('\n') || command.contains('\r') {
        return Err(anyhow!(
            "runner command contains blocked control characters (NUL/newline)"
        ));
    }
    if command.len() > 16_384 {
        return Err(anyhow!("runner command too long (>16384 bytes)"));
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

pub fn redact_text(raw: &str, patterns: &[String]) -> String {
    let mut out = raw.to_string();
    for token in patterns {
        if token.trim().is_empty() {
            continue;
        }
        out = out.replace(token, "[REDACTED]");
        out = out.replace(&token.to_uppercase(), "[REDACTED]");
    }
    out
}
