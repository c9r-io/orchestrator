use agent_orchestrator::config::{TaskExecutionPlan, TaskRuntimeContext};
use agent_orchestrator::config_load::{
    build_execution_plan_for_project, read_active_config, resolve_workspace_path,
};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::self_referential_policy::{
    evaluate_self_referential_policy, format_blocking_policy_error,
};
use agent_orchestrator::state::{InnerState, task_semaphore};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tracing::{Instrument, error, info_span};

use super::task_state::set_task_status;
use super::{RunningTask, run_task_loop};
use agent_orchestrator::runner::kill_child_process_group;

/// Kills the currently tracked child process for a running task, if present.
pub async fn kill_current_child(runtime: &RunningTask) {
    let mut child_lock = runtime.child.lock().await;
    if let Some(ref mut child) = *child_lock {
        kill_child_process_group(child).await;
    }
    *child_lock = None;
}

/// Registers and spawns the async runner loop for a task.
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

/// Inserts a task into the in-memory running-task registry.
pub async fn register_running_task(
    state: &InnerState,
    task_id: &str,
    runtime: RunningTask,
) -> bool {
    let mut running = state.running.lock().await;
    if running.contains_key(task_id) {
        return false;
    }
    running.insert(task_id.to_string(), runtime);
    state.daemon_runtime.running_task_started();
    true
}

/// Removes a task from the in-memory running-task registry.
pub async fn unregister_running_task(state: &InnerState, task_id: &str) {
    let mut running = state.running.lock().await;
    if running.remove(task_id).is_some() {
        state.daemon_runtime.running_task_finished();
    }
}

/// Stops a running task, falling back to DB-based child lookup when needed.
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

/// Stops a task in preparation for deleting its records.
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

/// Stops all running tasks during daemon shutdown.
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
        if running.remove(&task_id).is_some() {
            state.daemon_runtime.running_task_finished();
        }
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

/// Loads the runtime context required to resume or continue task execution.
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

    let (workflow, effective_project_id, self_referential, mut execution_plan) = {
        let active = read_active_config(state)?;
        let effective_project_id = active
            .config
            .effective_project_id(Some(project_id.as_str()))
            .to_string();
        let workflow = active
            .config
            .projects
            .get(&effective_project_id)
            .and_then(|p| p.workflows.get(&workflow_id))
            .with_context(|| {
                format!(
                    "workflow not found: {} in project '{}' for task {}",
                    workflow_id, effective_project_id, task_id
                )
            })?
            .clone();
        let self_referential = active
            .config
            .projects
            .get(&effective_project_id)
            .and_then(|p| p.workspaces.get(&workspace_id))
            .map(|ws| ws.self_referential)
            .unwrap_or(false);
        let execution_plan = serde_json::from_str::<TaskExecutionPlan>(&execution_plan_json)
            .ok()
            .filter(|plan| !plan.steps.is_empty())
            .unwrap_or_else(|| {
                build_execution_plan_for_project(
                    &active.config,
                    &workflow,
                    &workflow_id,
                    &effective_project_id,
                )
                .unwrap_or(TaskExecutionPlan {
                    steps: Vec::new(),
                    loop_policy: agent_orchestrator::config::WorkflowLoopConfig::default(),
                    finalize: agent_orchestrator::config::default_workflow_finalize_config(),
                    max_parallel: None,
                    stagger_delay_ms: None,
                    item_isolation: None,
                })
            });
        (
            workflow,
            effective_project_id,
            self_referential,
            execution_plan,
        )
    };
    // Layer 1 defense: re-normalize builtin steps whose `behavior.execution`
    // may have been stored as the serde default `Agent` in SQLite.
    for step in &mut execution_plan.steps {
        step.renormalize_execution_mode();
    }
    if execution_plan.finalize.rules.is_empty() {
        execution_plan.finalize = agent_orchestrator::config::default_workflow_finalize_config();
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
    let adaptive = workflow.adaptive.clone();
    let execution = workflow.execution.clone();
    let safety = workflow.safety.clone();

    if !state.unsafe_mode {
        let policy = evaluate_self_referential_policy(
            &workflow,
            &workflow_id,
            &workspace_id,
            self_referential,
        )?;
        if self_referential
            || workflow.safety.profile
                == agent_orchestrator::config::WorkflowSafetyProfile::SelfReferentialProbe
        {
            insert_event(
                state,
                task_id,
                None,
                "self_referential_policy_checked",
                json!({
                    "workspace_id": workspace_id,
                    "workflow_id": workflow_id,
                    "profile": match workflow.safety.profile {
                        agent_orchestrator::config::WorkflowSafetyProfile::Standard => "standard",
                        agent_orchestrator::config::WorkflowSafetyProfile::SelfReferentialProbe => "self_referential_probe",
                    },
                    "workspace_self_referential": self_referential,
                    "blocking": policy.has_blocking_errors(),
                    "diagnostics": policy.diagnostics,
                }),
            )
            .await?;
        }
        if policy.has_blocking_errors() {
            anyhow::bail!(format_blocking_policy_error(&policy));
        }
    }

    Ok(TaskRuntimeContext {
        workspace_id,
        workspace_root,
        ticket_dir,
        execution_plan: Arc::new(execution_plan),
        execution,
        current_cycle: current_cycle.max(0) as u32,
        init_done: init_done == 1,
        dynamic_steps: Arc::new(dynamic_steps),
        adaptive: Arc::new(adaptive),
        pipeline_vars: {
            let mut pv = match runtime_row.pipeline_vars_json.as_deref() {
                Some(json) if !json.is_empty() => {
                    match serde_json::from_str::<agent_orchestrator::config::PipelineVariables>(
                        json,
                    ) {
                        Ok(vars) => vars,
                        Err(e) => {
                            tracing::warn!(
                                "failed to parse pipeline_vars_json, starting fresh: {e}"
                            );
                            agent_orchestrator::config::PipelineVariables::default()
                        }
                    }
                }
                _ => agent_orchestrator::config::PipelineVariables::default(),
            };
            if !task_goal.is_empty() {
                pv.vars.entry("goal".to_string()).or_insert(task_goal);
            }
            pv
        },
        pinned_invariants: std::sync::Arc::new(safety.invariants.clone()),
        safety: Arc::new(safety),
        self_referential,
        consecutive_failures: 0,
        project_id: effective_project_id,
        workflow_id,
        spawn_depth: runtime_row.spawn_depth,
        item_step_failures: std::collections::HashMap::new(),
        item_retry_after: std::collections::HashMap::new(),
        restart_completed_steps: std::collections::HashSet::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::config::WorkflowSafetyProfile;
    use agent_orchestrator::config_load::read_active_config;
    use agent_orchestrator::db::open_conn;
    use agent_orchestrator::dto::CreateTaskPayload;
    use agent_orchestrator::task_ops::create_task_impl;
    use agent_orchestrator::test_utils::TestState;
    use rusqlite::params;

    fn seed_task(fixture: &mut TestState) -> (Arc<InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .data_dir
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

    fn default_workflow(
        active: &agent_orchestrator::config::ActiveConfig,
    ) -> (&str, &agent_orchestrator::config::WorkflowConfig) {
        let project = active
            .projects
            .get(agent_orchestrator::config::DEFAULT_PROJECT_ID)
            .expect("default project");
        if let Some(workflow) = project.workflows.get("basic") {
            return ("basic", workflow);
        }
        let workflow_id = project.workflows.keys().min().expect("default workflow");
        (
            workflow_id.as_str(),
            project.workflows.get(workflow_id).expect("workflow by id"),
        )
    }

    #[tokio::test]
    async fn load_task_runtime_context_normalizes_fields() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let plan_json = {
            let active = read_active_config(&state).expect("read active config");
            let (workflow_id, workflow) = default_workflow(&active);
            let mut plan = build_execution_plan_for_project(
                &active.config,
                workflow,
                workflow_id,
                agent_orchestrator::config::DEFAULT_PROJECT_ID,
            )
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
    async fn load_task_runtime_context_clone_shares_heavy_fields() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        let ctx = load_task_runtime_context(&state, &task_id)
            .await
            .expect("load runtime context");
        let cloned = ctx.clone();

        assert!(Arc::ptr_eq(&ctx.execution_plan, &cloned.execution_plan));
        assert!(Arc::ptr_eq(&ctx.dynamic_steps, &cloned.dynamic_steps));
        assert!(Arc::ptr_eq(&ctx.adaptive, &cloned.adaptive));
        assert!(Arc::ptr_eq(&ctx.safety, &cloned.safety));
        assert!(Arc::ptr_eq(
            &ctx.pinned_invariants,
            &cloned.pinned_invariants
        ));
    }

    #[tokio::test]
    async fn load_task_runtime_context_errors_when_workspace_root_is_missing() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        std::fs::remove_dir_all(state.data_dir.join("workspace/default"))
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

        agent_orchestrator::state::update_config_runtime(&state, |current| {
            let mut next = current.clone();
            let workflow_id = "basic".to_string();
            let workflow = std::sync::Arc::make_mut(&mut next.active_config)
                .config
                .projects
                .get_mut(agent_orchestrator::config::DEFAULT_PROJECT_ID)
                .expect("default project")
                .workflows
                .get_mut(&workflow_id)
                .expect("default workflow");
            workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
            workflow.safety.checkpoint_strategy =
                agent_orchestrator::config::CheckpointStrategy::GitTag;
            workflow.safety.auto_rollback = true;
            (next, ())
        });

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
        use agent_orchestrator::config::ExecutionMode;

        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let plan_json = {
            let active = read_active_config(&state).expect("read active config");
            let (workflow_id, workflow) = default_workflow(&active);
            let mut plan = build_execution_plan_for_project(
                &active.config,
                workflow,
                workflow_id,
                agent_orchestrator::config::DEFAULT_PROJECT_ID,
            )
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
            data_dir: base.data_dir.clone(),
            db_path: base.db_path.clone(),
            unsafe_mode: true,
            async_database: base.async_database.clone(),
            logs_dir: base.logs_dir.clone(),
            config_runtime: arc_swap::ArcSwap::from_pointee(
                agent_orchestrator::state::ConfigRuntimeSnapshot {
                    active_config: agent_orchestrator::config_load::read_loaded_config(base)
                        .expect("read active config"),
                    active_config_error: None,
                    active_config_notice: None,
                },
            ),
            running: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            agent_health: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            agent_metrics: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            agent_lifecycle: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            message_bus: base.message_bus.clone(),
            // FR-016 sync exception: runtime clone of the event-sink boundary.
            event_sink: std::sync::RwLock::new(agent_orchestrator::state::clone_event_sink(base)),
            db_writer: base.db_writer.clone(),
            session_store: base.session_store.clone(),
            task_repo: base.task_repo.clone(),
            store_manager: agent_orchestrator::store::StoreManager::new(
                base.async_database.clone(),
                base.data_dir.clone(),
            ),
            daemon_runtime: agent_orchestrator::runtime::DaemonRuntimeState::new(),
            worker_notify: Arc::new(tokio::sync::Notify::new()),
            trigger_event_tx: tokio::sync::broadcast::channel(64).0,
            trigger_engine_handle: std::sync::Mutex::new(None),
        })
    }

    #[tokio::test]
    async fn load_task_runtime_context_skips_safety_check_when_unsafe_mode() {
        let mut fixture = TestState::new();
        let (base_state, task_id) = seed_task(&mut fixture);

        // Set probe profile (which would normally fail for non-self-referential workspace)
        agent_orchestrator::state::update_config_runtime(&base_state, |current| {
            let mut next = current.clone();
            let workflow_id = "basic".to_string();
            Arc::make_mut(&mut next.active_config)
                .config
                .projects
                .get_mut(agent_orchestrator::config::DEFAULT_PROJECT_ID)
                .expect("default project")
                .workflows
                .get_mut(&workflow_id)
                .expect("default workflow")
                .safety
                .profile = WorkflowSafetyProfile::SelfReferentialProbe;
            (next, ())
        });

        let unsafe_state = state_with_unsafe_mode(&base_state);

        // With unsafe_mode, the self-referential safety check is skipped — no error expected.
        load_task_runtime_context(&unsafe_state, &task_id)
            .await
            .expect("unsafe_mode should skip self-referential safety validation");
    }
}
