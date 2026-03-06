use crate::config::{TaskExecutionPlan, TaskRuntimeContext};
use crate::config_load::{
    build_execution_plan, build_execution_plan_for_project, read_active_config,
    resolve_workspace_path,
};
use crate::events::insert_event;
use crate::state::{task_semaphore, InnerState};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::{error, info_span, Instrument};

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

    let permit = task_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to acquire semaphore: {}", e))?;
    let span_task_id = task_id.clone();

    tokio::spawn(
        async move {
            let runtime = {
                let running = state.running.lock().await;
                running.get(&task_id).cloned()
            };

            if let Some(runtime) = runtime {
                let run_result = run_task_loop(state.clone(), &task_id, runtime.clone()).await;
                if let Err(err) = run_result {
                    error!(task_id = %task_id, error = %err, "task runner failed");
                    let _ = set_task_status(&state, &task_id, "failed", false).await;
                    let _ = insert_event(
                        &state,
                        &task_id,
                        None,
                        "task_failed",
                        json!({"error": err.to_string()}),
                    )
                    .await;
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
        }
        .instrument(info_span!("task_runner", task_id = %span_task_id)),
    );

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
        kill_active_children_from_db(&state, task_id).await;
    }

    set_task_status(&state, task_id, status, false).await?;
    insert_event(
        &state,
        task_id,
        None,
        "task_control",
        json!({"status": status}),
    )
    .await?;
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
        let _ = set_task_status(&state, task_id, "paused", false).await;
        let _ = insert_event(
            &state,
            task_id,
            None,
            "task_paused",
            json!({"reason":"app_shutdown"}),
        )
        .await;
    }

    let mut running = state.running.lock().await;
    for (task_id, _) in runtimes {
        running.remove(&task_id);
    }
}

/// Kill active child processes found via DB when we don't have an in-process handle.
/// Used by cross-process `task pause` where `state.running` is empty.
async fn kill_active_children_from_db(state: &InnerState, task_id: &str) {
    let pids = match state.db_writer.find_active_child_pids(task_id).await {
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

pub async fn load_task_runtime_context(
    state: &InnerState,
    task_id: &str,
) -> Result<TaskRuntimeContext> {
    let runtime_row = state.task_repo.load_task_runtime_row(task_id).await?;
    let workspace_id = runtime_row.workspace_id;
    let workflow_id = runtime_row.workflow_id;
    let workspace_root_raw = runtime_row.workspace_root_raw;
    let ticket_dir = runtime_row.ticket_dir;
    let execution_plan_json = runtime_row.execution_plan_json;
    let current_cycle = runtime_row.current_cycle;
    let init_done = runtime_row.init_done;
    let task_goal = runtime_row.goal;
    let project_id = runtime_row.project_id;

    let active = read_active_config(state)?;
    // Look up workflow: try project-scoped first, then global
    let workflow = active
        .config
        .projects
        .get(&project_id)
        .and_then(|p| p.workflows.get(&workflow_id))
        .or_else(|| active.config.workflows.get(&workflow_id))
        .with_context(|| format!("workflow not found for task {}: {}", task_id, workflow_id))?;

    let mut execution_plan = serde_json::from_str::<TaskExecutionPlan>(&execution_plan_json)
        .ok()
        .filter(|plan| !plan.steps.is_empty())
        .unwrap_or_else(|| {
            build_execution_plan_for_project(&active.config, workflow, &workflow_id, &project_id)
                .or_else(|_| build_execution_plan(&active.config, workflow, &workflow_id))
                .unwrap_or(TaskExecutionPlan {
                    steps: Vec::new(),
                    loop_policy: crate::config::WorkflowLoopConfig::default(),
                    finalize: crate::config::default_workflow_finalize_config(),
                    max_parallel: None,
                })
        });
    // Layer 1 defense: re-normalize builtin steps whose `behavior.execution`
    // may have been stored as the serde default `Agent` in SQLite.
    for step in &mut execution_plan.steps {
        step.renormalize_execution_mode();
    }
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
        .projects
        .get(&project_id)
        .and_then(|p| p.workspaces.get(&workspace_id))
        .or_else(|| active.config.workspaces.get(&workspace_id))
        .map(|ws| ws.self_referential)
        .unwrap_or(false);

    if !state.unsafe_mode
        && (self_referential
            || workflow.safety.profile
                == crate::config::WorkflowSafetyProfile::SelfReferentialProbe)
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
            // Recover spill file paths that were lost across process restart
            // (e.g. self_restart exit 75 → relaunch). The spill files persist
            // on disk at a deterministic path: {logs_dir}/{task_id}/{key}.txt
            let spill_dir = state.logs_dir.join(task_id);
            if spill_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&spill_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("txt") {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                let path_key = format!("{}_path", stem);
                                if !pv.vars.contains_key(&path_key) {
                                    pv.vars.insert(
                                        path_key,
                                        path.to_string_lossy().to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            pv
        },
        safety,
        self_referential,
        consecutive_failures: 0,
        project_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkflowSafetyProfile;
    use crate::config_load::{build_execution_plan, read_active_config};
    use crate::db::open_conn;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;
    use rusqlite::params;

    fn seed_task(fixture: &mut TestState) -> (Arc<InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/runtime_test.md");
        std::fs::write(&qa_file, "# runtime test\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("runtime-test".to_string()),
                goal: Some("runtime-test-goal".to_string()),
                ..Default::default()
            },
        )
        .expect("create task");
        (state, created.id)
    }

    #[tokio::test]
    async fn load_task_runtime_context_normalizes_fields() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let plan_json = {
            let active = read_active_config(&state).expect("read active config");
            let workflow = active
                .config
                .workflows
                .get(&active.default_workflow_id)
                .expect("default workflow");
            let mut plan =
                build_execution_plan(&active.config, workflow, &active.default_workflow_id)
                    .expect("build execution plan");
            plan.finalize.rules.clear();
            serde_json::to_string(&plan).expect("serialize plan")
        };

        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET execution_plan_json = ?2, current_cycle = -4, init_done = 1 WHERE id = ?1",
            params![
                task_id.clone(),
                plan_json
            ],
        )
        .expect("update task");

        let ctx = load_task_runtime_context(&state, &task_id)
            .await
            .expect("load runtime context");
        assert_eq!(ctx.current_cycle, 0);
        assert!(ctx.init_done);
        assert!(!ctx.execution_plan.finalize.rules.is_empty());
        assert_eq!(
            ctx.pipeline_vars.vars.get("goal").map(String::as_str),
            Some("runtime-test-goal")
        );
        assert!(!ctx.execution_plan.steps.is_empty());
    }

    #[tokio::test]
    async fn load_task_runtime_context_errors_when_workspace_root_is_missing() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        std::fs::remove_dir_all(state.app_root.join("workspace/default"))
            .expect("remove workspace root");

        let err = load_task_runtime_context(&state, &task_id)
            .await
            .expect_err("missing root must fail");
        assert!(err.to_string().contains("workspace root does not exist"));
    }

    #[tokio::test]
    async fn load_task_runtime_context_validates_probe_profile() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        {
            let mut active = state.active_config.write().expect("lock active config");
            let workflow_id = active.default_workflow_id.clone();
            active
                .config
                .workflows
                .get_mut(&workflow_id)
                .expect("default workflow")
                .safety
                .profile = WorkflowSafetyProfile::SelfReferentialProbe;
        }

        let err = load_task_runtime_context(&state, &task_id)
            .await
            .expect_err("probe profile should fail for non-self-referential workspace");
        assert!(err.to_string().contains("not self_referential"));
    }

    #[tokio::test]
    async fn spawn_task_runner_returns_early_for_duplicate_task() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        {
            let mut running = state.running.lock().await;
            running.insert(task_id.clone(), RunningTask::new());
        }

        spawn_task_runner(state.clone(), task_id.clone())
            .await
            .expect("duplicate spawn should return ok");

        let running = state.running.lock().await;
        assert_eq!(running.len(), 1);
        assert!(running.contains_key(&task_id));
    }

    #[tokio::test]
    async fn stop_task_runtime_marks_task_and_stop_flag() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let runtime = RunningTask::new();

        {
            let mut running = state.running.lock().await;
            running.insert(task_id.clone(), runtime.clone());
        }

        stop_task_runtime(state.clone(), &task_id, "paused")
            .await
            .expect("stop runtime");

        assert!(runtime.stop_flag.load(Ordering::SeqCst));
        assert_eq!(
            state
                .task_repo
                .load_task_status(&task_id)
                .await
                .expect("load task status"),
            Some("paused".to_string())
        );
    }

    #[tokio::test]
    async fn stop_task_runtime_for_delete_removes_running_handle() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        {
            let mut running = state.running.lock().await;
            running.insert(task_id.clone(), RunningTask::new());
        }

        stop_task_runtime_for_delete(state.clone(), &task_id)
            .await
            .expect("stop for delete");

        let running = state.running.lock().await;
        assert!(!running.contains_key(&task_id));
    }

    #[tokio::test]
    async fn shutdown_running_tasks_pauses_and_clears_runtime_map() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        {
            let mut running = state.running.lock().await;
            running.insert(task_id.clone(), RunningTask::new());
        }

        shutdown_running_tasks(state.clone()).await;

        let running = state.running.lock().await;
        assert!(running.is_empty());

        assert_eq!(
            state
                .task_repo
                .load_task_status(&task_id)
                .await
                .expect("load task status"),
            Some("paused".to_string())
        );
    }

    #[tokio::test]
    async fn shutdown_running_tasks_is_noop_when_empty() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        shutdown_running_tasks(state.clone()).await;
        let running = state.running.lock().await;
        assert!(running.is_empty());
    }

    #[tokio::test]
    async fn load_task_runtime_context_renormalizes_stale_self_test_steps() {
        use crate::config::ExecutionMode;

        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let plan_json = {
            let active = read_active_config(&state).expect("read active config");
            let workflow = active
                .config
                .workflows
                .get(&active.default_workflow_id)
                .expect("default workflow");
            let mut plan =
                build_execution_plan(&active.config, workflow, &active.default_workflow_id)
                    .expect("build execution plan");

            let stale_step = plan.steps.first_mut().expect("plan has step");
            stale_step.id = "self_test".to_string();
            stale_step.builtin = Some("self_test".to_string());
            stale_step.required_capability = Some("self_test".to_string());
            stale_step.behavior.execution = ExecutionMode::Agent;
            serde_json::to_string(&plan).expect("serialize plan")
        };

        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET execution_plan_json = ?2 WHERE id = ?1",
            params![task_id.clone(), plan_json],
        )
        .expect("update task");

        let ctx = load_task_runtime_context(&state, &task_id)
            .await
            .expect("load runtime context");
        let loaded_step = ctx
            .execution_plan
            .step_by_id("self_test")
            .expect("self_test step present");

        assert_eq!(
            loaded_step.behavior.execution,
            ExecutionMode::Builtin {
                name: "self_test".to_string()
            },
            "load_task_runtime_context must heal stale stored execution mode"
        );
        assert!(
            loaded_step.required_capability.is_none(),
            "load_task_runtime_context must clear stale required_capability for builtin steps"
        );
        assert_eq!(
            loaded_step.effective_execution_mode().as_ref(),
            &ExecutionMode::Builtin {
                name: "self_test".to_string()
            },
            "the loaded step must dispatch as builtin through the real runtime path"
        );
    }

    /// Helper: rebuild a state arc with `unsafe_mode = true`, reusing the same DB/config.
    fn state_with_unsafe_mode(base: &Arc<InnerState>) -> Arc<InnerState> {
        Arc::new(InnerState {
            app_root: base.app_root.clone(),
            db_path: base.db_path.clone(),
            unsafe_mode: true,
            async_database: base.async_database.clone(),
            logs_dir: base.logs_dir.clone(),
            active_config: std::sync::RwLock::new(
                base.active_config.read().expect("lock active config").clone(),
            ),
            active_config_error: std::sync::RwLock::new(None),
            active_config_notice: std::sync::RwLock::new(None),
            running: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            agent_health: std::sync::RwLock::new(std::collections::HashMap::new()),
            agent_metrics: std::sync::RwLock::new(std::collections::HashMap::new()),
            message_bus: base.message_bus.clone(),
            event_sink: std::sync::RwLock::new(
                base.event_sink.read().expect("lock event_sink").clone(),
            ),
            db_writer: base.db_writer.clone(),
            session_store: base.session_store.clone(),
            task_repo: base.task_repo.clone(),
        })
    }

    #[tokio::test]
    async fn load_task_runtime_context_skips_safety_check_when_unsafe_mode() {
        let mut fixture = TestState::new();
        let (base_state, task_id) = seed_task(&mut fixture);

        // Set probe profile (which would normally fail for non-self-referential workspace)
        {
            let mut active = base_state
                .active_config
                .write()
                .expect("lock active config");
            let workflow_id = active.default_workflow_id.clone();
            active
                .config
                .workflows
                .get_mut(&workflow_id)
                .expect("default workflow")
                .safety
                .profile = WorkflowSafetyProfile::SelfReferentialProbe;
        }

        let unsafe_state = state_with_unsafe_mode(&base_state);

        // With unsafe_mode, the self-referential safety check is skipped — no error expected.
        load_task_runtime_context(&unsafe_state, &task_id)
            .await
            .expect("unsafe_mode should skip self-referential safety validation");
    }
}
