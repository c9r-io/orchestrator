use crate::config::{TaskExecutionPlan, TaskRuntimeContext};
use crate::config_load::{build_execution_plan, read_active_config, resolve_workspace_path};
use crate::events::insert_event;
use crate::state::{InnerState, TASK_SEMAPHORE};
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::task_state::set_task_status;
use super::{run_task_loop, RunningTask};
use crate::runner::kill_child_process_group;

pub async fn kill_current_child(runtime: &RunningTask) {
    let mut child_lock = runtime.child.lock().await;
    if let Some(ref mut child) = *child_lock {
        kill_child_process_group(child).await;
    }
    *child_lock = None;
}

pub async fn spawn_task_runner(state: Arc<InnerState>, task_id: String) -> Result<()> {
    {
        let mut running = state.running.lock().await;
        if running.contains_key(&task_id) {
            return Ok(());
        }
        running.insert(task_id.clone(), RunningTask::new());
    }

    let permit = TASK_SEMAPHORE
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to acquire semaphore: {}", e))?;

    tokio::spawn(async move {
        let runtime = {
            let running = state.running.lock().await;
            running.get(&task_id).cloned()
        };

        if let Some(runtime) = runtime {
            let run_result = run_task_loop(state.clone(), &task_id, runtime.clone()).await;
            if let Err(err) = run_result {
                let _ = set_task_status(&state, &task_id, "failed", false);
                let _ = insert_event(
                    &state,
                    &task_id,
                    None,
                    "task_failed",
                    json!({"error": err.to_string()}),
                );
                state.emit_event(
                    &task_id,
                    None,
                    "task_failed",
                    json!({"error": err.to_string()}),
                );
            }
        }

        drop(permit);

        let mut running = state.running.lock().await;
        running.remove(&task_id);
    });

    Ok(())
}

pub async fn stop_task_runtime(state: Arc<InnerState>, task_id: &str, status: &str) -> Result<()> {
    let runtime = {
        let running = state.running.lock().await;
        running.get(task_id).cloned()
    };

    if let Some(runtime) = runtime {
        // In-process path: we have a handle to the running task
        runtime.stop_flag.store(true, Ordering::SeqCst);
        kill_current_child(&runtime).await;
    } else {
        // Cross-process path: find and kill active children from DB
        kill_active_children_from_db(&state, task_id);
    }

    set_task_status(&state, task_id, status, false)?;
    insert_event(
        &state,
        task_id,
        None,
        "task_control",
        json!({"status": status}),
    )?;
    Ok(())
}

pub async fn stop_task_runtime_for_delete(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    let runtime = {
        let mut running = state.running.lock().await;
        running.remove(task_id)
    };
    if let Some(runtime) = runtime {
        runtime.stop_flag.store(true, Ordering::SeqCst);
        kill_current_child(&runtime).await;
    }
    Ok(())
}

pub async fn shutdown_running_tasks(state: Arc<InnerState>) {
    let runtimes: Vec<(String, RunningTask)> = {
        let running = state.running.lock().await;
        running
            .iter()
            .map(|(task_id, runtime)| (task_id.clone(), runtime.clone()))
            .collect()
    };

    if runtimes.is_empty() {
        return;
    }

    for (_, runtime) in &runtimes {
        runtime.stop_flag.store(true, Ordering::SeqCst);
        kill_current_child(runtime).await;
    }

    for (task_id, _) in &runtimes {
        let _ = set_task_status(&state, task_id, "paused", false);
        let _ = insert_event(
            &state,
            task_id,
            None,
            "task_paused",
            json!({"reason":"app_shutdown"}),
        );
    }

    let mut running = state.running.lock().await;
    for (task_id, _) in runtimes {
        running.remove(&task_id);
    }
}

/// Kill active child processes found via DB when we don't have an in-process handle.
/// Used by cross-process `task pause` where `state.running` is empty.
fn kill_active_children_from_db(state: &InnerState, task_id: &str) {
    let pids = match state.db_writer.find_active_child_pids(task_id) {
        Ok(pids) => pids,
        Err(_) => return,
    };
    for pid in pids {
        #[cfg(unix)]
        {
            // SAFETY: kill(-pid, SIGKILL) sends SIGKILL to the process group.
            // The pid was recorded from a child we spawned, so the group belongs to us.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

pub fn load_task_runtime_context(state: &InnerState, task_id: &str) -> Result<TaskRuntimeContext> {
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let runtime_row = repo.load_task_runtime_row(task_id)?;
    let workspace_id = runtime_row.workspace_id;
    let workflow_id = runtime_row.workflow_id;
    let workspace_root_raw = runtime_row.workspace_root_raw;
    let ticket_dir = runtime_row.ticket_dir;
    let execution_plan_json = runtime_row.execution_plan_json;
    let current_cycle = runtime_row.current_cycle;
    let init_done = runtime_row.init_done;
    let task_goal = runtime_row.goal;

    let active = read_active_config(state)?;
    let workflow = active
        .config
        .workflows
        .get(&workflow_id)
        .with_context(|| format!("workflow not found for task {}: {}", task_id, workflow_id))?;

    let mut execution_plan = serde_json::from_str::<TaskExecutionPlan>(&execution_plan_json)
        .ok()
        .filter(|plan| !plan.steps.is_empty())
        .unwrap_or_else(|| {
            build_execution_plan(&active.config, workflow, &workflow_id).unwrap_or(
                TaskExecutionPlan {
                    steps: Vec::new(),
                    loop_policy: crate::config::WorkflowLoopConfig::default(),
                    finalize: crate::config::default_workflow_finalize_config(),
                },
            )
        });
    if execution_plan.finalize.rules.is_empty() {
        execution_plan.finalize = crate::config::default_workflow_finalize_config();
    }
    if execution_plan.steps.is_empty() {
        anyhow::bail!(
            "[EMPTY_PLAN] task '{}' has empty execution plan\n  category: runtime\n  suggested_fix: ensure the workflow has at least one enabled step",
            task_id
        );
    }

    let workspace_root = PathBuf::from(workspace_root_raw);
    if !workspace_root.exists() {
        anyhow::bail!(
            "workspace root does not exist for task {}: {}",
            task_id,
            workspace_root.display()
        );
    }
    let workspace_root = workspace_root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize workspace root for task {}", task_id))?;
    resolve_workspace_path(&workspace_root, &ticket_dir, "task.ticket_dir")?;

    let dynamic_steps = workflow.dynamic_steps.clone();
    let safety = workflow.safety.clone();
    let self_referential = active
        .config
        .workspaces
        .get(&workspace_id)
        .map(|ws| ws.self_referential)
        .unwrap_or(false);

    if self_referential
        || workflow.safety.profile == crate::config::WorkflowSafetyProfile::SelfReferentialProbe
    {
        crate::config_load::validate_self_referential_safety(
            workflow,
            &workflow_id,
            &workspace_id,
            self_referential,
        )?;
    }

    Ok(TaskRuntimeContext {
        workspace_id,
        workspace_root,
        ticket_dir,
        execution_plan,
        current_cycle: current_cycle.max(0) as u32,
        init_done: init_done == 1,
        dynamic_steps,
        pipeline_vars: {
            let mut pv = crate::config::PipelineVariables::default();
            if !task_goal.is_empty() {
                pv.vars.insert("goal".to_string(), task_goal);
            }
            pv
        },
        safety,
        self_referential,
        consecutive_failures: 0,
    })
}
