use agent_orchestrator::config::StepScope;
use agent_orchestrator::driver::{DriverStartRequest, SessionRef, create_driver, driver_id};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::runner::spawn_with_runner_and_capture;
use agent_orchestrator::session_store;
use agent_orchestrator::state::InnerState;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use uuid::Uuid;

use super::RunningTask;
use super::types::{PhaseSetup, SpawnResult};
use super::util::{shell_escape, step_scope_label};
use crate::scheduler::coordination_tools::{CoordinationHostRequest, start_tool_host};

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
    resume_provider_session: bool,
    enable_coordination_tools: bool,
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
        let wrapped = wrap_session_stdin(&inner, &input_fifo.to_string_lossy());
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
    let typed_shell_tty = tty && supports_tty_driver(setup.driver.as_ref());
    if let Some(driver_config) = setup.driver.clone()
        && !typed_shell_tty
    {
        if tty {
            anyhow::bail!("only the shell/cli Agent driver supports TTY sessions");
        }
        let driver = create_driver(&driver_config)?;
        let provider_session =
            recalled_provider_session_for_step(task_id, resume_provider_session).await;
        let prompt = prompt_payload.as_deref().unwrap_or_default();
        let tool_host = if should_start_coordination_tool_host(
            enable_coordination_tools,
            driver.capabilities().tool_hosting,
        ) {
            let host = start_tool_host(CoordinationHostRequest {
                state: state.clone(),
                task_id,
                item_id,
                run_id: &setup.run_id,
                workspace_root,
                runner: &setup.runner,
                execution_profile: &setup.execution_profile,
                extra_env: &setup.resolved_extra_env,
                redaction_patterns: &setup.redaction_patterns,
                artifacts_dir: &setup.artifacts_dir,
                allowed_tools: &driver_config.options.allowed_tools,
            })
            .await?;
            setup
                .redaction_patterns
                .push(host.callback().expose_token().to_string());
            Some(host)
        } else {
            None
        };
        let session = driver
            .start(DriverStartRequest {
                driver: &driver_config,
                runner: &setup.runner,
                shell_command: &command_to_run,
                stdin_payload: if effective_pipe_stdin {
                    prompt_payload.as_deref()
                } else {
                    None
                },
                prompt,
                cwd: workspace_root,
                stdout: std::mem::replace(&mut setup.stdout_file, tempfile_placeholder()?),
                stderr: std::mem::replace(&mut setup.stderr_file, tempfile_placeholder()?),
                redaction_patterns: &setup.redaction_patterns,
                extra_env: &setup.resolved_extra_env,
                execution_profile: &setup.execution_profile,
                artifacts_dir: &setup.artifacts_dir,
                session_ref: provider_session.as_ref(),
                mcp_callback: tool_host.as_ref().map(|host| host.callback()),
            })
            .await?;
        let child_pid = session.pid();
        if let Some(pid) = child_pid {
            let _ = state
                .db_writer
                .update_command_run_pid(&setup.run_id, pid as i64)
                .await;
        }
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
                "driver": driver_id(&driver_config),
                "execution_profile": setup.execution_profile.name,
            }),
        )
        .await?;
        return Ok(SpawnResult {
            session_id: None,
            child_pid,
            output_capture: None,
            driver_session: Some(session),
            coordination_tool_host: tool_host,
            tty_early_return: None,
        });
    }
    // FR-173 retired the `[legacy_agent_execution_removed]` code but not this
    // guard. Immediately below is the direct-command spawn path, which the
    // comment there marks as *not* Agent execution; without this check a
    // driverless Agent would take it and run, which is the pre-driver execution
    // path the abstraction exists to have removed. Removing the guard would buy
    // one fewer error code at the price of a silent wrong execution.
    //
    // The advice changed with it: re-applying a command-only Agent no longer
    // promotes it to shell/cli, it is rejected, so the manifest is where the
    // driver has to be declared.
    if setup.driver.is_none() && agent_id != "builtin" {
        anyhow::bail!(
            "Agent '{agent_id}' has no typed driver; declare `spec.driver` in its manifest and re-apply"
        );
    }
    // Engine-owned direct Step commands deliberately keep the shared safe
    // spawn substrate. They are not Agent execution and therefore do not
    // require a provider driver.
    let captured = spawn_with_runner_and_capture(
        &setup.runner,
        &command_to_run,
        workspace_root,
        std::mem::replace(&mut setup.stdout_file, tempfile_placeholder()?),
        std::mem::replace(&mut setup.stderr_file, tempfile_placeholder()?),
        setup.redaction_patterns.clone(),
        &setup.resolved_extra_env,
        effective_pipe_stdin,
        &setup.execution_profile,
    )?;
    let mut child = captured.child;
    let mut output_capture = Some(captured.output_capture);

    // Write prompt to child stdin for stdin delivery mode
    if effective_pipe_stdin
        && let Some(payload) = &prompt_payload
        && let Some(mut stdin_handle) = child.stdin.take()
    {
        use tokio::io::AsyncWriteExt;
        stdin_handle.write_all(payload.as_bytes()).await?;
        drop(stdin_handle); // send EOF
    }

    if let Some(sid) = session_id.as_deref()
        && let Some(pid) = child.id()
    {
        let fingerprint = session_store::capture_process_fingerprint(pid);
        let sid_owned = sid.to_owned();
        let _ = session_store::update_session_process_async(
            &state.async_database,
            sid_owned,
            pid as i64,
            fingerprint,
        )
        .await;
    }

    if tty && session_id.is_some() {
        let sid = session_id.clone().unwrap_or_default();
        let state_for_wait = state.clone();
        let capture = output_capture.take();
        tokio::spawn(async move {
            let status = child.wait().await;
            if let Some(capture) = capture {
                let _ = capture.wait().await;
            }
            let (session_state, exit_code) = match status {
                Ok(status) if status.success() => ("closed", status.code()),
                Ok(status) => ("failed", status.code()),
                Err(_) => ("failed", None),
            };
            let _ = state_for_wait
                .session_store
                .update_session_state(&sid, session_state, exit_code.map(i64::from), true)
                .await;
        });
        return Ok(SpawnResult {
            session_id,
            child_pid: None,
            output_capture: None,
            driver_session: None,
            coordination_tool_host: None,
            tty_early_return: Some(agent_orchestrator::dto::RunResult {
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
                execution_profile: setup.execution_profile.name.clone(),
                execution_mode: match setup.execution_profile.mode {
                    agent_orchestrator::config::ExecutionProfileMode::Host => "host".to_string(),
                    agent_orchestrator::config::ExecutionProfileMode::Sandbox => {
                        "sandbox".to_string()
                    }
                },
                sandbox_denied: false,
                sandbox_denial_reason: None,
                sandbox_violation_kind: None,
                sandbox_resource_kind: None,
                sandbox_network_target: None,
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
            "driver": setup.driver.as_ref().map(driver_id),
            "command_preview": preview,
            "execution_profile": setup.execution_profile.name,
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
        output_capture,
        driver_session: None,
        coordination_tool_host: None,
        tty_early_return: None,
    })
}

/// Binds an interactive session's stdin to its input FIFO.
///
/// The redirect is `0<>` (read-write), not `<` (read-only), so the session
/// process itself holds a writer on the FIFO for its whole life and stdin never
/// reaches EOF.
///
/// Under a read-only `<`, the FIFO returns EOF the moment the last writer
/// closes — and `write_fifo_atomically` opens, writes and closes on *every*
/// `send-input`. A session that blocks on `read` therefore exits after the first
/// message it is ever sent, and a session that loops on EOF instead spins at
/// whatever rate its loop allows. The mock fixture's `sleep 0.05` poll was a
/// workaround for exactly this, and it cost ~315 minutes of CPU per orphaned
/// process (FR-159).
///
/// Both the premature exit and the spin are properties of this redirect rather
/// than of any particular agent command, which is why the fix lives here and not
/// in each command template.
fn wrap_session_stdin(inner: &str, input_fifo_path: &str) -> String {
    format!("{} 0<> {}", inner, shell_escape(input_fifo_path))
}

fn provider_sessions() -> &'static tokio::sync::Mutex<std::collections::HashMap<String, SessionRef>>
{
    static SESSIONS: OnceLock<tokio::sync::Mutex<std::collections::HashMap<String, SessionRef>>> =
        OnceLock::new();
    SESSIONS.get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn recalled_provider_session(task_id: &str) -> Option<SessionRef> {
    provider_sessions().lock().await.get(task_id).cloned()
}

async fn recalled_provider_session_for_step(
    task_id: &str,
    resume_provider_session: bool,
) -> Option<SessionRef> {
    if resume_provider_session {
        recalled_provider_session(task_id).await
    } else {
        None
    }
}

fn should_start_coordination_tool_host(
    enabled: bool,
    hosting: agent_orchestrator::config::ToolHosting,
) -> bool {
    enabled && hosting == agent_orchestrator::config::ToolHosting::Stdio
}

fn supports_tty_driver(driver: Option<&agent_orchestrator::config::AgentDriverConfig>) -> bool {
    driver.is_some_and(|driver| {
        driver.provider == agent_orchestrator::config::DriverProvider::Shell
            && driver.transport == agent_orchestrator::config::DriverTransport::Cli
    })
}

pub(super) async fn remember_provider_session(task_id: &str, reference: SessionRef) {
    provider_sessions()
        .lock()
        .await
        .insert(task_id.to_string(), reference);
}

/// Create a throwaway file handle used as a placeholder after the real file is moved out.
fn tempfile_placeholder() -> Result<std::fs::File> {
    // Open /dev/null as a cheap placeholder; the value is never used.
    std::fs::File::open("/dev/null").context("failed to open /dev/null placeholder")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shell reader used by both redirect tests: signal readiness, then
    /// append every line received to the sink.
    ///
    /// The readiness touch happens after the shell has applied its redirect, so
    /// the driver can wait for a reader that genuinely has the FIFO open.
    #[cfg(unix)]
    fn reader_program(sink: &Path, ready: &Path) -> String {
        format!(
            ": > {ready_path}; while IFS= read -r line; do printf '%s\\n' \"$line\" >> {sink_path}; done",
            ready_path = shell_escape(&ready.to_string_lossy()),
            sink_path = shell_escape(&sink.to_string_lossy())
        )
    }

    /// Drives a shell reader through repeated writer open/close cycles and
    /// reports how many of the messages it actually received, plus whether it
    /// was still alive at the end.
    ///
    /// Deliberately mirrors `write_fifo_atomically`: each message is a separate
    /// open, write and close, because that is what every `send-input` does and
    /// it is precisely the pattern that makes the redirect choice observable.
    #[cfg(unix)]
    fn drive_fifo_reader(
        command: &str,
        fifo: &Path,
        sink: &Path,
        ready: &Path,
        messages: &[&str],
    ) -> (usize, bool) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .spawn()
            .expect("spawn shell reader");

        // The reader touches `ready` only after its redirect has been applied,
        // so this waits for the FIFO to actually have an open reader rather than
        // guessing at a startup delay. Racing this would drop the first message
        // on ENXIO and make the test flaky in the direction that looks like the
        // defect it is guarding against.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(ready.exists(), "shell reader never signalled readiness");

        for message in messages {
            // Retry briefly: a live reader can be mid-loop between reads. ENXIO
            // that persists past this window means the reader is gone, which is
            // itself an outcome under test, so give up and record nothing.
            let send_deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            loop {
                let mut options = std::fs::OpenOptions::new();
                options.write(true).custom_flags(libc::O_NONBLOCK);
                match options.open(fifo) {
                    Ok(mut handle) => {
                        let _ = handle.write_all(format!("{message}\n").as_bytes());
                        break;
                    }
                    Err(_) if std::time::Instant::now() < send_deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(120));
        }

        std::thread::sleep(std::time::Duration::from_millis(150));
        let alive = child.try_wait().expect("poll shell reader").is_none();
        let received = std::fs::read_to_string(sink)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .count();
        let _ = child.kill();
        let _ = child.wait();
        (received, alive)
    }

    /// A session must survive every `send-input`, not just the first.
    ///
    /// This is the behavioral half of the `0<>` redirect. A text assertion that
    /// the command string contains `0<>` would pass just as happily on a
    /// redirect that silently kills the session, because what is under test is
    /// whether the process is still there to read the second message — and a
    /// grep cannot see that.
    #[cfg(unix)]
    #[test]
    fn session_stdin_survives_repeated_writer_open_close_cycles() {
        let dir = tempfile::Builder::new()
            .prefix("session-stdin-")
            .tempdir()
            .expect("temp dir");
        let fifo = dir.path().join("input.fifo");
        let sink = dir.path().join("received.txt");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("run mkfifo")
                .success()
        );

        let ready = dir.path().join("ready");
        let inner = reader_program(&sink, &ready);
        let wrapped = wrap_session_stdin(&inner, &fifo.to_string_lossy());
        std::fs::write(&sink, "").expect("seed sink");

        let (received, alive) = drive_fifo_reader(
            &wrapped,
            &fifo,
            &sink,
            &ready,
            &["first", "second", "third"],
        );

        assert_eq!(
            received, 3,
            "session must receive every message across separate writer open/close cycles"
        );
        assert!(
            alive,
            "session must still be running after its writers have come and gone"
        );
    }

    /// The negative fixture for the test above: the redirect this replaced.
    ///
    /// Without it, `session_stdin_survives_repeated_writer_open_close_cycles`
    /// proves nothing — it would pass on any redirect that happens to work,
    /// including one that never had the defect. This pins the defect itself:
    /// under a read-only `<`, the reader takes the first message and exits.
    ///
    /// It asserts the *diagnostic* (how many messages got through), not merely
    /// that something went wrong, so a failure names which way it broke.
    #[cfg(unix)]
    #[test]
    fn read_only_redirect_loses_the_session_after_the_first_message() {
        let dir = tempfile::Builder::new()
            .prefix("session-stdin-ro-")
            .tempdir()
            .expect("temp dir");
        let fifo = dir.path().join("input.fifo");
        let sink = dir.path().join("received.txt");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("run mkfifo")
                .success()
        );

        let ready = dir.path().join("ready");
        let inner = reader_program(&sink, &ready);
        let read_only = format!("{} < {}", inner, shell_escape(&fifo.to_string_lossy()));
        std::fs::write(&sink, "").expect("seed sink");

        let (received, alive) = drive_fifo_reader(
            &read_only,
            &fifo,
            &sink,
            &ready,
            &["first", "second", "third"],
        );

        assert_eq!(
            received, 1,
            "the read-only redirect should deliver exactly the first message before EOF"
        );
        assert!(
            !alive,
            "the read-only redirect should leave the session dead, which is the defect 0<> fixes"
        );
    }

    #[tokio::test]
    async fn provider_session_attachment_is_step_opt_in() {
        let task_id = format!("session-opt-in-{}", Uuid::new_v4());
        remember_provider_session(
            &task_id,
            SessionRef::from_provider("private-provider-session".to_string())
                .expect("valid reference"),
        )
        .await;

        assert!(
            recalled_provider_session_for_step(&task_id, false)
                .await
                .is_none()
        );
        let recalled = recalled_provider_session_for_step(&task_id, true)
            .await
            .expect("opted-in step should resume");
        assert_eq!(recalled.expose_secret(), "private-provider-session");
    }

    #[test]
    fn coordination_tool_host_is_step_opt_in_and_driver_capability_gated() {
        use agent_orchestrator::config::ToolHosting;

        assert!(!should_start_coordination_tool_host(
            false,
            ToolHosting::Stdio
        ));
        assert!(!should_start_coordination_tool_host(
            true,
            ToolHosting::None
        ));
        assert!(should_start_coordination_tool_host(
            true,
            ToolHosting::Stdio
        ));
    }

    #[test]
    fn tty_is_only_supported_by_typed_shell_cli_driver() {
        let shell = agent_orchestrator::config::AgentDriverConfig::shell_cli();
        let mut claude = shell.clone();
        claude.provider = agent_orchestrator::config::DriverProvider::Claude;
        let mut shell_sdk = shell.clone();
        shell_sdk.transport = agent_orchestrator::config::DriverTransport::Sdk;

        assert!(supports_tty_driver(Some(&shell)));
        assert!(!supports_tty_driver(Some(&claude)));
        assert!(!supports_tty_driver(Some(&shell_sdk)));
        assert!(!supports_tty_driver(None));
    }
}
