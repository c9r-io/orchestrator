mod policy;
mod profile;
mod redact;
mod resource_limits;
mod sandbox;
#[cfg(target_os = "linux")]
mod sandbox_linux;
#[cfg(target_os = "macos")]
mod sandbox_macos;
mod spawn;

pub use policy::{DaemonPidGuardBlocked, enforce_runner_policy};
pub use profile::ResolvedExecutionProfile;
pub use redact::redact_text;
pub use sandbox::{
    SandboxBackendError, SandboxResourceKind, sandbox_backend_label,
    sandbox_backend_preflight_issues, validate_execution_profile_support,
};
pub use spawn::{
    CapturedChild, RunnerExecutor, RunnerStdioMode, ShellRunnerExecutor, SpawnParams,
    kill_child_process_group, spawn_with_runner, spawn_with_runner_and_capture,
};

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_config::config::{
        ExecutionNetworkMode, ExecutionProfileMode, RunnerConfig, RunnerExecutorKind, RunnerPolicy,
    };
    use std::fs::File;
    use std::io;
    use tempfile::tempdir;

    use sandbox::classify_sandbox_spawn_error;

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
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("cannot be empty")
        );
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
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("control characters")
        );
    }

    #[test]
    fn test_enforce_runner_policy_rejects_too_long_command() {
        let runner = make_runner_config();
        let long_command = "x".repeat(131_073);
        let result = enforce_runner_policy(&runner, &long_command);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("too long")
        );
    }

    #[test]
    fn test_enforce_runner_policy_rejects_disallowed_shell() {
        let mut runner = make_runner_config();
        runner.policy = RunnerPolicy::Allowlist;
        runner.shell = "/bin/sh".to_string();

        let result = enforce_runner_policy(&runner, "echo hello");
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("runner.shell")
        );
    }

    #[test]
    fn test_enforce_runner_policy_rejects_disallowed_shell_arg() {
        let mut runner = make_runner_config();
        runner.policy = RunnerPolicy::Allowlist;
        runner.shell_arg = "-c".to_string();

        let result = enforce_runner_policy(&runner, "echo hello");
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("runner.shell_arg")
        );
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

    #[test]
    fn test_sandbox_backend_preflight_issues_reports_macos_allowlist_gap() {
        let mut profile = ResolvedExecutionProfile::host();
        profile.mode = ExecutionProfileMode::Sandbox;
        profile.network_mode = ExecutionNetworkMode::Allowlist;
        profile.network_allowlist = vec!["example.com:443".to_string()];

        let issues = sandbox_backend_preflight_issues(&profile);
        #[cfg(target_os = "macos")]
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("does not support network_mode=allowlist"))
        );
        #[cfg(not(target_os = "macos"))]
        assert!(!issues.is_empty());
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

        // SAFETY: test runs single-threaded; no concurrent env reads.
        unsafe {
            std::env::set_var("RUNNER_ALLOWED_TEST", "visible");
            std::env::set_var("RUNNER_BLOCKED_TEST", "hidden");
            std::env::set_var("CLAUDECODE", "nested-session");
        }

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
        // SAFETY: test runs single-threaded; no concurrent env reads.
        unsafe {
            std::env::remove_var("RUNNER_ALLOWED_TEST");
            std::env::remove_var("RUNNER_BLOCKED_TEST");
            std::env::remove_var("CLAUDECODE");
        }

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

    #[test]
    fn test_sandbox_backend_label_for_current_platform() {
        let profile = ResolvedExecutionProfile::host();
        let label = sandbox_backend_label(&profile);
        assert_eq!(
            label, "host",
            "host profile should always return 'host' backend label"
        );
    }
}
