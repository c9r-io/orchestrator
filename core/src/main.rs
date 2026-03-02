#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![cfg_attr(not(test), deny(clippy::panic, clippy::unwrap_used))]

// Binary-only modules (stay as mod)
mod cli;
mod cli_handler;
mod cli_output;

// Re-export library modules — makes `crate::X` paths work in cli/cli_handler
use agent_orchestrator::anomaly;
use agent_orchestrator::cli_types;
use agent_orchestrator::collab;
use agent_orchestrator::config;
use agent_orchestrator::config_load;
use agent_orchestrator::db;
use agent_orchestrator::db_write;
use agent_orchestrator::dto;
use agent_orchestrator::events;
use agent_orchestrator::events_backfill;
use agent_orchestrator::observability::{init_observability, CliLoggingOverrides};
use agent_orchestrator::resource;
use agent_orchestrator::scheduler;
use agent_orchestrator::scheduler_service;
use agent_orchestrator::session_store;
use agent_orchestrator::state;
use agent_orchestrator::task_ops;
use agent_orchestrator::task_repository;

#[cfg(test)]
mod test_utils;

use crate::cli::{Cli, Commands, DbCommands, ManifestCommands};
use crate::cli_output::{err_line, out_line};
use crate::collab::MessageBus;
use crate::config_load::{
    detect_app_root, load_or_seed_config, load_raw_config_from_db, persist_raw_config,
};
use crate::db::{init_schema, reset_db_by_path};
use crate::resource::{
    dispatch_resource, kind_as_str, parse_resources_from_yaml, ApplyResult, Resource,
};
use crate::state::ManagedState;
use anyhow::{Context, Result};
use clap::Parser;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;
use tracing::{info, warn};

fn init_state() -> Result<ManagedState> {
    let app_root = detect_app_root();
    let (db_path, logs_dir) = initialize_runtime(&app_root)?;

    let (config, _yaml, _version, _updated_at) = load_or_seed_config(&db_path)?;
    let (active, active_config_error, active_config_notice) =
        match config_load::build_active_config_with_self_heal(&app_root, &db_path, config.clone()) {
            Ok((active, report)) => {
            let default_workspace = active
                .workspaces
                .get(&active.default_workspace_id)
                .context("default workspace is missing after config validation")?;
            backfill_legacy_data(
                &db_path,
                &active.default_workspace_id,
                &active.default_workflow_id,
                default_workspace,
            )?;
                (active, None, report)
            }
            Err(error) => (
                placeholder_active_config(config),
                Some(format!(
                    "active config is not runnable; continue applying resources until configuration is complete: {error}"
                )),
                None,
            ),
        };

    let db_writer = Arc::new(crate::db_write::DbWriteCoordinator::new(&db_path)?);
    Ok(ManagedState {
        inner: Arc::new(crate::state::InnerState {
            app_root,
            db_path,
            logs_dir,
            active_config: RwLock::new(active),
            active_config_error: RwLock::new(active_config_error),
            active_config_notice: RwLock::new(active_config_notice),
            running: Mutex::new(std::collections::HashMap::new()),
            agent_health: std::sync::RwLock::new(std::collections::HashMap::new()),
            agent_metrics: std::sync::RwLock::new(std::collections::HashMap::new()),
            message_bus: Arc::new(MessageBus::new()),
            event_sink: std::sync::RwLock::new(Arc::new(crate::events::TracingEventSink::new())),
            db_writer,
        }),
    })
}

fn placeholder_active_config(
    config: crate::config::OrchestratorConfig,
) -> crate::config::ActiveConfig {
    crate::config::ActiveConfig {
        config,
        workspaces: std::collections::HashMap::new(),
        projects: std::collections::HashMap::new(),
        default_project_id: String::new(),
        default_workspace_id: String::new(),
        default_workflow_id: String::new(),
    }
}

fn backfill_legacy_data(
    db_path: &Path,
    default_workspace_id: &str,
    default_workflow_id: &str,
    workspace: &crate::config::ResolvedWorkspace,
) -> Result<()> {
    let conn = crate::db::open_conn(db_path)?;
    let workspace_root = workspace.root_path.to_string_lossy().to_string();
    let qa_targets = serde_json::to_string(&workspace.qa_targets)?;
    conn.execute(
        "UPDATE tasks SET workspace_id = ?1 WHERE workspace_id = ''",
        rusqlite::params![default_workspace_id],
    )?;
    conn.execute(
        "UPDATE tasks SET workflow_id = ?1 WHERE workflow_id = ''",
        rusqlite::params![default_workflow_id],
    )?;
    conn.execute(
        "UPDATE tasks SET workspace_root = ?1 WHERE workspace_root = ''",
        rusqlite::params![workspace_root],
    )?;
    conn.execute(
        "UPDATE tasks SET qa_targets_json = ?1 WHERE qa_targets_json = '' OR qa_targets_json = '[]'",
        rusqlite::params![qa_targets],
    )?;
    conn.execute(
        "UPDATE tasks SET ticket_dir = ?1 WHERE ticket_dir = ''",
        rusqlite::params![workspace.ticket_dir],
    )?;
    conn.execute(
        "UPDATE command_runs SET workspace_id = ?1 WHERE workspace_id = ''",
        rusqlite::params![default_workspace_id],
    )?;
    conn.execute(
        "UPDATE command_runs SET agent_id = 'legacy' WHERE agent_id = ''",
        [],
    )?;
    drop(conn);

    let stats = crate::events_backfill::backfill_event_step_scope(db_path)?;
    if stats.updated > 0 {
        warn!(
            updated = stats.updated,
            scanned = stats.scanned,
            skipped = stats.skipped,
            "[backfill] step_scope: {} legacy events updated ({} scanned, {} skipped)",
            stats.updated,
            stats.scanned,
            stats.skipped
        );
    }
    Ok(())
}

fn resolve_input_path(app_root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        app_root.join(path)
    }
}

fn initialize_runtime(app_root: &Path) -> Result<(PathBuf, PathBuf)> {
    let data_dir = app_root.join("data");
    let logs_dir = data_dir.join("logs");
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create logs dir {}", logs_dir.display()))?;
    let db_path = data_dir.join("agent_orchestrator.db");
    init_schema(&db_path)?;
    Ok((db_path, logs_dir))
}

fn run_apply_preflight(app_root: &Path, file: &str, dry_run: bool) -> Result<i32> {
    let (db_path, _logs_dir) = initialize_runtime(app_root)?;
    let content = read_manifest_input(file)?;
    let resources = parse_resources_from_yaml(&content)?;
    let mut merged_config = load_raw_config_from_db(&db_path)?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();

    let mut has_errors = false;
    let mut applied_results = Vec::new();
    for (index, manifest) in resources.into_iter().enumerate() {
        if let Err(error) = manifest.validate_version() {
            err_line(format!("document {}: {}", index + 1, error));
            has_errors = true;
            continue;
        }

        let registered = match dispatch_resource(manifest) {
            Ok(resource) => resource,
            Err(error) => {
                err_line(format!("document {}: {}", index + 1, error));
                has_errors = true;
                continue;
            }
        };
        if let Err(error) = registered.validate() {
            err_line(format!(
                "{} / {} invalid: {}",
                kind_as_str(registered.kind()),
                registered.name(),
                error
            ));
            has_errors = true;
            continue;
        }

        let result = registered.apply(&mut merged_config);
        applied_results.push(result);
        let action = match result {
            ApplyResult::Created => "created",
            ApplyResult::Configured => "updated",
            ApplyResult::Unchanged => "unchanged",
        };
        if dry_run {
            out_line(format!(
                "{}/{} would be {} (dry run)",
                kind_as_str(registered.kind()),
                registered.name(),
                action
            ));
        } else {
            out_line(format!(
                "{}/{} {}",
                kind_as_str(registered.kind()),
                registered.name(),
                action
            ));
        }
    }

    if has_errors {
        return Ok(1);
    }

    if !dry_run && !applied_results.is_empty() {
        autofill_defaults_for_manifest_mode(&mut merged_config);
        let overview = persist_raw_config(&db_path, merged_config, "cli-apply")?;
        out_line(format!("configuration version: {}", overview.version));
    }

    Ok(0)
}

fn run_manifest_validate_preflight(app_root: &Path, file: &str) -> Result<i32> {
    let (db_path, _logs_dir) = initialize_runtime(app_root)?;
    let content = read_manifest_input(file)?;
    let resources = parse_resources_from_yaml(&content)?;
    let mut merged_config = load_raw_config_from_db(&db_path)?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();

    let mut has_errors = false;
    for (index, manifest) in resources.into_iter().enumerate() {
        if let Err(error) = manifest.validate_version() {
            err_line(format!("document {}: {}", index + 1, error));
            has_errors = true;
            continue;
        }

        let registered = match dispatch_resource(manifest) {
            Ok(resource) => resource,
            Err(error) => {
                err_line(format!("document {}: {}", index + 1, error));
                has_errors = true;
                continue;
            }
        };
        if let Err(error) = registered.validate() {
            err_line(format!(
                "{} / {} invalid: {}",
                kind_as_str(registered.kind()),
                registered.name(),
                error
            ));
            has_errors = true;
            continue;
        }
        registered.apply(&mut merged_config);
    }

    if has_errors {
        return Ok(1);
    }

    autofill_defaults_for_manifest_mode(&mut merged_config);
    match config_load::build_active_config(app_root, merged_config) {
        Ok(_) => {
            out_line("Manifest is valid");
            Ok(0)
        }
        Err(e) => {
            err_line(format!("Error: {}", e));
            Ok(1)
        }
    }
}

fn read_manifest_input(file: &str) -> Result<String> {
    if file == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read manifest from stdin")?;
        return Ok(buf);
    }
    std::fs::read_to_string(file).with_context(|| format!("failed to read manifest file: {}", file))
}

fn autofill_defaults_for_manifest_mode(config: &mut crate::config::OrchestratorConfig) {
    if config.defaults.project.trim().is_empty() {
        config.defaults.project = "default".to_string();
    }

    if config.defaults.workspace.trim().is_empty() {
        if config.workspaces.contains_key("default") {
            config.defaults.workspace = "default".to_string();
        } else {
            let workspaces: BTreeSet<_> = config.workspaces.keys().cloned().collect();
            if let Some(first) = workspaces.into_iter().next() {
                config.defaults.workspace = first;
            }
        }
    }

    if config.defaults.workflow.trim().is_empty() {
        if config.workflows.contains_key("qa_only") {
            config.defaults.workflow = "qa_only".to_string();
        } else {
            let workflows: BTreeSet<_> = config.workflows.keys().cloned().collect();
            if let Some(first) = workflows.into_iter().next() {
                config.defaults.workflow = first;
            }
        }
    }
}

fn try_handle_preflight_command(cli: &Cli) -> Result<Option<i32>> {
    match &cli.command {
        Commands::Init { root, .. } => {
            let app_root = detect_app_root();
            let (db_path, _logs_dir) = initialize_runtime(&app_root)?;
            if let Some(root_path) = root {
                let path = resolve_input_path(&app_root, root_path);
                std::fs::create_dir_all(&path).with_context(|| {
                    format!("failed to create workspace root {}", path.display())
                })?;
            }
            out_line(format!(
                "Orchestrator initialized at {} (sqlite: {})",
                app_root.display(),
                db_path.display()
            ));
            Ok(Some(0))
        }
        Commands::Apply { file, dry_run } => {
            let app_root = detect_app_root();
            Ok(Some(run_apply_preflight(&app_root, file, *dry_run)?))
        }
        Commands::Manifest(ManifestCommands::Validate { file }) => {
            let app_root = detect_app_root();
            Ok(Some(run_manifest_validate_preflight(&app_root, file)?))
        }
        Commands::Db(DbCommands::Reset {
            force,
            include_history,
            include_config,
        }) => {
            if !force {
                err_line("Use --force to confirm database reset");
                return Ok(Some(1));
            }
            let app_root = detect_app_root();
            let (db_path, _logs_dir) = initialize_runtime(&app_root)?;
            reset_db_by_path(&db_path, *include_history, *include_config)?;
            out_line("Database reset completed");
            if *include_config {
                out_line("All config versions deleted (next apply starts from blank)");
            } else if *include_history {
                out_line("Config version history cleared (active version preserved)");
            }
            Ok(Some(0))
        }
        _ => Ok(None),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let app_root = detect_app_root();
    let db_path = app_root.join("data/agent_orchestrator.db");
    let config = load_raw_config_from_db(&db_path)
        .ok()
        .flatten()
        .map(|(cfg, _, _)| cfg);
    let _observability = init_observability(
        &app_root,
        config.as_ref(),
        CliLoggingOverrides {
            verbose: cli.verbose,
            level: cli.log_level.map(Into::into),
            format: cli.log_format.map(Into::into),
        },
    )?;
    info!(app_root = %app_root.display(), "structured logging initialized");
    if let Some(exit_code) = try_handle_preflight_command(&cli)? {
        std::process::exit(exit_code);
    }
    let state = init_state()?;

    cli::run_cli_mode(state.inner.clone(), cli)
}
