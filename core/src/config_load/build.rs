use crate::config::{ActiveConfig, OrchestratorConfig, TaskExecutionPlan, WorkflowConfig};
use crate::db::{
    count_non_terminal_tasks_by_workflow, count_non_terminal_tasks_by_workspace,
    list_non_terminal_tasks_by_workflow, list_non_terminal_tasks_by_workspace,
};
use crate::persistence::repository::{ConfigRepository, SqliteConfigRepository};
use anyhow::{Context, Result};
use std::path::Path;

use super::{
    apply_self_heal_pass, normalize_config, normalize_step_execution_mode_recursive,
    resolve_and_validate_projects, resolve_and_validate_workspaces,
    resolve_and_validate_workspaces_for_project, serialize_config_snapshot,
    validate_agent_env_store_refs, validate_agent_env_store_refs_for_project,
    validate_execution_profiles_for_project, validate_workflow_config,
    validate_workflow_config_with_agents, ConfigSelfHealReport,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// One resource that would be removed by a config update.
pub struct ResourceRemoval {
    /// Resource kind being removed.
    pub kind: String,
    /// Project owning the removed resource.
    pub project_id: String,
    /// Resource name being removed.
    pub name: String,
}

/// Builds the fully resolved active config for the whole workspace.
pub fn build_active_config(app_root: &Path, config: OrchestratorConfig) -> Result<ActiveConfig> {
    let config = normalize_config(config);
    let workspaces = resolve_and_validate_workspaces(app_root, &config)?;
    let projects = resolve_and_validate_projects(app_root, &config)?;
    validate_agent_env_store_refs(&config)?;
    Ok(ActiveConfig {
        workspaces,
        projects,
        config,
    })
}

/// Build active config validating only the target project's workspaces.
/// Other projects are included in the result but not validated, allowing
/// apply to succeed even if another project has broken paths.
pub fn build_active_config_for_project(
    app_root: &Path,
    config: OrchestratorConfig,
    target_project: &str,
) -> Result<ActiveConfig> {
    let config = normalize_config(config);
    let workspaces =
        resolve_and_validate_workspaces_for_project(app_root, &config, target_project)?;
    let projects = resolve_and_validate_projects(app_root, &config)?;
    validate_agent_env_store_refs_for_project(&config, target_project)?;
    Ok(ActiveConfig {
        workspaces,
        projects,
        config,
    })
}

/// Attempts to build active config and persists a healed snapshot when self-heal succeeds.
pub fn build_active_config_with_self_heal(
    app_root: &Path,
    db_path: &Path,
    config: OrchestratorConfig,
) -> Result<(ActiveConfig, Option<ConfigSelfHealReport>)> {
    match build_active_config(app_root, config.clone()) {
        Ok(active) => Ok((active, None)),
        Err(error) => {
            let original_error = error.to_string();
            let maybe_healed = match apply_self_heal_pass(&config) {
                Ok(result) => result,
                Err(_) => anyhow::bail!(original_error),
            };
            let Some((healed_config, changes)) = maybe_healed else {
                anyhow::bail!(original_error);
            };

            let healed_active = match build_active_config(app_root, healed_config) {
                Ok(active) => active,
                Err(_) => anyhow::bail!(original_error),
            };
            let normalized = healed_active.config.clone();
            let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;
            let (healed_version, healed_at) = SqliteConfigRepository::new(db_path)
                .persist_self_heal_snapshot(&yaml, &json_raw, &original_error, &changes)
                .context("failed to persist self-healed config")?;

            Ok((
                healed_active,
                Some(ConfigSelfHealReport {
                    original_error,
                    healed_version,
                    healed_at,
                    changes,
                }),
            ))
        }
    }
}

/// Builds a validated execution plan for a workflow using global agent validation.
pub fn build_execution_plan(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<TaskExecutionPlan> {
    validate_workflow_config(config, workflow, workflow_id)?;
    build_execution_plan_inner(workflow)
}

/// Build an execution plan using project-scoped agents for validation.
/// Strict project isolation: only project agents are used. If the project
/// has no agents, validation proceeds with an empty agent set (and will
/// fail if the workflow requires agent capabilities).
pub fn build_execution_plan_for_project(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
    project_id: &str,
) -> Result<TaskExecutionPlan> {
    let agents: std::collections::HashMap<String, &crate::config::AgentConfig> = config
        .projects
        .get(project_id)
        .map(|project| project.agents.iter().map(|(k, v)| (k.clone(), v)).collect())
        .unwrap_or_default();
    validate_workflow_config_with_agents(&agents, workflow, workflow_id)?;
    validate_execution_profiles_for_project(config, workflow, workflow_id, project_id)?;
    build_execution_plan_inner(workflow)
}

fn build_execution_plan_inner(workflow: &WorkflowConfig) -> Result<TaskExecutionPlan> {
    let mut steps = Vec::new();
    for step in &workflow.steps {
        if !step.enabled {
            continue;
        }
        let normalized_step = task_step_from_workflow_step(step)?;
        steps.push(normalized_step);
    }
    let loop_policy = workflow.loop_policy.clone();
    Ok(TaskExecutionPlan {
        steps,
        loop_policy,
        finalize: workflow.finalize.clone(),
        max_parallel: workflow.max_parallel,
        stagger_delay_ms: workflow.stagger_delay_ms,
        item_isolation: workflow.item_isolation.clone(),
    })
}

pub(crate) fn task_step_from_workflow_step(
    step: &crate::config::WorkflowStepConfig,
) -> Result<crate::config::TaskExecutionStep> {
    let mut normalized = step.clone();
    normalize_step_execution_mode_recursive(&mut normalized)?;

    Ok(crate::config::TaskExecutionStep {
        id: normalized.id.clone(),
        required_capability: normalized.required_capability.clone(),
        execution_profile: normalized.execution_profile.clone(),
        builtin: normalized.builtin.clone(),
        enabled: normalized.enabled,
        repeatable: normalized.repeatable,
        is_guard: normalized.is_guard,
        cost_preference: normalized.cost_preference.clone(),
        prehook: normalized.prehook.clone(),
        tty: normalized.tty,
        template: normalized.template.clone(),
        outputs: normalized.outputs.clone(),
        pipe_to: normalized.pipe_to.clone(),
        command: normalized.command.clone(),
        chain_steps: normalized
            .chain_steps
            .iter()
            .map(task_step_from_workflow_step)
            .collect::<Result<Vec<_>>>()?,
        scope: normalized.scope,
        behavior: normalized.behavior.clone(),
        max_parallel: normalized.max_parallel,
        stagger_delay_ms: normalized.stagger_delay_ms,
        timeout_secs: normalized.timeout_secs,
        stall_timeout_secs: normalized.stall_timeout_secs,
        item_select_config: normalized.item_select_config.clone(),
        store_inputs: normalized.store_inputs.clone(),
        store_outputs: normalized.store_outputs.clone(),
    })
}

/// Enforces deletion guards for all resources removed between two config snapshots.
pub fn enforce_deletion_guards(
    conn: &rusqlite::Connection,
    previous: &OrchestratorConfig,
    candidate: &OrchestratorConfig,
) -> Result<()> {
    let mut removals = Vec::new();
    // Check all projects for removed workspaces/workflows that still have tasks.
    for (project_id, prev_project) in &previous.projects {
        let candidate_project = candidate.projects.get(project_id);
        let removed_workspaces: Vec<String> = prev_project
            .workspaces
            .keys()
            .filter(|id| match candidate_project {
                None => true,
                Some(project) => !project.workspaces.contains_key(*id),
            })
            .cloned()
            .collect();
        for workspace_id in removed_workspaces {
            removals.push(ResourceRemoval {
                kind: "Workspace".to_string(),
                project_id: project_id.clone(),
                name: workspace_id,
            });
        }

        let removed_workflows: Vec<String> = prev_project
            .workflows
            .keys()
            .filter(|id| match candidate_project {
                None => true,
                Some(project) => !project.workflows.contains_key(*id),
            })
            .cloned()
            .collect();
        for workflow_id in removed_workflows {
            removals.push(ResourceRemoval {
                kind: "Workflow".to_string(),
                project_id: project_id.clone(),
                name: workflow_id,
            });
        }
    }

    enforce_deletion_guards_for_removals(conn, &removals)
}

/// Enforces deletion guards for an explicit list of removed resources.
pub fn enforce_deletion_guards_for_removals(
    conn: &rusqlite::Connection,
    removals: &[ResourceRemoval],
) -> Result<()> {
    for removal in removals {
        match removal.kind.as_str() {
            "Workspace" => {
                let task_count = count_non_terminal_tasks_by_workspace(
                    conn,
                    &removal.project_id,
                    &removal.name,
                )?;
                if task_count > 0 {
                    let blockers = list_non_terminal_tasks_by_workspace(
                        conn,
                        &removal.project_id,
                        &removal.name,
                        5,
                    )?;
                    anyhow::bail!(
                        "{}",
                        format_blocking_delete_error(
                            "workspace",
                            &removal.name,
                            &removal.project_id,
                            task_count,
                            &blockers
                        )
                    );
                }
            }
            "Workflow" => {
                let task_count =
                    count_non_terminal_tasks_by_workflow(conn, &removal.project_id, &removal.name)?;
                if task_count > 0 {
                    let blockers = list_non_terminal_tasks_by_workflow(
                        conn,
                        &removal.project_id,
                        &removal.name,
                        5,
                    )?;
                    anyhow::bail!(
                        "{}",
                        format_blocking_delete_error(
                            "workflow",
                            &removal.name,
                            &removal.project_id,
                            task_count,
                            &blockers
                        )
                    );
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn format_blocking_delete_error(
    kind: &str,
    name: &str,
    project_id: &str,
    task_count: i64,
    blockers: &[crate::db::TaskReference],
) -> String {
    let mut message = format!(
        "[FAILED_PRECONDITION] apply/delete would remove {}/{} in project {}, but {} non-terminal task(s) still reference it",
        kind, name, project_id, task_count
    );
    if !blockers.is_empty() {
        message.push_str("\nblocking tasks:");
        for blocker in blockers {
            message.push_str(&format!(
                "\n- {} status={}",
                blocker.task_id, blocker.status
            ));
        }
    }
    message.push_str(&format!(
        "\nsuggested fixes:\n- orchestrator task list --project {}\n- orchestrator task delete <task_id> --force\n- rerun without --prune if deletion is not intended",
        project_id
    ));
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ExecutionMode, LoopMode, OrchestratorConfig};
    use crate::config_load::tests::{
        make_builtin_step, make_command_step, make_config_with_default_project,
        make_minimal_buildable_config, make_step, make_test_db, make_workflow,
    };
    use crate::config_load::{detect_app_root, persist_raw_config};
    #[allow(unused_imports)]
    use std::collections::HashMap;

    #[test]
    fn build_active_config_with_self_heal_recovers_builtin_capability_conflict() {
        let app_root = detect_app_root();
        let (_temp_dir, db_path) = make_test_db();
        let mut config = make_minimal_buildable_config();
        let workflow = config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .get_mut("basic")
            .expect("missing basic workflow");
        let step = workflow
            .steps
            .first_mut()
            .expect("missing builtin self_test step");
        step.required_capability = Some("self_test".to_string());
        // Pass the invalid config directly to self-heal (not loaded from DB)
        // because the CRD round-trip in load_config strips the invalid workflow
        // during workflow_spec_to_config validation.
        let invalid_config = config.clone();
        persist_raw_config(&db_path, config.clone(), "test-seed").expect("seed config");

        let direct_error = build_active_config(&app_root, config)
            .expect_err("invalid config should fail direct active config construction");
        assert!(direct_error
            .to_string()
            .contains("cannot define both builtin and required_capability"));

        let (active, report) =
            build_active_config_with_self_heal(&app_root, &db_path, invalid_config)
                .expect("self-heal wrapper should recover");

        assert!(active
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .map(|p| p.workflows.contains_key("basic"))
            .unwrap_or(false));
        let report = report.expect("expected self-heal report");
        assert!(
            !report.changes.is_empty(),
            "expected recorded self-heal changes"
        );
    }

    #[test]
    fn build_active_config_with_self_heal_persists_self_heal_version() {
        let app_root = detect_app_root();
        let (_temp_dir, db_path) = make_test_db();
        let mut config = make_minimal_buildable_config();
        let workflow = config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .get_mut("basic")
            .expect("missing basic workflow");
        workflow
            .steps
            .first_mut()
            .expect("basic workflow should have a step")
            .required_capability = Some("self_test".to_string());
        let invalid_config = config.clone();
        let seeded = persist_raw_config(&db_path, config, "test-seed").expect("seed config");

        let (_active, report) =
            build_active_config_with_self_heal(&app_root, &db_path, invalid_config)
                .expect("self-heal wrapper should recover");

        let report = report.expect("expected self-heal report");
        assert_eq!(report.healed_version, seeded.version + 1);
        let conn = crate::db::open_conn(&db_path).expect("open sqlite connection");
        let latest_author: String = conn
            .query_row(
                "SELECT author FROM orchestrator_config_versions ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("query latest config version author");
        assert_eq!(latest_author, "self-heal");
    }

    #[test]
    fn build_active_config_with_self_heal_persists_heal_log_entries() {
        let app_root = detect_app_root();
        let (_temp_dir, db_path) = make_test_db();
        let mut config = make_minimal_buildable_config();
        let workflow = config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .get_mut("basic")
            .expect("missing basic workflow");
        workflow
            .steps
            .first_mut()
            .expect("basic workflow should have a step")
            .required_capability = Some("self_test".to_string());
        let invalid_config = config.clone();
        persist_raw_config(&db_path, config, "test-seed").expect("seed config");

        let (_active, report) =
            build_active_config_with_self_heal(&app_root, &db_path, invalid_config)
                .expect("self-heal wrapper should recover");

        let report = report.expect("expected self-heal report");
        let entries =
            crate::config_load::query_heal_log_entries(&db_path, 10).expect("query heal log");
        assert!(
            !entries.is_empty(),
            "heal log entries should be persisted during self-heal"
        );
        assert_eq!(entries[0].version, report.healed_version);
        assert!(entries[0]
            .original_error
            .contains("builtin and required_capability"));
    }

    #[test]
    fn build_active_config_with_self_heal_returns_original_error_for_unhealable_config() {
        let app_root = detect_app_root();
        let (_temp_dir, db_path) = make_test_db();
        let mut config = make_minimal_buildable_config();
        config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workspaces
            .get_mut("default")
            .expect("default workspace")
            .root_path = "fixtures/does-not-exist".to_string();
        persist_raw_config(&db_path, config.clone(), "test-seed").expect("seed config");

        let err = build_active_config_with_self_heal(&app_root, &db_path, config)
            .expect_err("unhealable config should still fail");

        assert!(
            err.to_string().contains("root_path not found"),
            "expected original error to be preserved, got: {err}"
        );
        let conn = crate::db::open_conn(&db_path).expect("open sqlite connection");
        let version_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orchestrator_config_versions",
                [],
                |row| row.get(0),
            )
            .expect("count config versions");
        assert_eq!(
            version_count, 1,
            "unhealable config must not persist new version"
        );
    }

    #[test]
    fn build_execution_plan_returns_only_enabled_steps() {
        let workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
            make_step("qa", false),
        ]);
        let config = make_config_with_default_project();
        let plan =
            build_execution_plan(&config, &workflow, "test-wf").expect("build execution plan");
        assert_eq!(plan.steps.len(), 1, "should only contain enabled steps");
        assert_eq!(plan.steps[0].id, "self_test");
    }

    #[test]
    fn build_execution_plan_copies_step_fields() {
        let mut step = make_command_step("build", "cargo build");
        step.repeatable = false;
        step.tty = true;
        step.outputs = vec!["result".to_string()];
        step.pipe_to = Some("next_step".to_string());
        step.cost_preference = Some(crate::config::CostPreference::Quality);
        step.scope = Some(crate::config::StepScope::Task);
        let workflow = make_workflow(vec![step]);
        let config = make_config_with_default_project();
        let plan =
            build_execution_plan(&config, &workflow, "test-wf").expect("build execution plan");
        let s = &plan.steps[0];
        assert_eq!(s.id, "build");
        assert_eq!(s.command.as_deref(), Some("cargo build"));
        assert!(!s.repeatable);
        assert!(s.tty);
        assert_eq!(s.outputs, vec!["result"]);
        assert_eq!(s.pipe_to.as_deref(), Some("next_step"));
        assert_eq!(
            s.cost_preference,
            Some(crate::config::CostPreference::Quality)
        );
        assert_eq!(s.scope, Some(crate::config::StepScope::Task));
    }

    #[test]
    fn build_execution_plan_includes_chain_steps() {
        let mut step = make_step("smoke_chain", true);
        step.chain_steps = vec![
            make_command_step("sub1", "cargo build"),
            make_command_step("sub2", "cargo test"),
        ];
        let workflow = make_workflow(vec![step]);
        let config = make_config_with_default_project();
        let plan =
            build_execution_plan(&config, &workflow, "test-wf").expect("build execution plan");
        assert_eq!(plan.steps[0].chain_steps.len(), 2);
        assert_eq!(plan.steps[0].chain_steps[0].id, "sub1");
        assert_eq!(plan.steps[0].chain_steps[1].id, "sub2");
        assert_eq!(plan.steps[0].behavior.execution, ExecutionMode::Chain);
        assert_eq!(
            plan.steps[0].chain_steps[0].behavior.execution,
            ExecutionMode::Builtin {
                name: "sub1".to_string()
            }
        );
    }

    #[test]
    fn build_execution_plan_copies_loop_policy() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.mode = LoopMode::Fixed;
        workflow.loop_policy.guard.max_cycles = Some(3);
        let config = make_config_with_default_project();
        let plan =
            build_execution_plan(&config, &workflow, "test-wf").expect("build execution plan");
        assert!(matches!(plan.loop_policy.mode, LoopMode::Fixed));
        assert_eq!(plan.loop_policy.guard.max_cycles, Some(3));
    }

    #[test]
    fn build_execution_plan_copies_finalize_config() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.finalize = crate::config::default_workflow_finalize_config();
        let config = make_config_with_default_project();
        let plan =
            build_execution_plan(&config, &workflow, "test-wf").expect("build execution plan");
        assert!(
            !plan.finalize.rules.is_empty(),
            "finalize rules should be copied"
        );
    }

    #[test]
    fn build_execution_plan_fails_on_invalid_workflow() {
        let workflow = make_workflow(vec![]);
        let config = make_config_with_default_project();
        let result = build_execution_plan(&config, &workflow, "test-wf");
        assert!(result.is_err(), "should fail validation");
    }

    #[test]
    fn build_execution_plan_rehydrates_builtin_execution_from_builtin_field() {
        let mut step = make_builtin_step("self_test", "self_test", true);
        step.behavior.execution = ExecutionMode::Agent;
        let workflow = make_workflow(vec![step]);
        let config = make_config_with_default_project();

        let plan =
            build_execution_plan(&config, &workflow, "test-wf").expect("build execution plan");

        assert_eq!(
            plan.steps[0].behavior.execution,
            ExecutionMode::Builtin {
                name: "self_test".to_string()
            }
        );
        assert_eq!(plan.steps[0].required_capability, None);
    }

    #[test]
    fn enforce_deletion_guards_allows_no_removals() {
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).expect("init schema");
        let conn = crate::db::open_conn(&db_path).expect("open db");
        let config = OrchestratorConfig::default();
        let result = enforce_deletion_guards(&conn, &config, &config);
        assert!(result.is_ok());
        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn enforce_deletion_guards_allows_removing_unused_workspace() {
        use crate::config::WorkspaceConfig;
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).expect("init schema");
        let conn = crate::db::open_conn(&db_path).expect("open db");
        let mut previous_workspaces = HashMap::new();
        previous_workspaces.insert(
            "ws-to-remove".to_string(),
            WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        let mut previous = OrchestratorConfig::default();
        previous
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .workspaces = previous_workspaces;
        let candidate = OrchestratorConfig::default();
        let result = enforce_deletion_guards(&conn, &previous, &candidate);
        assert!(
            result.is_ok(),
            "removing unused workspace should be allowed"
        );
        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn enforce_deletion_guards_allows_removing_unused_workflow() {
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).expect("init schema");
        let conn = crate::db::open_conn(&db_path).expect("open db");
        let mut previous_workflows = HashMap::new();
        previous_workflows.insert("wf-to-remove".to_string(), make_workflow(vec![]));
        let mut previous = OrchestratorConfig::default();
        previous
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .workflows = previous_workflows;
        let candidate = OrchestratorConfig::default();
        let result = enforce_deletion_guards(&conn, &previous, &candidate);
        assert!(result.is_ok(), "removing unused workflow should be allowed");
        std::fs::remove_file(&db_path).ok();
    }

    fn insert_task_reference(
        conn: &rusqlite::Connection,
        task_id: &str,
        project_id: &str,
        workspace_id: &str,
        workflow_id: &str,
        status: &str,
    ) {
        conn.execute(
            "INSERT INTO tasks (id, name, status, started_at, completed_at, goal, target_files_json, mode, project_id, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, resume_token, created_at, updated_at, parent_task_id, spawn_reason, spawn_depth)
             VALUES (?1, 'test', ?2, NULL, NULL, 'goal', '[]', '', ?3, ?4, ?5, '/tmp', '[]', 'tickets', '{}', 'once', 0, 0, NULL, datetime('now'), datetime('now'), NULL, NULL, 0)",
            rusqlite::params![task_id, status, project_id, workspace_id, workflow_id],
        )
        .expect("insert task reference");
    }

    #[test]
    fn enforce_deletion_guards_blocks_same_project_non_terminal_workflow_tasks() {
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).expect("init schema");
        let conn = crate::db::open_conn(&db_path).expect("open db");
        insert_task_reference(
            &conn,
            "task-running",
            crate::config::DEFAULT_PROJECT_ID,
            "default",
            "wf-to-remove",
            "running",
        );

        let mut previous = OrchestratorConfig::default();
        previous
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .workflows
            .insert("wf-to-remove".to_string(), make_workflow(vec![]));
        let candidate = OrchestratorConfig::default();

        let error = enforce_deletion_guards(&conn, &previous, &candidate)
            .expect_err("running task should block workflow deletion");
        let message = error.to_string();
        assert!(message.contains("workflow/wf-to-remove"));
        assert!(message.contains("project default"));
        assert!(message.contains("task-running status=running"));
        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn enforce_deletion_guards_ignores_terminal_workflow_tasks() {
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).expect("init schema");
        let conn = crate::db::open_conn(&db_path).expect("open db");
        insert_task_reference(
            &conn,
            "task-complete",
            crate::config::DEFAULT_PROJECT_ID,
            "default",
            "wf-to-remove",
            "completed",
        );

        let mut previous = OrchestratorConfig::default();
        previous
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .workflows
            .insert("wf-to-remove".to_string(), make_workflow(vec![]));
        let candidate = OrchestratorConfig::default();

        let result = enforce_deletion_guards(&conn, &previous, &candidate);
        assert!(
            result.is_ok(),
            "terminal task should not block workflow deletion"
        );
        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn enforce_deletion_guards_ignores_other_project_tasks_with_same_workflow_id() {
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).expect("init schema");
        let conn = crate::db::open_conn(&db_path).expect("open db");
        insert_task_reference(
            &conn,
            "task-other-project",
            "other-project",
            "default",
            "wf-to-remove",
            "running",
        );

        let mut previous = OrchestratorConfig::default();
        previous
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .workflows
            .insert("wf-to-remove".to_string(), make_workflow(vec![]));
        let candidate = OrchestratorConfig::default();

        let result = enforce_deletion_guards(&conn, &previous, &candidate);
        assert!(
            result.is_ok(),
            "same workflow id in another project should not block"
        );
        std::fs::remove_file(&db_path).ok();
    }
}
