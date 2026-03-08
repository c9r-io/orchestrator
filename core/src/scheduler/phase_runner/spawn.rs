use crate::config::StepScope;
use crate::events::insert_event;
use crate::runner::spawn_with_runner;
use crate::session_store;
use crate::state::InnerState;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use super::types::{PhaseSetup, SpawnResult};
use super::util::{shell_escape, step_scope_label};
use super::RunningTask;

/// Stage 2: TTY allocation, session creation, process spawning, stdin write.
/// Returns early for TTY sessions or hands back spawn metadata.
#[allow(clippy::too_many_arguments)]
pub(super) async fn spawn_phase_process(
    state: &Arc<InnerState>,
    setup: &mut PhaseSetup,
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    tty: bool,
    workspace_root: &Path,
    agent_id: &str,
    runtime: &RunningTask,
    step_scope: StepScope,
    prompt_payload: &Option<String>,
    req_pipe_stdin: bool,
) -> Result<SpawnResult> {
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
            setup.command
        );
        let wrapped = format!(
            "{} < {}",
            inner,
            shell_escape(&input_fifo.to_string_lossy())
        );
        session_id = Some(sid.clone());
        state
            .session_store
            .insert_session(session_store::OwnedNewSession {
                id: sid.clone(),
                task_id: task_id.to_owned(),
                task_item_id: Some(item_id.to_owned()),
                step_id: step_id.to_owned(),
                phase: phase.to_owned(),
                agent_id: agent_id.to_owned(),
                state: "active".to_owned(),
                pid: 0,
                pty_backend: "script".to_owned(),
                cwd: workspace_root.to_string_lossy().into_owned(),
                command: setup.command.clone(),
                input_fifo_path: input_fifo.to_string_lossy().into_owned(),
                stdout_path: setup.stdout_path.to_string_lossy().into_owned(),
                stderr_path: setup.stderr_path.to_string_lossy().into_owned(),
                transcript_path: transcript_path.to_string_lossy().into_owned(),
                output_json_path: Some(output_json_path.to_string_lossy().into_owned()),
            })
            .await?;
        wrapped
    } else {
        setup.command.clone()
    };
    // For stdin delivery in TTY mode, we already warned and fell back to arg
    let effective_pipe_stdin = req_pipe_stdin && !tty;
    let mut child = spawn_with_runner(
        &setup.runner,
        &command_to_run,
        workspace_root,
        // Take files out of setup; they are consumed by spawn
        std::mem::replace(&mut setup.stdout_file, tempfile_placeholder()?),
        std::mem::replace(&mut setup.stderr_file, tempfile_placeholder()?),
        &setup.resolved_extra_env,
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
            let _ = state
                .session_store
                .update_session_pid(sid, pid as i64)
                .await;
        }
    }

    if tty && session_id.is_some() {
        std::mem::forget(child);
        return Ok(SpawnResult {
            session_id,
            child_pid: None,
            tty_early_return: Some(crate::dto::RunResult {
                success: true,
                exit_code: 0,
                stdout_path: setup.stdout_path.to_string_lossy().to_string(),
                stderr_path: setup.stderr_path.to_string_lossy().to_string(),
                timed_out: false,
                duration_ms: Some(0),
                output: None,
                validation_status: "passed".to_string(),
                agent_id: agent_id.to_string(),
                run_id: setup.run_id.clone(),
            }),
        });
    }

    let child_pid = child.id();
    // Write PID to command_runs so cross-process pause can find and kill it
    if let Some(pid) = child_pid {
        let _ = state
            .db_writer
            .update_command_run_pid(&setup.run_id, pid as i64)
            .await;
    }
    let preview: String = setup.command.chars().take(120).collect();
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
            "run_id": setup.run_id,
            "pid": child_pid,
            "command_preview": preview,
        }),
    )
    .await?;

    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = Some(child);
    }

    Ok(SpawnResult {
        session_id,
        child_pid,
        tty_early_return: None,
    })
}

/// Create a throwaway file handle used as a placeholder after the real file is moved out.
fn tempfile_placeholder() -> Result<std::fs::File> {
    // Open /dev/null as a cheap placeholder; the value is never used.
    std::fs::File::open("/dev/null").context("failed to open /dev/null placeholder")
}
