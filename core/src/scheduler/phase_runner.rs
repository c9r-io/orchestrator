use crate::config::{PipelineVariables, PromptDelivery, StepScope};
use crate::config_load::now_ts;
use crate::events::insert_event;
use crate::health::{
    increment_consecutive_errors, mark_agent_diseased, reset_consecutive_errors,
    update_capability_health,
};
use crate::metrics::MetricsCollector;
use crate::output_validation::validate_phase_output;
use crate::runner::{kill_child_process_group, redact_text, spawn_with_runner};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::session_store;
use crate::state::{read_agent_health, read_agent_metrics, write_agent_metrics, InnerState};
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
const LOW_OUTPUT_DELTA_THRESHOLD_BYTES: u64 = 32;
const LOW_OUTPUT_MIN_ELAPSED_SECS: u64 = 90;
const LOW_OUTPUT_CONSECUTIVE_HEARTBEATS: u32 = 3;
const VALIDATION_FAILED_EXIT_CODE: i64 = -6;

struct LimitedOutput {
    text: String,
    truncated_prefix_bytes: u64,
}

#[derive(Default)]
struct HeartbeatProgress {
    last_stdout_bytes: u64,
    last_stderr_bytes: u64,
    stagnant_heartbeats: u32,
}

struct HeartbeatSample {
    stdout_bytes: u64,
    stderr_bytes: u64,
    stdout_delta_bytes: u64,
    stderr_delta_bytes: u64,
    stagnant_heartbeats: u32,
    output_state: &'static str,
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
    pub step_scope: StepScope,
    /// How the prompt payload is delivered to the agent process.
    pub prompt_delivery: PromptDelivery,
    /// Rendered prompt for non-arg delivery modes (stdin, file, env).
    pub prompt_payload: Option<String>,
    /// Whether to pipe stdin to the child process.
    pub pipe_stdin: bool,
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
    pub step_scope: StepScope,
    /// Prompt from a resolved StepTemplate, injected into the agent command's {prompt} placeholder
    pub step_template_prompt: Option<&'a str>,
    /// Project ID for project-scoped agent selection (empty = global)
    pub project_id: &'a str,
}

fn step_scope_label(scope: StepScope) -> &'static str {
    match scope {
        StepScope::Task => "task",
        StepScope::Item => "item",
    }
}

pub(crate) fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn resolved_step_timeout_secs(step_timeout_secs: Option<u64>) -> u64 {
    step_timeout_secs.unwrap_or(DEFAULT_STEP_TIMEOUT_SECS)
}

fn effective_exit_code(exit_code: i64, validation_status: &str) -> i64 {
    if exit_code == 0 && validation_status == "failed" {
        VALIDATION_FAILED_EXIT_CODE
    } else {
        exit_code
    }
}

fn sample_heartbeat_progress(
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
        step_scope,
        prompt_delivery,
        prompt_payload,
        pipe_stdin: req_pipe_stdin,
    } = request;
    let now = now_ts();
    let run_uuid = Uuid::new_v4();
    let run_id = run_uuid.to_string();
    let logs_dir = state.logs_dir.join(task_id);
    let stdout_path = logs_dir.join(format!("{}_{}.stdout", phase, run_id));
    let stderr_path = logs_dir.join(format!("{}_{}.stderr", phase, run_id));

    let (runner, mut resolved_extra_env, sensitive_values) = {
        let active = crate::config_load::read_active_config(state)?;
        let runner = active.config.runner.clone();
        let (extra_env, sensitive) = if let Some(agent_cfg) = active.config.agents.get(agent_id) {
            if let Some(ref env_entries) = agent_cfg.env {
                let env =
                    crate::env_resolve::resolve_agent_env(env_entries, &active.config.env_stores)?;
                let sens = crate::env_resolve::collect_sensitive_values(
                    env_entries,
                    &active.config.env_stores,
                );
                (env, sens)
            } else {
                (std::collections::HashMap::new(), Vec::new())
            }
        } else {
            (std::collections::HashMap::new(), Vec::new())
        };
        (runner, extra_env, sensitive)
    };
    let mut redaction_patterns = runner.redaction_patterns.clone();
    redaction_patterns.extend(sensitive_values);
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

    // Handle non-arg prompt delivery modes before spawn
    let command = match prompt_delivery {
        PromptDelivery::File => {
            if let Some(ref payload) = prompt_payload {
                let prompt_file_path = logs_dir.join(format!("prompt_{}.txt", run_id));
                std::fs::write(&prompt_file_path, payload).with_context(|| {
                    format!(
                        "failed to write prompt file: {}",
                        prompt_file_path.display()
                    )
                })?;
                command.replace("{prompt_file}", &prompt_file_path.to_string_lossy())
            } else {
                command
            }
        }
        PromptDelivery::Env => {
            if let Some(ref payload) = prompt_payload {
                const ENV_SIZE_LIMIT: usize = 128 * 1024;
                if payload.len() > ENV_SIZE_LIMIT {
                    tracing::warn!(
                        agent_id = %agent_id,
                        prompt_bytes = payload.len(),
                        "prompt exceeds env var size limit (~128KB); consider using file delivery"
                    );
                }
                resolved_extra_env.insert("ORCH_PROMPT".to_string(), payload.clone());
            }
            command
        }
        PromptDelivery::Stdin if tty => {
            tracing::warn!(
                agent_id = %agent_id,
                "stdin delivery conflicts with TTY mode (stdin redirected from FIFO); falling back to arg"
            );
            // Fall back: no stdin piping, command already has {prompt} stripped
            command
        }
        _ => command,
    };

    // Insert a "running" command_run record immediately so `task logs` shows it during execution
    {
        let initial_run = NewCommandRun {
            id: run_id.clone(),
            task_item_id: item_id.to_string(),
            phase: phase.to_string(),
            command: command.clone(),
            cwd: workspace_root.to_string_lossy().to_string(),
            workspace_id: workspace_id.to_string(),
            agent_id: agent_id.to_string(),
            exit_code: -1,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now.clone(),
            ended_at: String::new(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "running".to_string(),
            session_id: None,
            machine_output_source: if tty {
                "output_json_path".to_string()
            } else {
                "stdout".to_string()
            },
            output_json_path: None,
        };
        state.db_writer.insert_command_run(&initial_run)?;
    }

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
    // For stdin delivery in TTY mode, we already warned and fell back to arg
    let effective_pipe_stdin = req_pipe_stdin && !tty;
    let mut child = spawn_with_runner(
        &runner,
        &command_to_run,
        workspace_root,
        stdout_file,
        stderr_file,
        &resolved_extra_env,
        effective_pipe_stdin,
    )?;

    // Write prompt to child stdin for stdin delivery mode
    if effective_pipe_stdin {
        if let Some(ref payload) = prompt_payload {
            if let Some(mut stdin_handle) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin_handle.write_all(payload.as_bytes()).await?;
                drop(stdin_handle); // send EOF
            }
        }
    }

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
    // Write PID to command_runs so cross-process pause can find and kill it
    if let Some(pid) = child_pid {
        let _ = state.db_writer.update_command_run_pid(&run_id, pid as i64);
    }
    let preview: String = command.chars().take(120).collect();
    insert_event(
        state,
        task_id,
        Some(item_id),
        "step_spawned",
        json!({
            "step": phase,
            "step_id": step_id,
            "step_scope": step_scope_label(step_scope),
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
    let mut heartbeat_progress = HeartbeatProgress::default();

    let exit_code: i32 = loop {
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
                )?;

                // Cross-process pause: check if another process (e.g. `task pause`)
                // has marked this task as paused in the DB.
                if super::task_state::is_task_paused_in_db(state, task_id)? {
                    let mut child_lock = runtime.child.lock().await;
                    if let Some(ref mut child) = *child_lock {
                        kill_child_process_group(child).await;
                    }
                    break -5; // externally paused
                }
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
    let final_exit_code = effective_exit_code(exit_code as i64, validation.status);
    let mut success = final_exit_code == 0;
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
        exit_code: final_exit_code,
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
        writer.update_command_run_with_events(&insert_payload, &events)
    })
    .await
    .context("command run insert worker failed")??;

    update_capability_health(state, agent_id, Some(phase), success);

    let duration_ms = duration.as_millis() as u64;
    {
        let mut metrics_map = write_agent_metrics(state);
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
            Some(final_exit_code),
            true,
        );
    }

    Ok(crate::dto::RunResult {
        success,
        exit_code: final_exit_code,
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
        step_scope,
        step_template_prompt,
        project_id,
    } = request;
    let effective_capability = capability.or(match phase {
        "qa" | "fix" | "retest" => Some(phase),
        _ => None,
    });

    let (agent_id, template, prompt_delivery) = {
        let active = crate::config_load::read_active_config(state)?;
        let agents = crate::selection::resolve_effective_agents(
            project_id,
            &active.config,
            effective_capability,
        )
        .clone();

        if let Some(cap) = effective_capability {
            let health_map = read_agent_health(state);
            let metrics_map = read_agent_metrics(state);
            select_agent_advanced(cap, &agents, &health_map, &metrics_map, &HashSet::new())?
        } else {
            select_agent_by_preference(&agents)?
        }
    };

    {
        let mut metrics_map = write_agent_metrics(state);
        let metrics = metrics_map
            .entry(agent_id.clone())
            .or_insert_with(MetricsCollector::new_agent_metrics);
        MetricsCollector::increment_load(metrics);
    }

    // Render template variables into the step template prompt, then inject into agent command
    let rendered_prompt = step_template_prompt.map(|prompt| {
        let mut rendered = prompt
            .replace("{rel_path}", &shell_escape(rel_path))
            .replace("{phase}", phase)
            .replace("{cycle}", &cycle.to_string());
        let escaped_paths: Vec<String> = ticket_paths.iter().map(|p| shell_escape(p)).collect();
        rendered = rendered.replace("{ticket_paths}", &escaped_paths.join(" "));
        if pipeline_vars.is_some()
            || rendered.contains("{source_tree}")
            || rendered.contains("{workspace_root}")
        {
            let ctx = crate::collab::AgentContext::new(
                task_id.to_string(),
                item_id.to_string(),
                cycle,
                phase.to_string(),
                workspace_root.to_path_buf(),
                workspace_id.to_string(),
            );
            rendered = ctx.render_template_with_pipeline(&rendered, pipeline_vars);
        }
        rendered
    });

    // Dispatch prompt into command based on delivery mode
    let (mut command, prompt_payload) = match prompt_delivery {
        PromptDelivery::Arg => {
            let cmd = if let Some(ref prompt) = rendered_prompt {
                template.replace("{prompt}", prompt)
            } else {
                template
            };
            (cmd, None)
        }
        _ => {
            if template.contains("{prompt}") {
                tracing::warn!(
                    agent_id = %agent_id,
                    "command contains {{prompt}} but prompt_delivery={:?}; placeholder ignored",
                    prompt_delivery
                );
            }
            (template, rendered_prompt)
        }
    };

    let escaped_paths: Vec<String> = ticket_paths.iter().map(|p| shell_escape(p)).collect();
    command = command
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
            step_scope,
            prompt_delivery,
            prompt_payload,
            pipe_stdin: prompt_delivery == PromptDelivery::Stdin,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn heartbeat_sample_delta_exactly_at_threshold_counts_as_stagnant() {
        let mut progress = HeartbeatProgress::default();
        // First sample with exactly threshold bytes
        let s1 = sample_heartbeat_progress(
            &mut progress,
            LOW_OUTPUT_DELTA_THRESHOLD_BYTES,
            0,
            30,
            true,
        );
        assert_eq!(s1.stagnant_heartbeats, 1); // exactly at threshold counts as stagnant

        // Second sample with no additional output (delta = 0)
        let s2 = sample_heartbeat_progress(
            &mut progress,
            LOW_OUTPUT_DELTA_THRESHOLD_BYTES,
            0,
            60,
            true,
        );
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
