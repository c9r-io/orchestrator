use crate::config_load::{
    load_config_overview, now_ts, persist_config_and_reload, read_active_config,
};
use crate::db::open_conn;
use crate::dto::{
    AgentHealthInfo, BootstrapResponse, ConfigOverview, ConfigValidationResult,
    ConfigVersionDetail, ConfigVersionSummary, CreateTaskOptions, CreateTaskDefaults,
    CreateTaskPayload, DeleteTaskResponse, NamedOption, SaveConfigFormPayload,
    SaveConfigYamlPayload, SimulatePrehookPayload, SimulatePrehookResult, TaskDetail,
    TaskSummary, ValidationErrorDto, ValidationWarningDto,
};
use crate::prehook::simulate_prehook_impl;
use crate::scheduler::{
    delete_task_impl, get_task_details_impl, list_tasks_impl, load_task_summary,
    prepare_task_for_start, spawn_task_runner, stop_task_runtime, stop_task_runtime_for_delete,
    stream_task_logs_impl,
};
use crate::state::ManagedState;
use crate::ticket::{
    collect_target_files, collect_target_files_from_active_tickets,
    should_seed_targets_from_active_tickets,
};
use crate::config_load::build_execution_plan;
use crate::config::{WorkflowStepType, LoopMode};
use crate::db;
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::sync::Arc;
use tauri::{AppHandle, State};
use uuid::Uuid;

fn err_to_string(err: impl std::fmt::Display) -> String {
    err.to_string()
}

#[tauri::command]
pub async fn bootstrap(state: State<'_, ManagedState>) -> Result<BootstrapResponse, String> {
    let active = read_active_config(&state.inner).map_err(err_to_string)?;
    if !active.config.resume.auto {
        return Ok(BootstrapResponse {
            resumed_task_id: None,
        });
    }
    let resumed_task_id =
        crate::scheduler::find_latest_resumable_task_id(&state.inner, false).map_err(err_to_string)?;
    Ok(BootstrapResponse { resumed_task_id })
}

#[tauri::command]
pub async fn get_create_task_options(
    state: State<'_, ManagedState>,
) -> Result<CreateTaskOptions, String> {
    let active = read_active_config(&state.inner).map_err(err_to_string)?;

    let mut projects: Vec<NamedOption> = active
        .config
        .projects
        .keys()
        .cloned()
        .map(|id| NamedOption { id })
        .collect();
    projects.sort_by(|a, b| a.id.cmp(&b.id));

    let mut workspaces: Vec<NamedOption> = active
        .config
        .workspaces
        .keys()
        .cloned()
        .map(|id| NamedOption { id })
        .collect();
    if let Some(project_config) = active.config.projects.get(&active.default_project_id) {
        for ws_id in project_config.workspaces.keys() {
            if !workspaces.iter().any(|w| w.id == *ws_id) {
                workspaces.push(NamedOption { id: ws_id.clone() });
            }
        }
    }
    workspaces.sort_by(|a, b| a.id.cmp(&b.id));

    let mut workflows: Vec<NamedOption> = active
        .config
        .workflows
        .keys()
        .cloned()
        .map(|id| NamedOption { id })
        .collect();
    if let Some(project_config) = active.config.projects.get(&active.default_project_id) {
        for wf_id in project_config.workflows.keys() {
            if !workflows.iter().any(|w| w.id == *wf_id) {
                workflows.push(NamedOption { id: wf_id.clone() });
            }
        }
    }
    workflows.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(CreateTaskOptions {
        defaults: CreateTaskDefaults {
            project_id: active.default_project_id.clone(),
            workspace_id: active.default_workspace_id.clone(),
            workflow_id: active.default_workflow_id.clone(),
        },
        projects,
        workspaces,
        workflows,
    })
}

#[tauri::command]
pub async fn get_config_overview(state: State<'_, ManagedState>) -> Result<ConfigOverview, String> {
    load_config_overview(&state.inner).map_err(err_to_string)
}

#[tauri::command]
pub async fn save_config_from_form(
    state: State<'_, ManagedState>,
    payload: SaveConfigFormPayload,
) -> Result<ConfigOverview, String> {
    let yaml = serde_yaml::to_string(&payload.config).map_err(err_to_string)?;
    persist_config_and_reload(&state.inner, payload.config, yaml, "ui-form").map_err(err_to_string)
}

#[tauri::command]
pub async fn save_config_from_yaml(
    state: State<'_, ManagedState>,
    payload: SaveConfigYamlPayload,
) -> Result<ConfigOverview, String> {
    let config =
        serde_yaml::from_str::<crate::config::OrchestratorConfig>(&payload.yaml).map_err(err_to_string)?;
    persist_config_and_reload(&state.inner, config, payload.yaml, "ui-yaml").map_err(err_to_string)
}

#[tauri::command]
pub async fn validate_config_yaml(
    state: State<'_, ManagedState>,
    payload: SaveConfigYamlPayload,
) -> Result<ConfigValidationResult, String> {
    use crate::config_validation::validator::ConfigValidator;
    use crate::config_validation::{ValidationLevel, PathValidationOptions};

    let validator = ConfigValidator::new(&state.inner.app_root)
        .with_level(ValidationLevel::Full)
        .with_path_options(PathValidationOptions {
            missing_path_is_error: false,
            check_path_escape: true,
        });

    let result = validator.validate_yaml(&payload.yaml);

    if result.errors.is_empty() {
        let config = serde_yaml::from_str::<crate::config::OrchestratorConfig>(&payload.yaml)
            .map_err(err_to_string)?;
        let candidate = crate::config_load::build_active_config(&state.inner.app_root, config)
            .map_err(err_to_string)?;
        let current = read_active_config(&state.inner)
            .map_err(err_to_string)?
            .config
            .clone();
        let conn = open_conn(&state.inner.db_path).map_err(err_to_string)?;
        crate::config_load::enforce_deletion_guards(&conn, &current, &candidate.config)
            .map_err(err_to_string)?;
        let normalized_yaml = serde_yaml::to_string(&candidate.config).map_err(err_to_string)?;

        let errors: Vec<_> = result.errors.iter().map(|e| ValidationErrorDto {
            code: format!("{:?}", e.code),
            message: e.message.clone(),
            field: e.field.clone(),
            context: e.context.clone(),
        }).collect();

        let warnings: Vec<_> = result.warnings.iter().map(|w| ValidationWarningDto {
            code: format!("{:?}", w.code),
            message: w.message.clone(),
            field: w.field.clone(),
            suggestion: w.suggestion.clone(),
        }).collect();

        Ok(ConfigValidationResult {
            valid: result.is_valid,
            normalized_yaml,
            errors,
            warnings,
            summary: result.report(),
        })
    } else {
        let errors: Vec<_> = result.errors.iter().map(|e| ValidationErrorDto {
            code: format!("{:?}", e.code),
            message: e.message.clone(),
            field: e.field.clone(),
            context: e.context.clone(),
        }).collect();

        let warnings: Vec<_> = result.warnings.iter().map(|w| ValidationWarningDto {
            code: format!("{:?}", w.code),
            message: w.message.clone(),
            field: w.field.clone(),
            suggestion: w.suggestion.clone(),
        }).collect();

        Ok(ConfigValidationResult {
            valid: result.is_valid,
            normalized_yaml: String::new(),
            errors,
            warnings,
            summary: result.report(),
        })
    }
}

#[tauri::command]
pub async fn list_config_versions(
    state: State<'_, ManagedState>,
) -> Result<Vec<ConfigVersionSummary>, String> {
    let conn = open_conn(&state.inner.db_path).map_err(err_to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT version, created_at, author FROM orchestrator_config_versions ORDER BY version DESC LIMIT 200",
        )
        .map_err(err_to_string)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ConfigVersionSummary {
                version: row.get(0)?,
                created_at: row.get(1)?,
                author: row.get(2)?,
            })
        })
        .map_err(err_to_string)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(err_to_string)?;
    Ok(rows)
}

#[tauri::command]
pub async fn get_config_version(
    state: State<'_, ManagedState>,
    version: i64,
) -> Result<ConfigVersionDetail, String> {
    let conn = open_conn(&state.inner.db_path).map_err(err_to_string)?;
    let detail = conn
        .query_row(
            "SELECT version, created_at, author, config_yaml FROM orchestrator_config_versions WHERE version = ?1",
            params![version],
            |row| {
                Ok(ConfigVersionDetail {
                    version: row.get(0)?,
                    created_at: row.get(1)?,
                    author: row.get(2)?,
                    yaml: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(err_to_string)?;
    detail.ok_or_else(|| format!("config version not found: {}", version))
}

#[tauri::command]
pub async fn create_task(
    state: State<'_, ManagedState>,
    payload: CreateTaskPayload,
) -> Result<TaskSummary, String> {
    create_task_impl(&state.inner, payload).map_err(err_to_string)
}

pub fn create_task_impl(state: &crate::state::InnerState, payload: CreateTaskPayload) -> Result<TaskSummary> {
    let active = read_active_config(state)?;

    let project_id = payload
        .project_id
        .clone()
        .unwrap_or_else(|| active.default_project_id.clone());

    let workspace_id = payload
        .workspace_id
        .clone()
        .unwrap_or_else(|| active.default_workspace_id.clone());

    let workspace = if let Some(project) = active.projects.get(&project_id) {
        project.workspaces.get(&workspace_id).cloned()
    } else {
        active.workspaces.get(&workspace_id).cloned()
    }
    .with_context(|| {
        format!(
            "workspace not found: {} in project: {}",
            workspace_id, project_id
        )
    })?;

    let workflow_id = payload
        .workflow_id
        .clone()
        .unwrap_or_else(|| active.default_workflow_id.clone());

    let workflow = if let Some(project) = active.projects.get(&project_id) {
        project.workflows.get(&workflow_id).cloned()
    } else {
        active.config.workflows.get(&workflow_id).cloned()
    }
    .with_context(|| {
        format!(
            "workflow not found: {} in project: {}",
            workflow_id, project_id
        )
    })?;

    let execution_plan = build_execution_plan(&active.config, &workflow, &workflow_id)?;
    let execution_plan_json =
        serde_json::to_string(&execution_plan).context("serialize execution plan")?;
    let loop_mode = match execution_plan.loop_policy.mode {
        LoopMode::Once => "once",
        LoopMode::Infinite => "infinite",
    };

    let target_files_input = payload.target_files.clone();
    let seed_from_tickets =
        should_seed_targets_from_active_tickets(target_files_input.as_ref(), &execution_plan);
    let mut target_files = if seed_from_tickets {
        collect_target_files_from_active_tickets(&workspace.root_path, &workspace.ticket_dir)?
    } else {
        collect_target_files(
            &workspace.root_path,
            &workspace.qa_targets,
            target_files_input,
        )?
    };
    if target_files.is_empty() {
        if seed_from_tickets {
            target_files.push(crate::dto::UNASSIGNED_QA_FILE_PATH.to_string());
        } else {
            anyhow::bail!("No QA/Security markdown files found");
        }
    }

    let task_id = Uuid::new_v4().to_string();
    let created_at = now_ts();
    let task_name = payload
        .name
        .unwrap_or_else(|| format!("QA Sprint {}", Utc::now().format("%Y-%m-%d %H:%M:%S")));
    let goal = payload
        .goal
        .unwrap_or_else(|| "Automated QA workflow with fix and resume".to_string());

    let conn = open_conn(&state.db_path)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO tasks (id, name, status, started_at, completed_at, goal, target_files_json, mode, project_id, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, resume_token, created_at, updated_at) VALUES (?1, ?2, 'pending', NULL, NULL, ?3, ?4, '', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 0, NULL, ?13, ?13)",
        params![
            task_id,
            task_name,
            goal,
            serde_json::to_string(&target_files)?,
            project_id,
            workspace_id,
            workflow_id,
            workspace.root_path.to_string_lossy().to_string(),
            serde_json::to_string(&workspace.qa_targets)?,
            workspace.ticket_dir,
            execution_plan_json,
            loop_mode,
            created_at
        ],
    )?;

    for (idx, path) in target_files.iter().enumerate() {
        let item_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', '[]', 0, 0, '', NULL, NULL, ?5, ?5)",
            params![item_id, task_id, (idx as i64) + 1, path, created_at],
        )?;
    }
    tx.commit()?;

    load_task_summary(state, &task_id)
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, ManagedState>) -> Result<Vec<TaskSummary>, String> {
    list_tasks_impl(&state.inner).map_err(err_to_string)
}

#[tauri::command]
pub async fn get_task_details(
    state: State<'_, ManagedState>,
    task_id: String,
) -> Result<TaskDetail, String> {
    get_task_details_impl(&state.inner, &task_id).map_err(err_to_string)
}

#[tauri::command]
pub async fn start_task(
    state: State<'_, ManagedState>,
    app: AppHandle,
    task_id: String,
) -> Result<TaskSummary, String> {
    prepare_task_for_start(&state.inner, &task_id).map_err(err_to_string)?;
    spawn_task_runner(state.inner.clone(), app, task_id.clone())
        .await
        .map_err(err_to_string)?;
    load_task_summary(&state.inner, &task_id).map_err(err_to_string)
}

#[tauri::command]
pub async fn pause_task(
    state: State<'_, ManagedState>,
    task_id: String,
) -> Result<TaskSummary, String> {
    stop_task_runtime(state.inner.clone(), &task_id, "paused")
        .await
        .map_err(err_to_string)?;
    load_task_summary(&state.inner, &task_id).map_err(err_to_string)
}

#[tauri::command]
pub async fn resume_task(
    state: State<'_, ManagedState>,
    app: AppHandle,
    task_id: String,
) -> Result<TaskSummary, String> {
    prepare_task_for_start(&state.inner, &task_id).map_err(err_to_string)?;
    spawn_task_runner(state.inner.clone(), app, task_id.clone())
        .await
        .map_err(err_to_string)?;
    load_task_summary(&state.inner, &task_id).map_err(err_to_string)
}

#[tauri::command]
pub async fn retry_task_item(
    state: State<'_, ManagedState>,
    app: AppHandle,
    task_item_id: String,
) -> Result<TaskSummary, String> {
    let task_id = reset_task_item_for_retry(&state.inner, &task_item_id).map_err(err_to_string)?;
    prepare_task_for_start(&state.inner, &task_id).map_err(err_to_string)?;
    spawn_task_runner(state.inner.clone(), app, task_id.clone())
        .await
        .map_err(err_to_string)?;
    load_task_summary(&state.inner, &task_id).map_err(err_to_string)
}

pub fn reset_task_item_for_retry(state: &crate::state::InnerState, task_item_id: &str) -> Result<String> {
    let conn = open_conn(&state.db_path)?;
    let task_id: String = conn.query_row(
        "SELECT task_id FROM task_items WHERE id = ?1",
        params![task_item_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE task_items SET status = 'pending', ticket_files_json = '[]', ticket_content_json = '[]', fix_required = 0, fixed = 0, last_error = '', started_at = NULL, completed_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![task_item_id, now_ts()],
    )?;
    Ok(task_id)
}

#[tauri::command]
pub async fn delete_task(
    state: State<'_, ManagedState>,
    app: AppHandle,
    task_id: String,
) -> Result<DeleteTaskResponse, String> {
    println!("[agent-orchestrator][delete] begin task_id={}", task_id);
    stop_task_runtime_for_delete(state.inner.clone(), &task_id)
        .await
        .map_err(err_to_string)?;
    println!(
        "[agent-orchestrator][delete] runtime detached/stopped task_id={}",
        task_id
    );
    delete_task_impl(&state.inner, &task_id).map_err(err_to_string)?;
    println!(
        "[agent-orchestrator][delete] db records removed task_id={}",
        task_id
    );
    crate::events::emit_event(
        &app,
        &task_id,
        None,
        "task_deleted",
        serde_json::json!({ "task_id": task_id }),
    );
    println!(
        "[agent-orchestrator][delete] emitted task_deleted task_id={}",
        task_id
    );
    Ok(DeleteTaskResponse {
        task_id,
        deleted: true,
    })
}

#[tauri::command]
pub async fn stream_task_logs(
    state: State<'_, ManagedState>,
    task_id: String,
    limit: Option<usize>,
) -> Result<Vec<crate::dto::LogChunk>, String> {
    stream_task_logs_impl(&state.inner, &task_id, limit.unwrap_or(300)).map_err(err_to_string)
}

#[tauri::command]
pub async fn simulate_prehook(
    payload: SimulatePrehookPayload,
) -> Result<SimulatePrehookResult, String> {
    simulate_prehook_impl(payload).map_err(err_to_string)
}

#[tauri::command]
pub async fn get_agent_health(state: State<'_, ManagedState>) -> Result<Vec<AgentHealthInfo>, String> {
    let health = state.inner.agent_health.read().map_err(|e| e.to_string())?;
    let now = Utc::now();
    let mut result = Vec::new();
    let active = read_active_config(&state.inner).map_err(err_to_string)?;
    for agent_id in active.config.agents.keys() {
        let (healthy, diseased_until, consecutive_errors) = match health.get(agent_id) {
            None => (true, None, 0),
            Some(state) => {
                let is_healthy = match state.diseased_until {
                    None => true,
                    Some(until) => now >= until,
                };
                (
                    is_healthy,
                    state.diseased_until.map(|t| t.to_rfc3339()),
                    state.consecutive_errors,
                )
            }
        };
        result.push(AgentHealthInfo {
            agent_id: agent_id.clone(),
            healthy,
            diseased_until,
            consecutive_errors,
        });
    }
    Ok(result)
}
