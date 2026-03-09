use crate::config::{ActiveConfig, OrchestratorConfig, TaskExecutionPlan, WorkflowConfig};
use crate::db::{count_tasks_by_workflow, count_tasks_by_workspace, open_conn};
use anyhow::{Context, Result};
use std::path::Path;

use super::{
    apply_self_heal_pass, normalize_config, normalize_step_execution_mode_recursive,
    persist_config_versioned, persist_heal_log, resolve_and_validate_projects,
    resolve_and_validate_workspaces, serialize_config_snapshot, validate_agent_env_store_refs,
    validate_workflow_config, validate_workflow_config_with_agents, ConfigSelfHealReport,
};

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
            let conn = open_conn(db_path)?;
            let tx = conn.unchecked_transaction()?;
            let (healed_version, healed_at) =
                persist_config_versioned(&tx, &yaml, &json_raw, "self-heal")
                    .context("failed to persist self-healed config")?;
            persist_heal_log(&tx, healed_version, &original_error, &changes)
                .context("failed to persist self-heal log entries")?;
            tx.commit()
                .context("failed to commit self-healed config version")?;

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
        timeout_secs: normalized.timeout_secs,
        item_select_config: normalized.item_select_config.clone(),
        store_inputs: normalized.store_inputs.clone(),
        store_outputs: normalized.store_outputs.clone(),
    })
}

pub fn enforce_deletion_guards(
    conn: &rusqlite::Connection,
    previous: &OrchestratorConfig,
    candidate: &OrchestratorConfig,
) -> Result<()> {
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
            let task_count = count_tasks_by_workspace(conn, &workspace_id)?;
            if task_count > 0 {
                anyhow::bail!(
                    "cannot delete workspace '{}' because {} tasks reference it",
                    workspace_id,
                    task_count
                );
            }
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
            let task_count = count_tasks_by_workflow(conn, &workflow_id)?;
            if task_count > 0 {
                anyhow::bail!(
                    "cannot delete workflow '{}' because {} tasks reference it",
                    workflow_id,
                    task_count
                );
            }
        }
    }

    Ok(())
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
    use crate::config_load::load_config;
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

        let (active, report) = build_active_config_with_self_heal(
            &app_root,
            &db_path,
            invalid_config,
        )
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

        let (_active, report) = build_active_config_with_self_heal(
            &app_root,
            &db_path,
            invalid_config,
        )
        .expect("self-heal wrapper should recover");

        let report = report.expect("expected self-heal report");
        assert_eq!(report.healed_version, seeded.version + 1);
        let conn = open_conn(&db_path).expect("open sqlite connection");
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

        let (_active, report) = build_active_config_with_self_heal(
            &app_root,
            &db_path,
            invalid_config,
        )
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
            err.to_string()
                .contains("root_path not found"),
            "expected original error to be preserved, got: {err}"
        );
        let conn = open_conn(&db_path).expect("open sqlite connection");
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
}
