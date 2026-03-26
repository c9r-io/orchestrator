use crate::config::LoopMode;
use crate::config_load::build_execution_plan_for_project;
use crate::config_load::{now_ts, read_active_config};
use crate::db::open_conn;
use crate::dto::{CreateTaskPayload, TaskSummary, UNASSIGNED_QA_FILE_PATH};
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use crate::ticket::{collect_target_files, collect_target_files_from_active_tickets};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetSeedStrategy {
    Explicit,
    ActiveTickets,
    QaDirectoryScan,
    SyntheticAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedTaskTargets {
    persisted_target_files: Vec<String>,
    task_item_paths: Vec<String>,
}

fn execution_plan_requires_item_targets(plan: &crate::config::TaskExecutionPlan) -> bool {
    plan.steps
        .iter()
        .any(|step| step.enabled && step.resolved_scope() == crate::config::StepScope::Item)
}

fn select_target_seed_strategy(
    explicit_targets: Option<&Vec<String>>,
    plan: &crate::config::TaskExecutionPlan,
) -> TargetSeedStrategy {
    if explicit_targets.is_some() {
        TargetSeedStrategy::Explicit
    } else if !execution_plan_requires_item_targets(plan) {
        TargetSeedStrategy::SyntheticAnchor
    } else if plan.step_by_id("qa").is_none() && plan.step_by_id("ticket_scan").is_some() {
        TargetSeedStrategy::ActiveTickets
    } else {
        TargetSeedStrategy::QaDirectoryScan
    }
}

fn resolve_task_targets(
    workspace: &crate::config::ResolvedWorkspace,
    plan: &crate::config::TaskExecutionPlan,
    explicit_targets: Option<Vec<String>>,
) -> Result<ResolvedTaskTargets> {
    let requires_item_targets = execution_plan_requires_item_targets(plan);
    match select_target_seed_strategy(explicit_targets.as_ref(), plan) {
        TargetSeedStrategy::Explicit => {
            let validated = collect_target_files(
                &workspace.root_path,
                &workspace.qa_targets,
                explicit_targets,
            )?;
            if requires_item_targets {
                if validated.is_empty() {
                    anyhow::bail!("no valid --target-file entries found");
                }
                Ok(ResolvedTaskTargets {
                    persisted_target_files: validated.clone(),
                    task_item_paths: validated,
                })
            } else {
                match validated.len() {
                    0 => anyhow::bail!("no valid --target-file entries found"),
                    1 => Ok(ResolvedTaskTargets {
                        persisted_target_files: validated.clone(),
                        task_item_paths: validated,
                    }),
                    _ => anyhow::bail!("task-scoped workflow accepts at most one --target-file"),
                }
            }
        }
        TargetSeedStrategy::ActiveTickets => {
            let mut targets = collect_target_files_from_active_tickets(
                &workspace.root_path,
                &workspace.ticket_dir,
            )?;
            if targets.is_empty() {
                targets.push(UNASSIGNED_QA_FILE_PATH.to_string());
            }
            Ok(ResolvedTaskTargets {
                persisted_target_files: targets.clone(),
                task_item_paths: targets,
            })
        }
        TargetSeedStrategy::QaDirectoryScan => {
            let targets = collect_target_files(&workspace.root_path, &workspace.qa_targets, None)?;
            if targets.is_empty() {
                anyhow::bail!("No QA/Security markdown files found for item-scoped workflow");
            }
            Ok(ResolvedTaskTargets {
                persisted_target_files: targets.clone(),
                task_item_paths: targets,
            })
        }
        TargetSeedStrategy::SyntheticAnchor => Ok(ResolvedTaskTargets {
            persisted_target_files: Vec::new(),
            task_item_paths: vec![UNASSIGNED_QA_FILE_PATH.to_string()],
        }),
    }
}

/// Creates a task, its execution plan snapshot, and initial task items.
pub fn create_task_impl(
    state: &crate::state::InnerState,
    payload: CreateTaskPayload,
) -> Result<TaskSummary> {
    let active = read_active_config(state)?;

    let project_id = payload
        .project_id
        .clone()
        .unwrap_or_else(|| crate::config::DEFAULT_PROJECT_ID.to_string());
    let project = active
        .projects
        .get(&project_id)
        .with_context(|| format!("project not found: {}", project_id))?;

    let workspace_id = if let Some(workspace_id) = payload.workspace_id.clone() {
        workspace_id
    } else {
        resolve_default_resource_id(&project.workspaces, "workspace")?
    };
    let workspace = project
        .workspaces
        .get(&workspace_id)
        .cloned()
        .with_context(|| {
            format!(
                "workspace not found: {} in project '{}'",
                workspace_id, project_id
            )
        })?;

    let workflow_id = if let Some(workflow_id) = payload.workflow_id.clone() {
        workflow_id
    } else {
        resolve_default_resource_id(&project.workflows, "workflow")?
    };
    let workflow = project
        .workflows
        .get(&workflow_id)
        .cloned()
        .with_context(|| {
            format!(
                "workflow not found: {} in project '{}'",
                workflow_id, project_id
            )
        })?;

    let execution_plan =
        build_execution_plan_for_project(&active.config, &workflow, &workflow_id, &project_id)?;
    let execution_plan_json =
        serde_json::to_string(&execution_plan).context("serialize execution plan")?;
    let loop_mode = match execution_plan.loop_policy.mode {
        LoopMode::Once => "once",
        LoopMode::Fixed => "fixed",
        LoopMode::Infinite => "infinite",
    };

    let resolved_targets = resolve_task_targets(&workspace, &execution_plan, payload.target_files)?;

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
        "INSERT INTO tasks (id, name, status, started_at, completed_at, goal, target_files_json, mode, project_id, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, resume_token, created_at, updated_at, parent_task_id, spawn_reason, spawn_depth) VALUES (?1, ?2, 'created', NULL, NULL, ?3, ?4, '', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 0, NULL, ?13, ?13, ?14, ?15, 0)",
        params![
            task_id,
            task_name,
            goal,
            serde_json::to_string(&resolved_targets.persisted_target_files)?,
            project_id,
            workspace_id,
            workflow_id,
            workspace.root_path.to_string_lossy().to_string(),
            serde_json::to_string(&workspace.qa_targets)?,
            workspace.ticket_dir,
            execution_plan_json,
            loop_mode,
            created_at,
            payload.parent_task_id,
            payload.spawn_reason,
        ],
    )?;

    for (idx, path) in resolved_targets.task_item_paths.iter().enumerate() {
        let item_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', '[]', 0, 0, '', NULL, NULL, ?5, ?5)",
            params![item_id, task_id, (idx as i64) + 1, path, created_at],
        )?;
    }
    tx.commit()?;

    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let mut summary = repo.load_task_summary(&task_id)?;
    let (total, finished, failed) = repo.load_task_item_counts(&task_id)?;
    summary.total_items = total;
    summary.finished_items = finished;
    summary.failed_items = failed;
    Ok(summary)
}

fn resolve_default_resource_id<T>(
    entries: &std::collections::HashMap<String, T>,
    resource_kind: &str,
) -> Result<String> {
    if entries.is_empty() {
        anyhow::bail!("project has no {}s configured", resource_kind);
    }
    if entries.len() == 1 {
        return Ok(entries.keys().next().cloned().unwrap_or_default());
    }
    if entries.contains_key("default") {
        return Ok("default".to_string());
    }
    anyhow::bail!(
        "multiple {}s exist in project; specify --{} explicitly",
        resource_kind,
        resource_kind
    )
}

/// Resets one failed task item back to the pending state and returns its parent task id.
///
/// Accepts an exact task-item ID or a unique prefix (same behaviour as
/// `resolve_task_id` for tasks).
pub fn reset_task_item_for_retry(
    state: &crate::state::InnerState,
    task_item_id: &str,
) -> Result<String> {
    let conn = open_conn(&state.db_path)?;
    let resolved_id = resolve_task_item_id(&conn, task_item_id)?;
    let task_id: String = conn.query_row(
        "SELECT task_id FROM task_items WHERE id = ?1",
        params![resolved_id],
        |row| row.get(0),
    )?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE task_items SET status = 'pending', ticket_files_json = '[]', ticket_content_json = '[]', fix_required = 0, fixed = 0, last_error = '', started_at = NULL, completed_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![resolved_id, now_ts()],
    )?;
    // Clear old command runs so compensation doesn't re-finalize with stale results
    tx.execute(
        "DELETE FROM command_runs WHERE task_item_id = ?1",
        params![resolved_id],
    )?;
    tx.commit()?;
    Ok(task_id)
}

/// Resolve a task-item ID from an exact match or unique prefix.
fn resolve_task_item_id(conn: &rusqlite::Connection, id_or_prefix: &str) -> Result<String> {
    use rusqlite::OptionalExtension;
    let exact: Option<String> = conn
        .query_row(
            "SELECT id FROM task_items WHERE id = ?1",
            params![id_or_prefix],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = exact {
        return Ok(id);
    }
    let pattern = format!("{}%", id_or_prefix);
    let mut stmt = conn.prepare("SELECT id FROM task_items WHERE id LIKE ?1")?;
    let matches: Vec<String> = stmt
        .query_map(params![pattern], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    match matches.len() {
        1 => Ok(matches
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("unexpected empty matches"))?),
        0 => anyhow::bail!("task item not found: {}", id_or_prefix),
        _ => anyhow::bail!(
            "multiple task items match prefix '{}': {:?}",
            id_or_prefix,
            matches
        ),
    }
}

/// Service-layer wrapper around [`create_task_impl`] with error classification.
///
/// This exists so that core modules (trigger_engine, service/resource) can
/// create tasks without depending on the `orchestrator-scheduler` service layer.
pub fn create_task_as_service(
    state: &crate::state::InnerState,
    payload: CreateTaskPayload,
) -> crate::error::Result<TaskSummary> {
    create_task_impl(state, payload)
        .map_err(|err| crate::error::classify_task_error("task.create", err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        LoopMode, ProjectConfig, ResolvedProject, SafetyConfig, StepBehavior, WorkflowConfig,
        WorkflowFinalizeConfig, WorkflowLoopConfig, WorkflowLoopGuardConfig, WorkflowStepConfig,
    };
    use crate::dto::CreateTaskPayload;
    use crate::state::update_config_runtime;
    use crate::test_utils::TestState;
    use std::collections::HashMap;

    fn make_workflow(steps: Vec<WorkflowStepConfig>) -> WorkflowConfig {
        WorkflowConfig {
            steps,
            execution: Default::default(),
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    stop_when_no_unresolved: false,
                    max_cycles: None,
                    agent_template: None,
                },
                convergence_expr: None,
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: SafetyConfig::default(),
            max_parallel: None,
            stagger_delay_ms: None,
            item_isolation: None,
        }
    }

    fn make_step(
        id: &str,
        builtin: Option<&str>,
        required_capability: Option<&str>,
    ) -> WorkflowStepConfig {
        WorkflowStepConfig {
            id: id.to_string(),
            description: None,
            builtin: builtin.map(str::to_string),
            required_capability: required_capability.map(str::to_string),
            template: None,
            execution_profile: None,
            enabled: true,
            repeatable: false,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: Vec::new(),
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
            max_parallel: None,
            stagger_delay_ms: None,
            timeout_secs: None,
            stall_timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
            step_vars: None,
        }
    }

    fn task_only_workflow() -> WorkflowConfig {
        make_workflow(vec![make_step("self_test", Some("self_test"), None)])
    }

    fn ticket_seed_workflow() -> WorkflowConfig {
        make_workflow(vec![make_step("ticket_scan", Some("ticket_scan"), None)])
    }

    fn load_task_storage(
        state: &crate::state::InnerState,
        task_id: &str,
    ) -> (Vec<String>, Vec<String>) {
        let conn = open_conn(&state.db_path).expect("open task storage database");
        let target_files_json: String = conn
            .query_row(
                "SELECT target_files_json FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("load serialized target_files");
        let target_files = serde_json::from_str::<Vec<String>>(&target_files_json)
            .expect("deserialize target_files");
        let mut stmt = conn
            .prepare("SELECT qa_file_path FROM task_items WHERE task_id = ?1 ORDER BY order_no")
            .expect("prepare task item query");
        let item_paths = stmt
            .query_map(params![task_id], |row| row.get::<_, String>(0))
            .expect("query task item paths")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect task item paths");
        (target_files, item_paths)
    }

    #[test]
    fn create_task_with_defaults() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Create a QA file so target_files is non-empty
        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("test-qa.md"), "# QA Test\n").expect("write qa file");
        drop(active);

        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(
            result.is_ok(),
            "create_task_impl should succeed: {:?}",
            result.err()
        );
        let summary = result.expect("create_task_impl should produce summary");
        assert_eq!(summary.status, "created");
        assert!(!summary.id.is_empty());
        assert!(summary.name.starts_with("QA Sprint"));
        assert_eq!(summary.goal, "Automated QA workflow with fix and resume");
        assert_eq!(summary.workspace_id, "default");
        assert_eq!(summary.workflow_id, "basic");
        assert!(summary.total_items >= 1);
    }

    #[test]
    fn create_task_with_custom_name_and_goal() {
        let mut ts = TestState::new();
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("custom-qa.md"), "# Custom QA\n")
            .expect("write custom qa file");
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("My Custom Task".to_string()),
            goal: Some("Custom goal description".to_string()),
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload).expect("create custom task");
        assert_eq!(result.name, "My Custom Task");
        assert_eq!(result.goal, "Custom goal description");
    }

    #[test]
    fn create_task_with_nonexistent_workspace_fails() {
        let mut ts = TestState::new();
        let state = ts.build();

        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: Some("nonexistent-ws".to_string()),
            workflow_id: None,
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("workspace not found"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn create_task_with_nonexistent_workflow_fails() {
        let mut ts = TestState::new();
        let state = ts.build();

        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: Some("nonexistent-wf".to_string()),
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("workflow not found"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn create_task_item_scoped_workflow_with_no_qa_files_fails() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Don't create any qa files - the qa_targets dirs exist but are empty
        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("No QA/Security markdown files found for item-scoped workflow"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn create_task_with_explicit_target_files() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Create target files
        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        let file1 = qa_path.join("file1.md");
        let file2 = qa_path.join("file2.md");
        std::fs::write(&file1, "# File 1\n").expect("write file1");
        std::fs::write(&file2, "# File 2\n").expect("write file2");
        let rel1 = format!("{}/file1.md", qa_dir);
        let rel2 = format!("{}/file2.md", qa_dir);
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Targeted".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: Some(vec![rel1, rel2]),
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload).expect("create targeted task");
        assert_eq!(result.total_items, 2, "should have 2 task items");
        let (target_files, item_paths) = load_task_storage(&state, &result.id);
        assert_eq!(target_files.len(), 2);
        assert_eq!(item_paths.len(), 2);
    }

    #[test]
    fn create_task_item_scoped_workflow_with_explicit_non_markdown_target_succeeds() {
        let mut ts = TestState::new();
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let src_path = ws.root_path.join("src");
        std::fs::create_dir_all(&src_path).ok();
        std::fs::write(src_path.join("lib.rs"), "fn main() {}\n").expect("write lib.rs");
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Targeted Source".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: Some(vec!["src/lib.rs".to_string()]),
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload).expect("create source task");
        assert_eq!(result.total_items, 1);
        let (target_files, item_paths) = load_task_storage(&state, &result.id);
        assert_eq!(target_files, vec!["src/lib.rs".to_string()]);
        assert_eq!(item_paths, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn create_task_task_scoped_workflow_without_qa_files_uses_synthetic_anchor() {
        let mut ts = TestState::new().with_workflow("task_only", task_only_workflow());
        let state = ts.build();

        let payload = CreateTaskPayload {
            name: Some("Task Only".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: Some("task_only".to_string()),
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload).expect("create task-scoped task");
        assert_eq!(result.total_items, 1);
        let (target_files, item_paths) = load_task_storage(&state, &result.id);
        assert!(target_files.is_empty());
        assert_eq!(item_paths, vec![UNASSIGNED_QA_FILE_PATH.to_string()]);
    }

    #[test]
    fn create_task_task_scoped_workflow_with_single_explicit_target_succeeds() {
        let mut ts = TestState::new().with_workflow("task_only", task_only_workflow());
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let src_path = ws.root_path.join("src");
        std::fs::create_dir_all(&src_path).ok();
        std::fs::write(src_path.join("lib.rs"), "fn main() {}\n").expect("write lib.rs");
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Task Only Target".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: Some("task_only".to_string()),
            target_files: Some(vec!["src/lib.rs".to_string()]),
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload).expect("create task-only targeted task");
        assert_eq!(result.total_items, 1);
        let (target_files, item_paths) = load_task_storage(&state, &result.id);
        assert_eq!(target_files, vec!["src/lib.rs".to_string()]);
        assert_eq!(item_paths, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn create_task_with_empty_project_rejects_missing_workspace() {
        let mut ts = TestState::new().with_workflow("task_only", task_only_workflow());
        let state = ts.build();

        update_config_runtime(&state, |current| {
            let mut next = current.clone();
            std::sync::Arc::make_mut(&mut next.active_config)
                .config
                .projects
                .insert(
                    "proj-a".to_string(),
                    ProjectConfig {
                        description: None,
                        workspaces: HashMap::new(),
                        agents: HashMap::new(),
                        workflows: HashMap::new(),
                        step_templates: HashMap::new(),
                        env_stores: HashMap::new(),
                        execution_profiles: HashMap::new(),
                        triggers: HashMap::new(),
                    },
                );
            std::sync::Arc::make_mut(&mut next.active_config)
                .projects
                .insert(
                    "proj-a".to_string(),
                    ResolvedProject {
                        workspaces: HashMap::new(),
                        agents: HashMap::new(),
                        workflows: HashMap::new(),
                        step_templates: HashMap::new(),
                        env_stores: HashMap::new(),
                        execution_profiles: HashMap::new(),
                    },
                );
            (next, ())
        });

        let payload = CreateTaskPayload {
            name: Some("Project Strict".to_string()),
            goal: None,
            project_id: Some("proj-a".to_string()),
            workspace_id: Some("default".to_string()),
            workflow_id: Some("task_only".to_string()),
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let err = create_task_impl(&state, payload).unwrap_err();
        assert!(
            err.to_string().contains("workspace not found"),
            "expected workspace-not-found error, got: {err}"
        );
    }

    #[test]
    fn create_task_task_scoped_workflow_with_multiple_explicit_targets_fails() {
        let mut ts = TestState::new().with_workflow("task_only", task_only_workflow());
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let src_path = ws.root_path.join("src");
        std::fs::create_dir_all(&src_path).ok();
        std::fs::write(src_path.join("a.rs"), "fn a() {}\n").expect("write a.rs");
        std::fs::write(src_path.join("b.rs"), "fn b() {}\n").expect("write b.rs");
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Task Only Multi".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: Some("task_only".to_string()),
            target_files: Some(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]),
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("task-scoped workflow accepts at most one --target-file")
        );
    }

    #[test]
    fn create_task_ticket_seed_workflow_without_active_tickets_uses_unassigned() {
        let mut ts = TestState::new().with_workflow("ticket_only", ticket_seed_workflow());
        let state = ts.build();

        let payload = CreateTaskPayload {
            name: Some("Ticket Seed Empty".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: Some("ticket_only".to_string()),
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload).expect("create ticket seed empty task");
        assert_eq!(result.total_items, 1);
        let (target_files, item_paths) = load_task_storage(&state, &result.id);
        assert_eq!(target_files, vec![UNASSIGNED_QA_FILE_PATH.to_string()]);
        assert_eq!(item_paths, vec![UNASSIGNED_QA_FILE_PATH.to_string()]);
    }

    #[test]
    fn create_task_ticket_seed_workflow_with_active_tickets_uses_ticket_targets() {
        let mut ts = TestState::new().with_workflow("ticket_only", ticket_seed_workflow());
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let qa_dir = ws.root_path.join("docs/qa");
        std::fs::create_dir_all(&qa_dir).ok();
        std::fs::write(qa_dir.join("from_ticket.md"), "# From Ticket\n")
            .expect("write qa target from ticket");
        let ticket_dir = ws.root_path.join(&ws.ticket_dir);
        std::fs::write(
            ticket_dir.join("active_ticket.md"),
            "**Status**: OPEN\n**QA Document**: `docs/qa/from_ticket.md`\n",
        )
        .expect("write active ticket file");
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Ticket Seed".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: Some("ticket_only".to_string()),
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let result = create_task_impl(&state, payload).expect("create ticket-seed task");
        assert_eq!(result.total_items, 1);
        let (target_files, item_paths) = load_task_storage(&state, &result.id);
        assert_eq!(target_files, vec!["docs/qa/from_ticket.md".to_string()]);
        assert_eq!(item_paths, vec!["docs/qa/from_ticket.md".to_string()]);
    }

    #[test]
    fn create_multiple_tasks_get_unique_ids() {
        let mut ts = TestState::new();
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("multi.md"), "# Multi\n").expect("write multi qa file");
        drop(active);

        let payload1 = CreateTaskPayload {
            name: Some("Task 1".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let payload2 = CreateTaskPayload {
            name: Some("Task 2".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let t1 = create_task_impl(&state, payload1).expect("create first task");
        let t2 = create_task_impl(&state, payload2).expect("create second task");
        assert_ne!(t1.id, t2.id, "tasks should have unique ids");
    }

    #[test]
    fn reset_task_item_for_retry_resets_fields() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Create a task first
        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("retry.md"), "# Retry\n").expect("write retry qa file");
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Retry Task".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
            parent_task_id: None,
            spawn_reason: None,
        };
        let task = create_task_impl(&state, payload).expect("create retry task");

        // Get an item id
        let conn = open_conn(&state.db_path).expect("open retry task database");
        let item_id: String = conn
            .query_row(
                "SELECT id FROM task_items WHERE task_id = ?1 LIMIT 1",
                params![task.id],
                |row| row.get(0),
            )
            .expect("load task item id");

        // Update item to simulate completed/failed state
        conn.execute(
            "UPDATE task_items SET status = 'failed', fix_required = 1, last_error = 'some error', started_at = '2024-01-01', completed_at = '2024-01-01' WHERE id = ?1",
            params![item_id],
        )
        .expect("seed failed task item state");

        // Reset it
        let returned_task_id =
            reset_task_item_for_retry(&state, &item_id).expect("reset task item for retry");
        assert_eq!(returned_task_id, task.id);

        // Verify reset
        let (status, fix_required, last_error, started_at, completed_at): (
            String,
            i64,
            String,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT status, fix_required, last_error, started_at, completed_at FROM task_items WHERE id = ?1",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("reload reset task item");
        assert_eq!(status, "pending");
        assert_eq!(fix_required, 0);
        assert_eq!(last_error, "");
        assert!(started_at.is_none());
        assert!(completed_at.is_none());
    }

    #[test]
    fn reset_task_item_for_retry_nonexistent_item_fails() {
        let mut ts = TestState::new();
        let state = ts.build();
        let result = reset_task_item_for_retry(&state, "nonexistent-item-id");
        assert!(result.is_err(), "should fail for nonexistent item");
    }
}
