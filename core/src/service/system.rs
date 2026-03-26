use super::daemon::runtime_snapshot;
use crate::config_load::read_active_config;
use crate::error::{OrchestratorError, Result, classify_system_error};
use crate::persistence::migration;
use crate::persistence::repository::{SchedulerRepository, SqliteSchedulerRepository};
use crate::state::InnerState;
use anyhow::Context;
use std::path::{Path, PathBuf};

/// Result of manifest validation with both plain-text and structured diagnostics.
#[derive(Debug, Clone)]
pub struct ManifestValidationReport {
    /// Whether all manifests validated successfully.
    pub valid: bool,
    /// Plain-text error strings for quick CLI output.
    pub errors: Vec<String>,
    /// Summary message describing the overall result.
    pub message: String,
    /// Structured diagnostics for gRPC and UI consumers.
    pub diagnostics: Vec<orchestrator_proto::DiagnosticEntry>,
}

// NOTE: RenderedCheckReport, run_check, and diagnostic_entry_from_check
// moved to orchestrator-scheduler crate (service::system module).

/// Get debug information for a component.
pub fn debug_info(state: &InnerState, component: Option<&str>) -> Result<String> {
    let comp = component.unwrap_or("state");
    match comp {
        "state" => Ok(
            "Debug Information\n=================\n\nAvailable: state, config, dag, messagebus\n"
                .to_string(),
        ),
        "config" => {
            let config = read_active_config(state)
                .map_err(|err| classify_system_error("system.debug_info", err))?;
            Ok(format!(
                "Active Configuration:\n{}",
                serde_yaml::to_string(&config.config).unwrap_or_default()
            ))
        }
        "dag" => debug_dag_info(state),
        "messagebus" => Ok(
            "MessageBus Debug Information\n============================\n\nMessageBus is an internal component.\n"
                .to_string(),
        ),
        _ => Ok(format!(
            "Unknown debug component: {}\nAvailable: state, config, dag, messagebus\n",
            comp
        )),
    }
}

fn debug_dag_info(state: &InnerState) -> Result<String> {
    let active = read_active_config(state)?;
    let mut lines = vec![
        "DAG Debug Information".to_string(),
        "=====================".to_string(),
        String::new(),
    ];

    for (project_id, project) in &active.projects {
        for (workflow_id, workflow) in &project.workflows {
            let planner_agent = workflow
                .adaptive
                .as_ref()
                .and_then(|cfg| cfg.planner_agent.clone())
                .unwrap_or_else(|| "-".to_string());
            let planner_enabled = workflow
                .adaptive
                .as_ref()
                .map(|cfg| cfg.enabled)
                .unwrap_or(false);
            lines.push(format!(
                "project={} workflow={} mode={:?} fallback={:?} persist_graph_snapshots={} adaptive_enabled={} planner_agent={} dynamic_steps={}",
                project_id,
                workflow_id,
                workflow.execution.mode,
                workflow.execution.fallback_mode,
                workflow.execution.persist_graph_snapshots,
                planner_enabled,
                planner_agent,
                workflow.dynamic_steps.len(),
            ));
        }
    }

    if lines.len() == 3 {
        lines.push("no workflows loaded".to_string());
    }

    Ok(lines.join("\n"))
}

/// Returns the number of tasks currently in the pending state.
pub async fn pending_task_count(state: &InnerState) -> anyhow::Result<i64> {
    SqliteSchedulerRepository::new(state.async_database.clone())
        .pending_task_count()
        .await
}

/// Returns the marker-file path used to request worker shutdown.
pub fn worker_stop_signal_path(state: &InnerState) -> PathBuf {
    state.data_dir.join("worker.stop")
}

/// Removes the worker stop marker if it exists.
pub fn clear_worker_stop_signal(state: &InnerState) -> anyhow::Result<()> {
    let path = worker_stop_signal_path(state);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Writes the worker stop marker and wakes the worker loop.
pub fn signal_worker_stop(state: &InnerState) -> anyhow::Result<()> {
    let path = worker_stop_signal_path(state);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, "stop")?;
    state.worker_notify.notify_waiters();
    Ok(())
}

/// Get worker status.
pub async fn worker_status(state: &InnerState) -> Result<orchestrator_proto::WorkerStatusResponse> {
    let pending = pending_task_count(state)
        .await
        .map_err(|err| classify_system_error("system.worker_status", err))?;
    let stop_signal = worker_stop_signal_path(state).exists();
    let runtime = runtime_snapshot(state);

    Ok(orchestrator_proto::WorkerStatusResponse {
        pending_tasks: pending,
        stop_signal,
        active_workers: runtime.active_workers as i64,
        idle_workers: runtime.idle_workers as i64,
        running_tasks: runtime.running_tasks as i64,
        configured_workers: runtime.configured_workers as i64,
        shutdown_requested: runtime.shutdown_requested,
        lifecycle_state: runtime.lifecycle_state.as_str().to_string(),
        total_worker_restarts: runtime.total_worker_restarts as i64,
    })
}

/// Initialize orchestrator runtime at the given root.
pub fn run_init(state: &InnerState, root: Option<&str>) -> Result<String> {
    if let Some(root_path) = root {
        let path = if Path::new(root_path).is_absolute() {
            std::path::PathBuf::from(root_path)
        } else {
            state.data_dir.join(root_path)
        };
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create workspace root {}", path.display()))?;
    }
    Ok(format!(
        "Orchestrator initialized at {} (sqlite: {})",
        state.data_dir.display(),
        state.db_path.display()
    ))
}

/// Returns database migration status for the current runtime database.
pub fn db_status(state: &InnerState) -> Result<orchestrator_proto::DbStatusResponse> {
    let status = crate::persistence::schema::PersistenceBootstrap::status(&state.db_path)
        .map_err(|err| classify_system_error("system.db_status", err))?;
    let is_current = status.is_current();
    let size_info = crate::db_maintenance::database_size_info(
        &state.db_path,
        &state.logs_dir,
        None, // TODO: archive dir from config
    )
    .unwrap_or(crate::db_maintenance::SizeInfo {
        db_size: 0,
        logs_size: 0,
        archive_size: 0,
    });
    Ok(orchestrator_proto::DbStatusResponse {
        db_path: state.db_path.display().to_string(),
        current_version: status.current_version,
        target_version: status.target_version,
        pending_versions: status.pending_versions,
        pending_names: status
            .pending_names
            .into_iter()
            .map(str::to_string)
            .collect(),
        is_current,
        db_size_bytes: size_info.db_size,
        logs_size_bytes: size_info.logs_size,
        archive_size_bytes: size_info.archive_size,
    })
}

/// Lists registered migrations and whether each one has been applied.
pub fn db_migrations_list(
    state: &InnerState,
) -> Result<orchestrator_proto::DbMigrationsListResponse> {
    let conn = crate::db::open_conn(&state.db_path)
        .map_err(|err| classify_system_error("system.db_migrations_list", err))?;
    let status = migration::registered_status(&conn)
        .map_err(|err| classify_system_error("system.db_migrations_list", err))?;
    let migrations = migration::registered_migration_statuses(&conn)?
        .into_iter()
        .map(|migration| orchestrator_proto::DbMigration {
            version: migration.version,
            name: migration.name.to_string(),
            applied: migration.applied,
        })
        .collect();

    Ok(orchestrator_proto::DbMigrationsListResponse {
        db_path: state.db_path.display().to_string(),
        current_version: status.current_version,
        target_version: status.target_version,
        migrations,
    })
}

/// Reset the database.
pub fn run_db_reset(
    state: &InnerState,
    force: bool,
    include_history: bool,
    include_config: bool,
) -> Result<String> {
    if !force {
        return Err(OrchestratorError::invalid_state(
            "system.db_reset",
            anyhow::anyhow!("Use --force to confirm database reset"),
        ));
    }
    crate::db::reset_db_by_path(&state.db_path, include_history, include_config)
        .map_err(|err| classify_system_error("system.db_reset", err))?;

    // When config is cleared from SQLite, sync the daemon's in-memory state
    // to avoid stale ActiveConfig surviving until the next `apply`.
    if include_config {
        crate::state::reset_active_config_to_default(state);
    }

    let mut msg = "Database reset completed".to_string();
    if include_config {
        msg.push_str("\nAll config versions deleted (next apply starts from blank)");
    } else if include_history {
        msg.push_str("\nConfig version history cleared (active version preserved)");
    }
    Ok(msg)
}

/// Validate manifest YAML content. Returns (valid, errors, message).
///
/// Validation is always project-scoped. Omitting `project_id` targets the
/// built-in `default` project.
pub fn validate_manifests(
    state: &InnerState,
    content: &str,
    project_id: Option<&str>,
) -> Result<ManifestValidationReport> {
    use crate::crd::{self, ParsedManifest};
    use crate::resource::{Resource, dispatch_resource, kind_as_str};

    let manifests = match crate::resource::parse_manifests_from_yaml(content) {
        Ok(m) => m,
        Err(e) => {
            let err = e.to_string();
            return Ok(ManifestValidationReport {
                valid: false,
                errors: vec![err.clone()],
                message: "Parse error".to_string(),
                diagnostics: vec![diagnostic_entry_from_error("parse_error", err)],
            });
        }
    };

    let mut merged_config = crate::config_load::load_config(&state.db_path)
        .map_err(|err| classify_system_error("system.manifest_validate", err))?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();

    let effective_project_id = project_id.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let mut errors = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, manifest) in manifests.into_iter().enumerate() {
        match manifest {
            ParsedManifest::Builtin(resource) => {
                if let Err(error) = resource.validate_version() {
                    let message = format!("document {}: {}", index + 1, error);
                    diagnostics.push(diagnostic_entry_from_error(
                        "manifest_version_invalid",
                        message.clone(),
                    ));
                    errors.push(message);
                    continue;
                }
                let registered = match dispatch_resource(*resource) {
                    Ok(r) => r,
                    Err(error) => {
                        let message = format!("document {}: {}", index + 1, error);
                        diagnostics.push(diagnostic_entry_from_error(
                            "manifest_dispatch_failed",
                            message.clone(),
                        ));
                        errors.push(message);
                        continue;
                    }
                };
                if let Err(error) = registered.validate() {
                    let message = format!(
                        "{}/{} invalid: {}",
                        kind_as_str(registered.kind()),
                        registered.name(),
                        error
                    );
                    diagnostics.push(diagnostic_entry_from_error(
                        "manifest_resource_invalid",
                        message.clone(),
                    ));
                    errors.push(message);
                    continue;
                }
                let _ = crate::resource::apply_to_project(
                    &registered,
                    &mut merged_config,
                    effective_project_id,
                );
            }
            ParsedManifest::Crd(crd_manifest) => {
                if let Err(error) = crd::apply_crd(&mut merged_config, crd_manifest) {
                    let message = format!("document {}: {}", index + 1, error);
                    diagnostics.push(diagnostic_entry_from_error(
                        "crd_apply_failed",
                        message.clone(),
                    ));
                    errors.push(message);
                }
            }
            ParsedManifest::Custom(cr_manifest) => {
                if let Err(error) = crd::apply_custom_resource(&mut merged_config, cr_manifest) {
                    let message = format!("document {}: {}", index + 1, error);
                    diagnostics.push(diagnostic_entry_from_error(
                        "custom_resource_apply_failed",
                        message.clone(),
                    ));
                    errors.push(message);
                }
            }
        }
    }

    if !errors.is_empty() {
        return Ok(ManifestValidationReport {
            valid: false,
            errors,
            message: "Validation failed".to_string(),
            diagnostics,
        });
    }

    // Try to build active config to validate the full configuration
    match crate::config_load::build_active_config_for_project(
        &state.data_dir,
        merged_config,
        effective_project_id,
    ) {
        Ok(_) => Ok(ManifestValidationReport {
            valid: true,
            errors: vec![],
            message: "Manifest is valid".to_string(),
            diagnostics: vec![],
        }),
        Err(e) => {
            let message = e.to_string();
            Ok(ManifestValidationReport {
                valid: false,
                errors: vec![message.clone()],
                message: "Config build failed".to_string(),
                diagnostics: vec![diagnostic_entry_from_error("config_build_failed", message)],
            })
        }
    }
}

fn diagnostic_entry_from_error(
    rule: impl Into<String>,
    message: impl Into<String>,
) -> orchestrator_proto::DiagnosticEntry {
    orchestrator_proto::DiagnosticEntry {
        source: "manifest_validate".to_string(),
        rule: rule.into(),
        severity: "error".to_string(),
        passed: false,
        blocking: true,
        message: message.into(),
        context: None,
        scope: None,
        actual: None,
        expected: None,
        risk: None,
        suggested_fix: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    /// Lightweight enqueue helper for tests — sets task status to pending and
    /// emits the scheduler_enqueued event.  Avoids a dependency on the
    /// `orchestrator-scheduler` crate.
    async fn enqueue_task_for_test(state: &InnerState, task_id: &str) {
        state.task_repo.reset_unresolved_items(task_id).await.expect("reset items");
        state
            .db_writer
            .set_task_status(task_id, "pending", false)
            .await
            .expect("set pending");
        state.worker_notify.notify_waiters();
        crate::events::insert_event(
            state,
            task_id,
            None,
            "scheduler_enqueued",
            serde_json::json!({"task_id": task_id}),
        )
        .await
        .expect("insert enqueue event");
    }

    fn workflow_manifest(name: &str) -> String {
        format!(
            "apiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: {name}\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo ok\"\n  loop:\n    mode: once\n"
        )
    }

    #[test]
    fn debug_info_covers_known_and_unknown_components() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let state_info = debug_info(&state, None).expect("default debug info");
        assert!(state_info.contains("Available: state, config, dag, messagebus"));

        let config_info = debug_info(&state, Some("config")).expect("config debug info");
        assert!(config_info.contains("Active Configuration"));

        let dag_info = debug_info(&state, Some("dag")).expect("dag debug info");
        assert!(dag_info.contains("DAG Debug Information"));

        let messagebus_info =
            debug_info(&state, Some("messagebus")).expect("messagebus debug info");
        assert!(messagebus_info.contains("MessageBus"));

        let unknown = debug_info(&state, Some("bogus")).expect("unknown component");
        assert!(unknown.contains("Unknown debug component"));
    }

    #[tokio::test]
    async fn worker_status_reports_pending_tasks_and_stop_signal() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/system-worker.md");
        std::fs::write(&qa_file, "# worker\n").expect("seed qa file");
        let created = create_task_impl(&state, crate::dto::CreateTaskPayload::default())
            .expect("create task");
        enqueue_task_for_test(&state, &created.id).await;

        let status = worker_status(&state).await.expect("worker status");
        assert_eq!(status.pending_tasks, 1);
        assert!(!status.stop_signal);
        assert_eq!(status.active_workers, 0);
        assert_eq!(status.idle_workers, 0);
        assert_eq!(status.running_tasks, 0);
        assert_eq!(status.lifecycle_state, "serving");
        assert!(!status.shutdown_requested);

        state.daemon_runtime.set_configured_workers(2);
        state.daemon_runtime.worker_started();
        state.daemon_runtime.worker_started();
        state.daemon_runtime.worker_became_busy();
        state.daemon_runtime.running_task_started();

        let busy = worker_status(&state).await.expect("worker status busy");
        assert_eq!(busy.active_workers, 1);
        assert_eq!(busy.idle_workers, 1);
        assert_eq!(busy.running_tasks, 1);
        assert_eq!(busy.configured_workers, 2);

        std::fs::write(worker_stop_signal_path(&state), "stop").expect("seed stop signal");
        let stopped = worker_status(&state)
            .await
            .expect("worker status with stop");
        assert!(stopped.stop_signal);
    }

    #[test]
    fn run_init_creates_requested_directory_and_reports_paths() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let message = run_init(&state, Some("workspace/new-root")).expect("run init");
        assert!(message.contains("Orchestrator initialized at"));
        assert!(state.data_dir.join("workspace/new-root").exists());
    }

    #[test]
    fn db_status_reports_current_schema() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let status = db_status(&state).expect("db status");
        assert!(status.db_path.ends_with("agent_orchestrator.db"));
        assert!(status.is_current);
        assert_eq!(status.current_version, status.target_version);
        assert!(status.pending_versions.is_empty());
        assert!(status.pending_names.is_empty());
    }

    #[test]
    fn db_migrations_list_marks_all_migrations_applied_on_seeded_state() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let list = db_migrations_list(&state).expect("db migrations list");
        assert!(list.db_path.ends_with("agent_orchestrator.db"));
        assert_eq!(list.current_version, list.target_version);
        assert!(!list.migrations.is_empty());
        assert!(list.migrations.iter().all(|migration| migration.applied));
    }

    #[test]
    fn run_db_reset_requires_force_and_clears_in_memory_config_when_requested() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let err = run_db_reset(&state, false, false, false).expect_err("force is required");
        assert!(err.to_string().contains("Use --force"));

        let message = run_db_reset(&state, true, false, true).expect("reset with config");
        assert!(message.contains("All config versions deleted"));
        assert!(
            crate::config_load::read_loaded_config(&state)
                .expect("read active config")
                .projects
                .is_empty()
        );
    }

    #[test]
    fn validate_manifests_handles_parse_valid_and_invalid_config() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let parse_result = validate_manifests(&state, "not: [yaml", None).expect("parse result");
        assert!(!parse_result.valid);
        assert_eq!(parse_result.message, "Parse error");
        assert!(!parse_result.diagnostics.is_empty());

        let valid = validate_manifests(&state, &workflow_manifest("validated"), None)
            .expect("valid manifest");
        assert!(valid.valid);
        assert_eq!(valid.message, "Manifest is valid");

        let invalid = validate_manifests(
            &state,
            "apiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: broken\nspec:\n  steps: []\n  loop:\n    mode: once\n",
            None,
        )
        .expect("invalid manifest");
        assert!(!invalid.valid);
        assert_eq!(invalid.message, "Validation failed");
        assert!(!invalid.errors.is_empty());
    }

    #[tokio::test]
    async fn pending_task_count_returns_correct_count() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        assert_eq!(pending_task_count(&state).await.expect("count 0"), 0);

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/count_test.md");
        std::fs::write(&qa_file, "# count test\n").expect("seed qa file");
        let t1 =
            create_task_impl(&state, crate::dto::CreateTaskPayload::default()).expect("create 1");
        let t2 =
            create_task_impl(&state, crate::dto::CreateTaskPayload::default()).expect("create 2");
        enqueue_task_for_test(&state, &t1.id).await;
        enqueue_task_for_test(&state, &t2.id).await;
        assert_eq!(pending_task_count(&state).await.expect("count 2"), 2);
    }

    #[test]
    fn signal_worker_stop_creates_stop_file() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let stop_path = worker_stop_signal_path(&state);
        assert!(!stop_path.exists());
        signal_worker_stop(&state).expect("signal stop");
        assert!(stop_path.exists());
        assert_eq!(
            std::fs::read_to_string(&stop_path).expect("read"),
            "stop"
        );
    }

    #[test]
    fn clear_worker_stop_signal_removes_stop_file() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        signal_worker_stop(&state).expect("signal stop");
        assert!(worker_stop_signal_path(&state).exists());
        clear_worker_stop_signal(&state).expect("clear");
        assert!(!worker_stop_signal_path(&state).exists());
    }

    #[test]
    fn clear_worker_stop_signal_noop_when_no_file() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        clear_worker_stop_signal(&state).expect("clear nonexistent");
    }

    #[test]
    fn worker_signal_paths_are_under_data_dir() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let stop_path = worker_stop_signal_path(&state);
        assert!(stop_path.starts_with(&*state.data_dir));
        assert!(stop_path.ends_with("worker.stop"));
    }

    // NOTE: run_check tests moved to orchestrator-scheduler crate.
}
