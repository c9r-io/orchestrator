#[cfg(test)]
mod cases {
    use crate::config::StepScope;
    use crate::config::{ExecutionFsMode, ExecutionNetworkMode, ExecutionProfileMode};
    use crate::runner::ResolvedExecutionProfile;
    use crate::scheduler::phase_runner::types::*;
    use crate::scheduler::phase_runner::util::*;

    #[test]
    fn shell_escape_simple_string() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn shell_escape_string_with_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_escape_empty_string() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn shell_escape_special_chars_preserved() {
        assert_eq!(shell_escape("$HOME"), "'$HOME'");
        assert_eq!(shell_escape("a b c"), "'a b c'");
        assert_eq!(shell_escape("a`b"), "'a`b'");
    }

    #[test]
    fn resolved_step_timeout_defaults() {
        assert_eq!(resolved_step_timeout_secs(None), DEFAULT_STEP_TIMEOUT_SECS);
        assert_eq!(resolved_step_timeout_secs(Some(60)), 60);
        assert_eq!(resolved_step_timeout_secs(Some(0)), 0);
    }

    #[test]
    fn effective_exit_code_preserves_nonzero_codes() {
        assert_eq!(effective_exit_code(7, "passed"), 7);
        assert_eq!(effective_exit_code(7, "failed"), 7);
    }

    #[test]
    fn effective_exit_code_maps_validation_failure_to_nonzero() {
        assert_eq!(
            effective_exit_code(0, "failed"),
            VALIDATION_FAILED_EXIT_CODE
        );
        assert_eq!(effective_exit_code(0, "passed"), 0);
    }

    #[test]
    fn heartbeat_sample_active_when_output_grows() {
        let mut progress = HeartbeatProgress::default();
        let sample = sample_heartbeat_progress(&mut progress, 256, 0, 30, true);

        assert_eq!(sample.stdout_delta_bytes, 256);
        assert_eq!(sample.stderr_delta_bytes, 0);
        assert_eq!(sample.stagnant_heartbeats, 0);
        assert_eq!(sample.output_state, "active");
    }

    #[test]
    fn heartbeat_sample_quiet_before_threshold() {
        let mut progress = HeartbeatProgress::default();

        let first = sample_heartbeat_progress(&mut progress, 0, 0, 30, true);
        let second = sample_heartbeat_progress(&mut progress, 0, 0, 60, true);

        assert_eq!(first.output_state, "quiet");
        assert_eq!(second.output_state, "quiet");
        assert_eq!(second.stagnant_heartbeats, 2);
    }

    #[test]
    fn heartbeat_sample_low_output_after_three_quiet_heartbeats() {
        let mut progress = HeartbeatProgress::default();

        let _ = sample_heartbeat_progress(&mut progress, 0, 0, 30, true);
        let _ = sample_heartbeat_progress(&mut progress, 0, 0, 60, true);
        let third = sample_heartbeat_progress(&mut progress, 0, 0, 90, true);

        assert_eq!(third.stagnant_heartbeats, 3);
        assert_eq!(third.output_state, "low_output");
    }

    #[test]
    fn heartbeat_sample_resets_quiet_counter_after_output_resumes() {
        let mut progress = HeartbeatProgress::default();

        let _ = sample_heartbeat_progress(&mut progress, 0, 0, 30, true);
        let _ = sample_heartbeat_progress(&mut progress, 0, 0, 60, true);
        let resumed = sample_heartbeat_progress(
            &mut progress,
            LOW_OUTPUT_DELTA_THRESHOLD_BYTES + 64,
            0,
            90,
            true,
        );

        assert_eq!(resumed.stagnant_heartbeats, 0);
        assert_eq!(resumed.output_state, "active");
    }

    #[test]
    fn heartbeat_sample_marks_quiet_when_process_is_not_alive() {
        let mut progress = HeartbeatProgress::default();
        let sample = sample_heartbeat_progress(&mut progress, 0, 0, 120, false);

        assert_eq!(sample.output_state, "quiet");
        assert_eq!(sample.stagnant_heartbeats, 1);
    }

    #[test]
    fn step_scope_label_matches_both_variants() {
        assert_eq!(step_scope_label(StepScope::Task), "task");
        assert_eq!(step_scope_label(StepScope::Item), "item");
    }

    #[tokio::test]
    async fn read_output_with_limit_returns_only_tail_bytes() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("phase_runner_tail.log");
        std::fs::write(&path, "0123456789abcdef").expect("write log file");

        let limited = read_output_with_limit(&path, 6)
            .await
            .expect("read limited output");

        assert_eq!(limited.text, "abcdef");
        assert_eq!(limited.truncated_prefix_bytes, 10);
    }

    #[tokio::test]
    async fn read_output_with_limit_no_truncation_when_file_smaller_than_limit() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("small.log");
        std::fs::write(&path, "short").expect("write log file");

        let limited = read_output_with_limit(&path, 1024)
            .await
            .expect("read limited output");

        assert_eq!(limited.text, "short");
        assert_eq!(limited.truncated_prefix_bytes, 0);
    }

    #[tokio::test]
    async fn read_output_with_limit_empty_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("empty.log");
        std::fs::write(&path, "").expect("write log file");

        let limited = read_output_with_limit(&path, 1024)
            .await
            .expect("read limited output");

        assert_eq!(limited.text, "");
        assert_eq!(limited.truncated_prefix_bytes, 0);
    }

    #[tokio::test]
    async fn read_output_with_limit_exact_size_match() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("exact.log");
        std::fs::write(&path, "12345").expect("write log file");

        let limited = read_output_with_limit(&path, 5)
            .await
            .expect("read limited output");

        assert_eq!(limited.text, "12345");
        assert_eq!(limited.truncated_prefix_bytes, 0);
    }

    #[tokio::test]
    async fn read_output_with_limit_missing_file_returns_error() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("nonexistent.log");

        let result = read_output_with_limit(&path, 1024).await;
        assert!(result.is_err());
    }

    fn sandbox_profile() -> ResolvedExecutionProfile {
        ResolvedExecutionProfile {
            name: "sandbox_profile".to_string(),
            mode: ExecutionProfileMode::Sandbox,
            fs_mode: ExecutionFsMode::WorkspaceRwScoped,
            writable_paths: Vec::new(),
            network_mode: ExecutionNetworkMode::Deny,
            network_allowlist: Vec::new(),
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        }
    }

    #[tokio::test]
    async fn detect_sandbox_denial_returns_false_for_host_mode() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("stderr.log");
        std::fs::write(&path, "Operation not permitted").expect("write stderr");

        let info = detect_sandbox_denial(&ResolvedExecutionProfile::host(), 1, &path).await;

        assert!(!info.denied);
        assert!(info.reason.is_none());
    }

    #[tokio::test]
    async fn detect_sandbox_denial_detects_operation_not_permitted() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("stderr.log");
        std::fs::write(&path, "/bin/bash: sandbox-denied.txt: Operation not permitted\n")
            .expect("write stderr");

        let info = detect_sandbox_denial(&sandbox_profile(), 1, &path).await;

        assert!(info.denied);
        assert_eq!(info.reason.as_deref(), Some("file_write_denied"));
        assert_eq!(
            info.stderr_excerpt.as_deref(),
            Some("/bin/bash: sandbox-denied.txt: Operation not permitted")
        );
    }

    #[tokio::test]
    async fn detect_sandbox_denial_ignores_other_stderr() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("stderr.log");
        std::fs::write(&path, "syntax error near unexpected token").expect("write stderr");

        let info = detect_sandbox_denial(&sandbox_profile(), 2, &path).await;

        assert!(!info.denied);
        assert!(info.reason.is_none());
    }

    #[tokio::test]
    async fn detect_sandbox_denial_handles_missing_stderr() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("missing.log");

        let info = detect_sandbox_denial(&sandbox_profile(), 1, &path).await;

        assert!(!info.denied);
        assert!(info.reason.is_none());
    }

    #[test]
    fn heartbeat_sample_delta_exactly_at_threshold_counts_as_stagnant() {
        let mut progress = HeartbeatProgress::default();
        // First sample with exactly threshold bytes
        let s1 =
            sample_heartbeat_progress(&mut progress, LOW_OUTPUT_DELTA_THRESHOLD_BYTES, 0, 30, true);
        assert_eq!(s1.stagnant_heartbeats, 1); // exactly at threshold counts as stagnant

        // Second sample with no additional output (delta = 0)
        let s2 =
            sample_heartbeat_progress(&mut progress, LOW_OUTPUT_DELTA_THRESHOLD_BYTES, 0, 60, true);
        assert_eq!(s2.stagnant_heartbeats, 2);
        assert_eq!(s2.stdout_delta_bytes, 0);
    }

    #[test]
    fn heartbeat_sample_not_alive_overrides_low_output_detection() {
        let mut progress = HeartbeatProgress::default();
        // Accumulate 3 stagnant heartbeats
        let _ = sample_heartbeat_progress(&mut progress, 0, 0, 30, true);
        let _ = sample_heartbeat_progress(&mut progress, 0, 0, 60, true);
        let _ = sample_heartbeat_progress(&mut progress, 0, 0, 90, true);
        // Now process is dead - should be "quiet" not "low_output"
        let sample = sample_heartbeat_progress(&mut progress, 0, 0, 120, false);
        assert_eq!(sample.output_state, "quiet");
        assert_eq!(sample.stagnant_heartbeats, 4);
    }

    #[test]
    fn heartbeat_sample_tracks_stderr_delta() {
        let mut progress = HeartbeatProgress::default();
        let _ = sample_heartbeat_progress(&mut progress, 0, 100, 30, true);
        let sample = sample_heartbeat_progress(&mut progress, 0, 300, 60, true);
        assert_eq!(sample.stderr_delta_bytes, 200);
        assert_eq!(sample.stdout_delta_bytes, 0);
        assert_eq!(sample.output_state, "active");
    }

    #[test]
    fn effective_exit_code_with_various_validation_statuses() {
        // Non-standard validation statuses
        assert_eq!(effective_exit_code(0, "running"), 0);
        assert_eq!(effective_exit_code(0, "skipped"), 0);
        assert_eq!(effective_exit_code(0, ""), 0);
        // Only "failed" triggers override
        assert_eq!(effective_exit_code(0, "Failed"), 0); // case-sensitive
    }

    #[test]
    fn shell_escape_multiple_single_quotes() {
        assert_eq!(shell_escape("it's Bob's"), "'it'\\''s Bob'\\''s'");
    }

    #[test]
    fn shell_escape_only_single_quote() {
        assert_eq!(shell_escape("'"), "''\\'''");
    }
}
