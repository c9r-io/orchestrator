use super::daemon::runtime_snapshot;
use crate::config_load::read_active_config;
use crate::error::{classify_system_error, OrchestratorError, Result};
use crate::persistence::migration;
use crate::scheduler::check::{run_checks, CheckReport, CheckResult};
use crate::scheduler_service::{pending_task_count, worker_stop_signal_path};
use crate::state::InnerState;
use anyhow::Context;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ManifestValidationReport {
    pub valid: bool,
    pub errors: Vec<String>,
    pub message: String,
    pub diagnostics: Vec<orchestrator_proto::DiagnosticEntry>,
}

#[derive(Debug, Clone)]
pub struct RenderedCheckReport {
    pub report: CheckReport,
    pub content: String,
    pub exit_code: i32,
}

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
                serde_yml::to_string(&config.config).unwrap_or_default()
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
    })
}

/// Initialize orchestrator runtime at the given root.
pub fn run_init(state: &InnerState, root: Option<&str>) -> Result<String> {
    if let Some(root_path) = root {
        let path = if Path::new(root_path).is_absolute() {
            std::path::PathBuf::from(root_path)
        } else {
            state.app_root.join(root_path)
        };
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create workspace root {}", path.display()))?;
    }
    Ok(format!(
        "Orchestrator initialized at {} (sqlite: {})",
        state.app_root.display(),
        state.db_path.display()
    ))
}

pub fn db_status(state: &InnerState) -> Result<orchestrator_proto::DbStatusResponse> {
    let status = crate::persistence::schema::PersistenceBootstrap::status(&state.db_path)
        .map_err(|err| classify_system_error("system.db_status", err))?;
    let is_current = status.is_current();
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
    })
}

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
    use crate::resource::{dispatch_resource, kind_as_str, Resource};

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
                let registered = match dispatch_resource(resource) {
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
    match crate::config_load::build_active_config(&state.app_root, merged_config) {
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

/// Run preflight checks. Returns (content, exit_code).
/// When `project_id` is Some, checks are scoped to that project's resources.
pub fn run_check(
    state: &InnerState,
    workflow: Option<&str>,
    output_format: &str,
    project_id: Option<&str>,
) -> Result<RenderedCheckReport> {
    let active = read_active_config(state)?;
    let report = run_checks(&active, &state.app_root, workflow, project_id);

    let content = match output_format {
        "json" => serde_json::to_string_pretty(&report)?,
        "yaml" => serde_yml::to_string(&report)?,
        _ => {
            let mut buf = String::new();
            buf.push_str("orchestrator check — preflight validation\n\n");
            for check in &report.checks {
                let icon = if check.passed {
                    "\u{2713}"
                } else {
                    match check.severity {
                        crate::anomaly::Severity::Error => "\u{2717}",
                        crate::anomaly::Severity::Warning => "\u{26a0}",
                        crate::anomaly::Severity::Info => "\u{2139}",
                    }
                };
                buf.push_str(&format!("{} [{}] {}\n", icon, check.rule, check.message));
                if let Some(actual) = check.actual.as_deref() {
                    buf.push_str(&format!("  actual: {actual}\n"));
                }
                if let Some(expected) = check.expected.as_deref() {
                    buf.push_str(&format!("  expected: {expected}\n"));
                }
                if let Some(risk) = check.risk.as_deref() {
                    buf.push_str(&format!("  risk: {risk}\n"));
                }
                if let Some(fix) = check.suggested_fix.as_deref() {
                    buf.push_str(&format!("  suggested_fix: {fix}\n"));
                }
            }
            buf.push_str(&format!(
                "\n{} passed, {} errors, {} warnings\n",
                report.summary.passed, report.summary.errors, report.summary.warnings
            ));
            buf
        }
    };

    let exit_code = if report.summary.errors > 0 { 1 } else { 0 };
    Ok(RenderedCheckReport {
        report,
        content,
        exit_code,
    })
}

pub fn diagnostic_entry_from_check(check: &CheckResult) -> orchestrator_proto::DiagnosticEntry {
    orchestrator_proto::DiagnosticEntry {
        source: check.source.clone(),
        rule: check.rule.clone(),
        severity: format!("{:?}", check.severity).to_lowercase(),
        passed: check.passed,
        blocking: check.blocking,
        message: check.message.clone(),
        context: check.context.clone(),
        scope: check.scope.clone(),
        actual: check.actual.clone(),
        expected: check.expected.clone(),
        risk: check.risk.clone(),
        suggested_fix: check.suggested_fix.clone(),
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
    use crate::service::resource::apply_manifests;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

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
            .app_root
            .join("workspace/default/docs/qa/system-worker.md");
        std::fs::write(&qa_file, "# worker\n").expect("seed qa file");
        create_task_impl(&state, crate::dto::CreateTaskPayload::default()).expect("create task");

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
        assert!(state.app_root.join("workspace/new-root").exists());
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
        assert!(crate::config_load::read_loaded_config(&state)
            .expect("read active config")
            .projects
            .is_empty());
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

    #[test]
    fn run_check_supports_text_json_and_yaml_outputs() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        apply_manifests(
            &state,
            &workflow_manifest("checkable"),
            false,
            Some(crate::config::DEFAULT_PROJECT_ID),
            false,
        )
        .expect("apply workflow manifest");

        let text = run_check(&state, None, "text", None).expect("text check");
        assert!(text.content.contains("orchestrator check"));
        assert_eq!(text.exit_code, 0);

        let json = run_check(&state, None, "json", None).expect("json check");
        assert!(json.content.contains("\"summary\""));
        assert_eq!(json.exit_code, 0);
        assert!(!json.report.checks.is_empty());

        let yaml = run_check(&state, None, "yaml", None).expect("yaml check");
        assert!(yaml.content.contains("summary:"));
        assert_eq!(yaml.exit_code, 0);
    }
}
