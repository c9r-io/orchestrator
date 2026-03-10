use crate::config::{ExecutionProfileMode, StepScope};
use crate::runner::{ResolvedExecutionProfile, SandboxResourceKind};
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

pub(super) async fn detect_sandbox_violation(
    execution_profile: &ResolvedExecutionProfile,
    wait_result: &WaitResult,
    stderr_path: &Path,
) -> SandboxViolationInfo {
    if execution_profile.mode != ExecutionProfileMode::Sandbox || wait_result.exit_code == 0 {
        return SandboxViolationInfo::default();
    }

    let stderr_tail = match read_output_with_limit(stderr_path, SANDBOX_STDERR_EXCERPT_MAX_BYTES)
        .await
    {
        Ok(output) => output.text,
        Err(err) => {
            debug!(path = %stderr_path.display(), error = %err, "sandbox denial detection skipped: failed to read stderr");
            return SandboxViolationInfo::default();
        }
    };
    let stderr_excerpt = sanitize_stderr_excerpt(&stderr_tail);
    let lower_stderr = stderr_tail.to_lowercase();

    if execution_profile.network_mode == crate::config::ExecutionNetworkMode::Allowlist
        && lower_stderr.contains("does not support network allowlists")
    {
        return SandboxViolationInfo {
            denied: true,
            event_type: Some("sandbox_network_blocked"),
            reason: Some("unsupported_backend_feature".to_string()),
            stderr_excerpt,
            resource_kind: None,
            network_target: None,
        };
    }

    if let Some(resource_kind) =
        detect_resource_exceeded(execution_profile, wait_result.exit_signal, &lower_stderr)
    {
        return SandboxViolationInfo {
            denied: true,
            event_type: Some("sandbox_resource_exceeded"),
            reason: Some(format!("{}_limit_exceeded", resource_kind.as_str())),
            stderr_excerpt,
            resource_kind: Some(resource_kind),
            network_target: None,
        };
    }

    if execution_profile.network_mode == crate::config::ExecutionNetworkMode::Deny
        && looks_like_network_denial(&lower_stderr)
    {
        return SandboxViolationInfo {
            denied: true,
            event_type: Some("sandbox_network_blocked"),
            reason: Some("network_blocked".to_string()),
            stderr_excerpt,
            resource_kind: None,
            network_target: detect_network_target(&stderr_tail),
        };
    }

    if !stderr_tail.contains("Operation not permitted") {
        return SandboxViolationInfo::default();
    }

    SandboxViolationInfo {
        denied: true,
        event_type: Some("sandbox_denied"),
        reason: Some("file_write_denied".to_string()),
        stderr_excerpt,
        resource_kind: None,
        network_target: None,
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

fn detect_resource_exceeded(
    execution_profile: &ResolvedExecutionProfile,
    exit_signal: Option<i32>,
    lower_stderr: &str,
) -> Option<SandboxResourceKind> {
    #[cfg(unix)]
    {
        if exit_signal == Some(libc::SIGXCPU) && execution_profile.max_cpu_seconds.is_some() {
            return Some(SandboxResourceKind::Cpu);
        }
    }
    if execution_profile.max_open_files.is_some() && lower_stderr.contains("too many open files") {
        return Some(SandboxResourceKind::OpenFiles);
    }
    if execution_profile.max_processes.is_some()
        && lower_stderr.contains("resource temporarily unavailable")
    {
        return Some(SandboxResourceKind::Processes);
    }
    if execution_profile.max_memory_mb.is_some()
        && (lower_stderr.contains("cannot allocate memory")
            || lower_stderr.contains("out of memory")
            || lower_stderr.contains("memory exhausted"))
    {
        return Some(SandboxResourceKind::Memory);
    }
    None
}

fn looks_like_network_denial(lower_stderr: &str) -> bool {
    lower_stderr.contains("connect")
        || lower_stderr.contains("network")
        || lower_stderr.contains("curl:")
        || lower_stderr.contains("wget:")
        || lower_stderr.contains("fetch")
        || lower_stderr.contains("connection")
        || lower_stderr.contains("socket")
        || lower_stderr.contains("resolve")
        || lower_stderr.contains("could not resolve host")
        || lower_stderr.contains("name or service not known")
        || lower_stderr.contains("temporary failure in name resolution")
        || lower_stderr.contains("nodename nor servname provided")
        || lower_stderr.contains("no route to host")
        || lower_stderr.contains("network is unreachable")
}

fn detect_network_target(stderr_tail: &str) -> Option<String> {
    for token in stderr_tail.split_whitespace() {
        let trimmed = token.trim_matches(|ch: char| "()[]{}<>\",'\"".contains(ch));
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Some(trimmed.to_string());
        }
    }
    if let Some(host) = extract_host_from_stderr(stderr_tail) {
        return Some(host);
    }
    for token in stderr_tail.split_whitespace() {
        let trimmed = token.trim_matches(|ch: char| "()[]{}<>\",'\"".contains(ch));
        if trimmed.contains(':')
            && !trimmed.starts_with('/')
            && !trimmed.ends_with(':')
            && trimmed.chars().any(|ch| ch.is_ascii_alphabetic())
            && (trimmed.contains('.') || has_numeric_port_suffix(trimmed))
            && trimmed != "curl:"
            && trimmed != "wget:"
        {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn has_numeric_port_suffix(value: &str) -> bool {
    let Some((host, port)) = value.rsplit_once(':') else {
        return false;
    };
    !host.is_empty() && !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit())
}

fn extract_host_from_stderr(stderr_tail: &str) -> Option<String> {
    for line in stderr_tail.lines() {
        let lower = line.to_lowercase();
        for marker in [
            "could not resolve host:",
            "failed to connect to",
            "connection to",
        ] {
            if let Some(idx) = lower.find(marker) {
                let value = line[idx + marker.len()..]
                    .trim()
                    .trim_matches(|ch: char| "()[]{}<>\",'\"".contains(ch))
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(|ch: char| ",.;".contains(ch));
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}
