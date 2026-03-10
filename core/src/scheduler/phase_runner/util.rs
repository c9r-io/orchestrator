use crate::config::{ExecutionProfileMode, StepScope};
use crate::runner::ResolvedExecutionProfile;
use anyhow::{Context, Result};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::debug;

use super::types::*;

pub(super) fn step_scope_label(scope: StepScope) -> &'static str {
    match scope {
        StepScope::Task => "task",
        StepScope::Item => "item",
    }
}

pub(crate) fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub(super) fn resolved_step_timeout_secs(step_timeout_secs: Option<u64>) -> u64 {
    step_timeout_secs.unwrap_or(DEFAULT_STEP_TIMEOUT_SECS)
}

pub(super) fn effective_exit_code(exit_code: i64, validation_status: &str) -> i64 {
    if exit_code == 0 && validation_status == "failed" {
        VALIDATION_FAILED_EXIT_CODE
    } else {
        exit_code
    }
}

pub(super) fn sample_heartbeat_progress(
    progress: &mut HeartbeatProgress,
    stdout_bytes: u64,
    stderr_bytes: u64,
    elapsed_secs: u64,
    pid_alive: bool,
) -> HeartbeatSample {
    let stdout_delta_bytes = stdout_bytes.saturating_sub(progress.last_stdout_bytes);
    let stderr_delta_bytes = stderr_bytes.saturating_sub(progress.last_stderr_bytes);
    let total_delta = stdout_delta_bytes + stderr_delta_bytes;

    if total_delta <= LOW_OUTPUT_DELTA_THRESHOLD_BYTES {
        progress.stagnant_heartbeats += 1;
    } else {
        progress.stagnant_heartbeats = 0;
    }

    let output_state = if !pid_alive {
        "quiet"
    } else if elapsed_secs >= LOW_OUTPUT_MIN_ELAPSED_SECS
        && progress.stagnant_heartbeats >= LOW_OUTPUT_CONSECUTIVE_HEARTBEATS
        && total_delta <= LOW_OUTPUT_DELTA_THRESHOLD_BYTES
    {
        "low_output"
    } else if total_delta <= LOW_OUTPUT_DELTA_THRESHOLD_BYTES {
        "quiet"
    } else {
        "active"
    };

    progress.last_stdout_bytes = stdout_bytes;
    progress.last_stderr_bytes = stderr_bytes;

    HeartbeatSample {
        stdout_bytes,
        stderr_bytes,
        stdout_delta_bytes,
        stderr_delta_bytes,
        stagnant_heartbeats: progress.stagnant_heartbeats,
        output_state,
    }
}

pub(super) async fn read_output_with_limit(path: &Path, max_bytes: u64) -> Result<LimitedOutput> {
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("failed to open output log: {}", path.display()))?;
    let file_len = file
        .metadata()
        .await
        .with_context(|| format!("failed to stat output log: {}", path.display()))?
        .len();

    let start = file_len.saturating_sub(max_bytes);
    if start > 0 {
        file.seek(tokio::io::SeekFrom::Start(start))
            .await
            .with_context(|| format!("failed to seek output log: {}", path.display()))?;
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .await
        .with_context(|| format!("failed to read output log: {}", path.display()))?;
    Ok(LimitedOutput {
        text: String::from_utf8_lossy(&buf).into_owned(),
        truncated_prefix_bytes: start,
    })
}

pub(super) async fn detect_sandbox_denial(
    execution_profile: &ResolvedExecutionProfile,
    exit_code: i32,
    stderr_path: &Path,
) -> SandboxDenialInfo {
    if execution_profile.mode != ExecutionProfileMode::Sandbox || exit_code == 0 {
        return SandboxDenialInfo::default();
    }

    let stderr_tail = match read_output_with_limit(stderr_path, SANDBOX_STDERR_EXCERPT_MAX_BYTES).await
    {
        Ok(output) => output.text,
        Err(err) => {
            debug!(path = %stderr_path.display(), error = %err, "sandbox denial detection skipped: failed to read stderr");
            return SandboxDenialInfo::default();
        }
    };

    if !stderr_tail.contains("Operation not permitted") {
        return SandboxDenialInfo::default();
    }

    SandboxDenialInfo {
        denied: true,
        reason: Some("file_write_denied".to_string()),
        stderr_excerpt: sanitize_stderr_excerpt(&stderr_tail),
    }
}

fn sanitize_stderr_excerpt(stderr_tail: &str) -> Option<String> {
    let excerpt = stderr_tail
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(stderr_tail)
        .trim();
    if excerpt.is_empty() {
        None
    } else {
        Some(excerpt.to_string())
    }
}
