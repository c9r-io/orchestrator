use crate::config::{RunnerConfig, RunnerExecutorKind, RunnerPolicy};
use anyhow::{anyhow, Context, Result};
use std::fs::File;
use std::path::Path;
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
        )
        .expect("spawn with extra env");

        let status = child.wait().await.expect("wait for child");
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&stdout_path).expect("read stdout"),
            "injected_value"
        );
    }
}

pub struct SpawnParams<'a> {
    pub runner: &'a RunnerConfig,
    pub command: &'a str,
    pub cwd: &'a Path,
    pub stdout: File,
    pub stderr: File,
    pub extra_env: &'a std::collections::HashMap<String, String>,
    pub pipe_stdin: bool,
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
            stdout,
            stderr,
            extra_env,
            pipe_stdin,
        } = params;

        enforce_runner_policy(runner, command)?;

        let mut cmd = tokio::process::Command::new(&runner.shell);
        cmd.arg(&runner.shell_arg)
            .arg(command)
            .current_dir(cwd)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);

        if pipe_stdin {
            cmd.stdin(Stdio::piped());
        }

        #[cfg(unix)]
        {
            cmd.process_group(0); // child becomes its own process group leader
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
    extra_env: &std::collections::HashMap<String, String>,
    pipe_stdin: bool,
) -> Result<tokio::process::Child> {
    match runner.executor {
        RunnerExecutorKind::Shell => ShellRunnerExecutor.spawn(SpawnParams {
            runner,
            command,
            cwd,
            stdout,
            stderr,
            extra_env,
            pipe_stdin,
        }),
    }
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
