use crate::config::StepScope;
use crate::events::insert_event;
use crate::output_capture::OutputCaptureHandles;
use crate::runner::kill_child_process_group;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::sync::Arc;
use tokio::time::Instant;

use super::types::HEARTBEAT_INTERVAL_SECS;
use super::types::{HeartbeatProgress, WaitResult};
use super::util::{resolved_step_timeout_secs, sample_heartbeat_progress, step_scope_label};
use super::RunningTask;

/// Stage 3: Polling loop with heartbeat sampling, pause detection, timeout handling.
#[allow(clippy::too_many_arguments)]
pub(super) async fn wait_for_process(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    step_scope: StepScope,
    step_timeout_secs: Option<u64>,
    runtime: &RunningTask,
    child_pid: Option<u32>,
    output_capture: Option<OutputCaptureHandles>,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<WaitResult> {
    let step_timeout_secs = resolved_step_timeout_secs(step_timeout_secs);
    let start = Instant::now();
    let deadline = start + std::time::Duration::from_secs(step_timeout_secs);
    let heartbeat_interval = std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
    let mut timed_out = false;
    let mut heartbeat_progress = HeartbeatProgress::default();

    let (exit_code, exit_signal): (i32, Option<i32>) = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let mut child_lock = runtime.child.lock().await;
            if let Some(ref mut child) = *child_lock {
                kill_child_process_group(child).await;
            }
            timed_out = true;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_timeout",
                json!({
                    "step": phase,
                    "step_id": step_id,
                    "step_scope": step_scope_label(step_scope),
                    "timeout_secs": step_timeout_secs,
                    "pid": child_pid,
                }),
            )
            .await?;
            break (-4, None);
        }

        let wait_duration = heartbeat_interval.min(remaining);
        let wait_result = {
            let mut child_lock = runtime.child.lock().await;
            if let Some(ref mut child) = *child_lock {
                tokio::time::timeout(wait_duration, child.wait()).await
            } else {
                break (-3, None);
            }
        };

        match wait_result {
            Ok(Ok(status)) => {
                #[cfg(unix)]
                {
                    break (status.code().unwrap_or(-1), status.signal());
                }
                #[cfg(not(unix))]
                {
                    break (status.code().unwrap_or(-1), None);
                }
            }
            Ok(Err(e)) => {
                break (
                    if e.kind() == std::io::ErrorKind::NotFound {
                        -2
                    } else {
                        -3
                    },
                    None,
                );
            }
            Err(_) => {
                let elapsed = start.elapsed();
                let stdout_bytes = tokio::fs::metadata(stdout_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                let stderr_bytes = tokio::fs::metadata(stderr_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                let pid_alive = child_pid
                    .map(|pid| {
                        std::process::Command::new("kill")
                            .args(["-0", &pid.to_string()])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                let heartbeat = sample_heartbeat_progress(
                    &mut heartbeat_progress,
                    stdout_bytes,
                    stderr_bytes,
                    elapsed.as_secs(),
                    pid_alive,
                );

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_heartbeat",
                    json!({
                        "step": phase,
                        "step_id": step_id,
                        "step_scope": step_scope_label(step_scope),
                        "elapsed_secs": elapsed.as_secs(),
                        "stdout_bytes": heartbeat.stdout_bytes,
                        "stderr_bytes": heartbeat.stderr_bytes,
                        "stdout_delta_bytes": heartbeat.stdout_delta_bytes,
                        "stderr_delta_bytes": heartbeat.stderr_delta_bytes,
                        "stagnant_heartbeats": heartbeat.stagnant_heartbeats,
                        "output_state": heartbeat.output_state,
                        "pid": child_pid,
                        "pid_alive": pid_alive,
                    }),
                )
                .await?;

                // Stall auto-kill: if low_output persists for too many consecutive
                // heartbeats, automatically kill the step to prevent pipeline stalls.
                if heartbeat.output_state == "low_output"
                    && heartbeat.stagnant_heartbeats
                        >= super::types::STALL_AUTO_KILL_CONSECUTIVE_HEARTBEATS
                {
                    let mut child_lock = runtime.child.lock().await;
                    if let Some(ref mut child) = *child_lock {
                        kill_child_process_group(child).await;
                    }
                    insert_event(
                        state,
                        task_id,
                        Some(item_id),
                        "step_stall_killed",
                        json!({
                            "step": phase,
                            "step_id": step_id,
                            "step_scope": step_scope_label(step_scope),
                            "elapsed_secs": elapsed.as_secs(),
                            "stagnant_heartbeats": heartbeat.stagnant_heartbeats,
                            "pid": child_pid,
                        }),
                    )
                    .await?;
                    break (-7, None);
                }

                // Cross-process pause: check if another process (e.g. `task pause`)
                // has marked this task as paused in the DB.
                if super::super::task_state::is_task_paused_in_db(state, task_id).await? {
                    let mut child_lock = runtime.child.lock().await;
                    if let Some(ref mut child) = *child_lock {
                        kill_child_process_group(child).await;
                    }
                    break (-5, None); // externally paused
                }
            }
        }
    };

    let duration = start.elapsed();
    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = None;
    }
    if let Some(capture) = output_capture {
        capture.wait().await?;
    }

    Ok(WaitResult {
        exit_code,
        exit_signal,
        timed_out,
        duration,
    })
}
