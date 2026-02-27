use crate::config::PipelineVariables;
use crate::config_load::now_ts;
use crate::events::insert_event;
use crate::health::{
    increment_consecutive_errors, mark_agent_diseased, reset_consecutive_errors,
    update_capability_health,
};
use crate::metrics::MetricsCollector;
use crate::output_validation::validate_phase_output;
use crate::runner::{redact_text, spawn_with_runner};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::session_store;
use crate::state::InnerState;
use crate::task_repository::NewCommandRun;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::time::Instant;
use uuid::Uuid;

use super::RunningTask;

const DEFAULT_STEP_TIMEOUT_SECS: u64 = 1800;
const HEARTBEAT_INTERVAL_SECS: u64 = 30;

struct LimitedOutput {
    text: String,
    truncated_prefix_bytes: u64,
}

pub struct PhaseRunRequest<'a> {
    pub task_id: &'a str,
    pub item_id: &'a str,
    pub step_id: &'a str,
    pub phase: &'a str,
    pub tty: bool,
    pub command: String,
    pub workspace_root: &'a Path,
    pub workspace_id: &'a str,
    pub agent_id: &'a str,
    pub runtime: &'a RunningTask,
    pub step_timeout_secs: Option<u64>,
}

pub struct RotatingPhaseRunRequest<'a> {
    pub task_id: &'a str,
    pub item_id: &'a str,
    pub step_id: &'a str,
    pub phase: &'a str,
    pub tty: bool,
    pub capability: Option<&'a str>,
    pub rel_path: &'a str,
    pub ticket_paths: &'a [String],
    pub workspace_root: &'a Path,
    pub workspace_id: &'a str,
    pub cycle: u32,
    pub runtime: &'a RunningTask,
    pub pipeline_vars: Option<&'a PipelineVariables>,
    pub step_timeout_secs: Option<u64>,
}

pub(crate) fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn resolved_step_timeout_secs(step_timeout_secs: Option<u64>) -> u64 {
    step_timeout_secs.unwrap_or(DEFAULT_STEP_TIMEOUT_SECS)
}

async fn read_output_with_limit(path: &Path, max_bytes: u64) -> Result<LimitedOutput> {
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

async fn run_phase_with_timeout(
    state: &Arc<InnerState>,
    request: PhaseRunRequest<'_>,
) -> Result<crate::dto::RunResult> {
    let PhaseRunRequest {
        task_id,
        item_id,
        step_id,
        phase,
        tty,
        command,
        workspace_root,
        workspace_id,
        agent_id,
        runtime,
        step_timeout_secs,
    } = request;
    let now = now_ts();
    let run_uuid = Uuid::new_v4();
    let run_id = run_uuid.to_string();
    let logs_dir = state.logs_dir.join(task_id);
    let stdout_path = logs_dir.join(format!("{}_{}.stdout", phase, run_id));
    let stderr_path = logs_dir.join(format!("{}_{}.stderr", phase, run_id));

    let runner = {
        let active = crate::config_load::read_active_config(state)?;
        active.config.runner.clone()
    };
    let redaction_patterns = runner.redaction_patterns.clone();
    if !logs_dir.starts_with(&state.logs_dir) {
        return Err(anyhow::anyhow!(
            "logs dir escapes managed root: {}",
            logs_dir.display()
        ));
    }

    let logs_dir_for_create = logs_dir.clone();
    let stdout_path_for_create = stdout_path.clone();
    let stderr_path_for_create = stderr_path.clone();
    let (stdout_file, stderr_file) = tokio::task::spawn_blocking(move || -> Result<_> {
        std::fs::create_dir_all(&logs_dir_for_create).with_context(|| {
            format!(
                "failed to create logs dir: {}",
                logs_dir_for_create.display()
            )
        })?;
        let stdout_file = std::fs::File::create(&stdout_path_for_create).with_context(|| {
            format!(
                "failed to create stdout log: {}",
                stdout_path_for_create.display()
            )
        })?;
        let stderr_file = std::fs::File::create(&stderr_path_for_create).with_context(|| {
            format!(
                "failed to create stderr log: {}",
                stderr_path_for_create.display()
            )
        })?;
        Ok((stdout_file, stderr_file))
    })
    .await
    .context("log file setup worker failed")??;

    let mut session_id: Option<String> = None;
    let command_to_run = if tty {
        let sid = Uuid::new_v4().to_string();
        let session_dir = state.logs_dir.join("sessions").join(&sid);
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("failed to create session dir: {}", session_dir.display()))?;
        let input_fifo = session_dir.join("input.fifo");
        let transcript_path = session_dir.join("transcript.log");
        let output_json_path = session_dir.join("output.json");
        if !input_fifo.exists() {
            let status = std::process::Command::new("mkfifo")
                .arg(&input_fifo)
                .status()
                .with_context(|| format!("failed to spawn mkfifo for {}", input_fifo.display()))?;
            if !status.success() {
                anyhow::bail!("mkfifo failed for {}", input_fifo.display());
            }
        }
        let inner = format!(
            "ORCH_OUTPUT_JSON_PATH={} ORCH_SESSION_ID={} ORCH_STEP_ID={} {}",
            shell_escape(&output_json_path.to_string_lossy()),
            shell_escape(&sid),
            shell_escape(step_id),
            command
        );
        let wrapped = format!(
            "{} < {}",
            inner,
            shell_escape(&input_fifo.to_string_lossy())
        );
        session_id = Some(sid.clone());
        session_store::insert_session(
            &state.db_path,
            &session_store::NewSession {
                id: &sid,
                task_id,
                task_item_id: Some(item_id),
                step_id,
                phase,
                agent_id,
                state: "active",
                pid: 0,
                pty_backend: "script",
                cwd: &workspace_root.to_string_lossy(),
                command: &command,
                input_fifo_path: &input_fifo.to_string_lossy(),
                stdout_path: &stdout_path.to_string_lossy(),
                stderr_path: &stderr_path.to_string_lossy(),
                transcript_path: &transcript_path.to_string_lossy(),
                output_json_path: Some(&output_json_path.to_string_lossy()),
            },
        )?;
        wrapped
    } else {
        command.clone()
    };
    let child = spawn_with_runner(
        &runner,
        &command_to_run,
        workspace_root,
        stdout_file,
        stderr_file,
    )?;
    if let Some(sid) = session_id.as_deref() {
        if let Some(pid) = child.id() {
            let _ = session_store::update_session_pid(&state.db_path, sid, pid as i64);
        }
    }

    if tty && session_id.is_some() {
        std::mem::forget(child);
        return Ok(crate::dto::RunResult {
            success: true,
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            timed_out: false,
            duration_ms: Some(0),
            output: None,
            validation_status: "passed".to_string(),
            agent_id: agent_id.to_string(),
            run_id: run_id.clone(),
        });
    }

    let child_pid = child.id();
    let preview: String = command.chars().take(120).collect();
    insert_event(
        state,
        task_id,
        Some(item_id),
        "step_spawned",
        json!({
            "step": phase,
            "step_id": step_id,
            "agent_id": agent_id,
            "run_id": run_id,
            "pid": child_pid,
            "command_preview": preview,
        }),
    )?;

    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = Some(child);
    }

    let step_timeout_secs = resolved_step_timeout_secs(step_timeout_secs);
    let start = Instant::now();
    let deadline = start + std::time::Duration::from_secs(step_timeout_secs);
    let heartbeat_interval = std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
    let mut timed_out = false;

    let exit_code: i32 = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let mut child_lock = runtime.child.lock().await;
            if let Some(ref mut child) = *child_lock {
                let _ = child.kill().await;
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
                    "timeout_secs": step_timeout_secs,
                    "pid": child_pid,
                }),
            )?;
            break -4;
        }

        let wait_duration = heartbeat_interval.min(remaining);
        let wait_result = {
            let mut child_lock = runtime.child.lock().await;
            if let Some(ref mut child) = *child_lock {
                tokio::time::timeout(wait_duration, child.wait()).await
            } else {
                break -3;
            }
        };

        match wait_result {
            Ok(Ok(status)) => break status.code().unwrap_or(-1),
            Ok(Err(e)) => {
                break if e.kind() == std::io::ErrorKind::NotFound {
                    -2
                } else {
                    -3
                };
            }
            Err(_) => {
                let elapsed = start.elapsed();
                let stdout_bytes = tokio::fs::metadata(&stdout_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                let stderr_bytes = tokio::fs::metadata(&stderr_path)
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

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_heartbeat",
                    json!({
                        "step": phase,
                        "step_id": step_id,
                        "elapsed_secs": elapsed.as_secs(),
                        "stdout_bytes": stdout_bytes,
                        "stderr_bytes": stderr_bytes,
                        "pid": child_pid,
                        "pid_alive": pid_alive,
                    }),
                )?;
            }
        }
    };

    let duration = start.elapsed();
    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = None;
    }

    const MAX_PHASE_OUTPUT_BYTES: u64 = 256 * 1024;
    let stdout_output = read_output_with_limit(&stdout_path, MAX_PHASE_OUTPUT_BYTES)
        .await
        .with_context(|| format!("failed to read stdout log: {}", stdout_path.display()))?;
    let stderr_output = read_output_with_limit(&stderr_path, MAX_PHASE_OUTPUT_BYTES)
        .await
        .with_context(|| format!("failed to read stderr log: {}", stderr_path.display()))?;
    let stdout_content = stdout_output.text;
    let stderr_content = stderr_output.text;

    let validation = validate_phase_output(
        phase,
        run_uuid,
        agent_id,
        exit_code as i64,
        &stdout_content,
        &stderr_content,
    )?;
    let mut success = exit_code == 0;
    let mut validation_event_payload_json: Option<String> = None;
    if validation.status == "failed" {
        success = false;
        validation_event_payload_json = Some(serde_json::to_string(&json!({
            "phase": phase,
            "run_id": run_id.clone(),
            "error": validation.error.clone(),
            "stdout_truncated_prefix_bytes": stdout_output.truncated_prefix_bytes,
            "stderr_truncated_prefix_bytes": stderr_output.truncated_prefix_bytes
        }))?);
    }

    let mut redacted_output = validation.output.clone();
    redacted_output.stdout = redact_text(&redacted_output.stdout, &redaction_patterns);
    redacted_output.stderr = redact_text(&redacted_output.stderr, &redaction_patterns);

    let writer = state.db_writer.clone();
    let task_id_owned = task_id.to_string();
    let item_id_owned = item_id.to_string();
    let insert_payload = NewCommandRun {
        id: run_id.clone(),
        task_item_id: item_id.to_string(),
        phase: phase.to_string(),
        command: command.clone(),
        cwd: workspace_root.to_string_lossy().to_string(),
        workspace_id: workspace_id.to_string(),
        agent_id: agent_id.to_string(),
        exit_code: exit_code as i64,
        stdout_path: stdout_path.to_string_lossy().to_string(),
        stderr_path: stderr_path.to_string_lossy().to_string(),
        started_at: now,
        ended_at: now_ts(),
        interrupted: 0,
        output_json: serde_json::to_string(&redacted_output)?,
        artifacts_json: serde_json::to_string(&redacted_output.artifacts)?,
        confidence: Some(redacted_output.confidence),
        quality_score: Some(redacted_output.quality_score),
        validation_status: validation.status.to_string(),
        session_id: session_id.clone(),
        machine_output_source: if tty {
            "output_json_path".to_string()
        } else {
            "stdout".to_string()
        },
        output_json_path: session_id
            .as_ref()
            .map(|sid| {
                state
                    .logs_dir
                    .join("sessions")
                    .join(sid)
                    .join("output.json")
            })
            .map(|p| p.to_string_lossy().to_string()),
    };
    let sender = crate::collab::AgentEndpoint::for_task_item(agent_id, task_id, item_id);
    let msg = crate::collab::AgentMessage::publish(
        sender,
        crate::collab::MessagePayload::ExecutionResult(crate::collab::ExecutionResult {
            run_id: run_uuid,
            output: redacted_output.clone(),
            success,
            error: validation.error.clone(),
        }),
    );
    let (publish_event_type, publish_event_payload_json) = if let Err(err) =
        state.message_bus.publish(msg).await
    {
        (
            "bus_publish_failed",
            serde_json::to_string(&json!({"phase":phase,"run_id":run_id,"error":err.to_string()}))?,
        )
    } else {
        (
            "phase_output_published",
            serde_json::to_string(&json!({"phase":phase,"run_id":run_id}))?,
        )
    };

    tokio::task::spawn_blocking(move || {
        let mut events = Vec::with_capacity(2);
        if let Some(payload_json) = validation_event_payload_json.as_deref() {
            events.push(crate::db_write::DbEventRecord {
                task_id: task_id_owned.as_str(),
                task_item_id: Some(item_id_owned.as_str()),
                event_type: "output_validation_failed",
                payload_json,
            });
        }
        events.push(crate::db_write::DbEventRecord {
            task_id: task_id_owned.as_str(),
            task_item_id: Some(item_id_owned.as_str()),
            event_type: publish_event_type,
            payload_json: publish_event_payload_json.as_str(),
        });
        writer.persist_phase_result_with_events(&insert_payload, &events)
    })
    .await
    .context("command run insert worker failed")??;

    update_capability_health(state, agent_id, Some(phase), success);

    let duration_ms = duration.as_millis() as u64;
    {
        let mut metrics_map = state.agent_metrics.write().unwrap();
        let metrics = metrics_map
            .entry(agent_id.to_string())
            .or_insert_with(MetricsCollector::new_agent_metrics);
        if success {
            MetricsCollector::record_success(metrics, duration_ms);
        } else {
            MetricsCollector::record_failure(metrics);
        }
        MetricsCollector::decrement_load(metrics);
    }

    if !success {
        let errors = increment_consecutive_errors(state, agent_id);
        if errors >= 2 {
            mark_agent_diseased(state, agent_id);
        }
    } else {
        reset_consecutive_errors(state, agent_id);
    }
    if let Some(sid) = session_id.as_deref() {
        let _ = session_store::update_session_state(
            &state.db_path,
            sid,
            "closed",
            Some(exit_code as i64),
            true,
        );
    }

    Ok(crate::dto::RunResult {
        success,
        exit_code: exit_code as i64,
        stdout_path: stdout_path.to_string_lossy().to_string(),
        stderr_path: stderr_path.to_string_lossy().to_string(),
        timed_out,
        duration_ms: Some(duration_ms),
        output: Some(redacted_output),
        validation_status: validation.status.to_string(),
        agent_id: agent_id.to_string(),
        run_id,
    })
}

pub async fn run_phase(
    state: &Arc<InnerState>,
    request: PhaseRunRequest<'_>,
) -> Result<crate::dto::RunResult> {
    run_phase_with_timeout(state, request).await
}

pub async fn run_phase_with_rotation(
    state: &Arc<InnerState>,
    request: RotatingPhaseRunRequest<'_>,
) -> Result<crate::dto::RunResult> {
    let RotatingPhaseRunRequest {
        task_id,
        item_id,
        step_id,
        phase,
        tty,
        capability,
        rel_path,
        ticket_paths,
        workspace_root,
        workspace_id,
        cycle,
        runtime,
        pipeline_vars,
        step_timeout_secs,
    } = request;
    let effective_capability = capability.or(match phase {
        "qa" | "fix" | "retest" => Some(phase),
        _ => None,
    });

    let (agent_id, template) = {
        let active = crate::config_load::read_active_config(state)?;
        let agents = active.config.agents.clone();

        if let Some(cap) = effective_capability {
            let health_map = state.agent_health.read().unwrap();
            let metrics_map = state.agent_metrics.read().unwrap();
            select_agent_advanced(cap, &agents, &health_map, &metrics_map, &HashSet::new())?
        } else {
            select_agent_by_preference(&agents)?
        }
    };

    {
        let mut metrics_map = state.agent_metrics.write().unwrap();
        let metrics = metrics_map
            .entry(agent_id.clone())
            .or_insert_with(MetricsCollector::new_agent_metrics);
        MetricsCollector::increment_load(metrics);
    }

    let escaped_paths: Vec<String> = ticket_paths.iter().map(|p| shell_escape(p)).collect();
    let mut command = template
        .replace("{rel_path}", &shell_escape(rel_path))
        .replace("{ticket_paths}", &escaped_paths.join(" "))
        .replace("{phase}", phase)
        .replace("{cycle}", &cycle.to_string());

    if pipeline_vars.is_some()
        || command.contains("{source_tree}")
        || command.contains("{workspace_root}")
    {
        let ctx = crate::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            cycle,
            phase.to_string(),
            workspace_root.to_path_buf(),
            workspace_id.to_string(),
        );
        command = ctx.render_template_with_pipeline(&command, pipeline_vars);
    }

    run_phase_with_timeout(
        state,
        PhaseRunRequest {
            task_id,
            item_id,
            step_id,
            phase,
            tty,
            command,
            workspace_root,
            workspace_id,
            agent_id: &agent_id,
            runtime,
            step_timeout_secs,
        },
    )
    .await
}
