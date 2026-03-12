use crate::config::{RunnerConfig, RunnerPolicy};
use anyhow::{anyhow, Result};

/// Enforces runner shell-policy allowlists before command execution.
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
