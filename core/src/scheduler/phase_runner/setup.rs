use crate::config::PromptDelivery;
use crate::config_load::now_ts;
use crate::state::InnerState;
use crate::task_repository::NewCommandRun;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use super::types::PhaseSetup;

/// Stage 1: Destructure request, load config, create log files, insert initial DB record.
#[allow(clippy::too_many_arguments)]
pub(super) async fn setup_phase_execution(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    tty: bool,
    command: String,
    workspace_root: &Path,
    workspace_id: &str,
    agent_id: &str,
    prompt_delivery: PromptDelivery,
    prompt_payload: &Option<String>,
) -> Result<PhaseSetup> {
    let now = now_ts();
    let run_uuid = Uuid::new_v4();
    let run_id = run_uuid.to_string();
    let logs_dir = state.logs_dir.join(task_id);
    let stdout_path = logs_dir.join(format!("{}_{}.stdout", phase, run_id));
    let stderr_path = logs_dir.join(format!("{}_{}.stderr", phase, run_id));

    let (runner, mut resolved_extra_env, sensitive_values) = {
        let active = crate::config_load::read_active_config(state)?;
        let mut runner = active.config.runner.clone();
        if state.unsafe_mode {
            runner.policy = crate::config::RunnerPolicy::Unsafe;
        }
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
                (HashMap::new(), Vec::new())
            }
        } else {
            (HashMap::new(), Vec::new())
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
        state.db_writer.insert_command_run(&initial_run).await?;
    }

    Ok(PhaseSetup {
        run_uuid,
        run_id,
        now,
        runner,
        resolved_extra_env,
        redaction_patterns,
        stdout_path,
        stderr_path,
        stdout_file,
        stderr_file,
        command,
    })
}
