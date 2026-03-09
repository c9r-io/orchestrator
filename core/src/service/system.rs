use crate::config_load::read_active_config;
use crate::scheduler::check::run_checks;
use crate::scheduler_service::{pending_task_count, worker_stop_signal_path};
use crate::state::InnerState;
use anyhow::{Context, Result};
use std::path::Path;

/// Get debug information for a component.
pub fn debug_info(state: &InnerState, component: Option<&str>) -> Result<String> {
    let comp = component.unwrap_or("state");
    match comp {
        "state" => Ok(
            "Debug Information\n=================\n\nAvailable: state, config, messagebus\n"
                .to_string(),
        ),
        "config" => {
            let config = read_active_config(state)?;
            Ok(format!(
                "Active Configuration:\n{}",
                serde_yml::to_string(&config.config).unwrap_or_default()
            ))
        }
        "messagebus" => Ok(
            "MessageBus Debug Information\n============================\n\nMessageBus is an internal component.\n"
                .to_string(),
        ),
        _ => Ok(format!("Unknown debug component: {}\nAvailable: state, config, messagebus\n", comp)),
    }
}

/// Get worker status.
pub async fn worker_status(state: &InnerState) -> Result<orchestrator_proto::WorkerStatusResponse> {
    let pending = pending_task_count(state).await?;
    let stop_signal = worker_stop_signal_path(state).exists();

    Ok(orchestrator_proto::WorkerStatusResponse {
        pending_tasks: pending,
        stop_signal,
        active_workers: 0, // TODO: track active worker count
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

/// Reset the database.
pub fn run_db_reset(
    state: &InnerState,
    force: bool,
    include_history: bool,
    include_config: bool,
) -> Result<String> {
    if !force {
        anyhow::bail!("Use --force to confirm database reset");
    }
    crate::db::reset_db_by_path(&state.db_path, include_history, include_config)?;

    // When config is cleared from SQLite, sync the daemon's in-memory state
    // to avoid stale ActiveConfig surviving until the next `apply`.
    if include_config {
        if let Ok(mut active) = state.active_config.write() {
            *active = crate::config::ActiveConfig {
                config: Default::default(),
                workspaces: Default::default(),
                projects: Default::default(),
            };
        }
        if let Ok(mut error) = state.active_config_error.write() {
            *error = None;
        }
        if let Ok(mut notice) = state.active_config_notice.write() {
            *notice = None;
        }
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
) -> Result<(bool, Vec<String>, String)> {
    use crate::crd::{self, ParsedManifest};
    use crate::resource::{dispatch_resource, kind_as_str, Resource};

    let manifests = match crate::resource::parse_manifests_from_yaml(content) {
        Ok(m) => m,
        Err(e) => return Ok((false, vec![e.to_string()], "Parse error".to_string())),
    };

    let mut merged_config = crate::config_load::load_raw_config_from_db(&state.db_path)?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();

    let effective_project_id = project_id.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let mut errors = Vec::new();
    for (index, manifest) in manifests.into_iter().enumerate() {
        match manifest {
            ParsedManifest::Builtin(resource) => {
                if let Err(error) = resource.validate_version() {
                    errors.push(format!("document {}: {}", index + 1, error));
                    continue;
                }
                let registered = match dispatch_resource(resource) {
                    Ok(r) => r,
                    Err(error) => {
                        errors.push(format!("document {}: {}", index + 1, error));
                        continue;
                    }
                };
                if let Err(error) = registered.validate() {
                    errors.push(format!(
                        "{}/{} invalid: {}",
                        kind_as_str(registered.kind()),
                        registered.name(),
                        error
                    ));
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
                    errors.push(format!("document {}: {}", index + 1, error));
                }
            }
            ParsedManifest::Custom(cr_manifest) => {
                if let Err(error) = crd::apply_custom_resource(&mut merged_config, cr_manifest) {
                    errors.push(format!("document {}: {}", index + 1, error));
                }
            }
        }
    }

    if !errors.is_empty() {
        return Ok((false, errors, "Validation failed".to_string()));
    }

    // Try to build active config to validate the full configuration
    match crate::config_load::build_active_config(&state.app_root, merged_config) {
        Ok(_) => Ok((true, vec![], "Manifest is valid".to_string())),
        Err(e) => Ok((
            false,
            vec![e.to_string()],
            "Config build failed".to_string(),
        )),
    }
}

/// Run preflight checks. Returns (content, exit_code).
/// When `project_id` is Some, checks are scoped to that project's resources.
pub fn run_check(
    state: &InnerState,
    workflow: Option<&str>,
    output_format: &str,
    project_id: Option<&str>,
) -> Result<(String, i32)> {
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
                buf.push_str(&format!("{} {}\n", icon, check.message));
            }
            buf.push_str(&format!(
                "\n{} passed, {} errors, {} warnings\n",
                report.summary.passed, report.summary.errors, report.summary.warnings
            ));
            buf
        }
    };

    let exit_code = if report.summary.errors > 0 { 1 } else { 0 };
    Ok((content, exit_code))
}
