#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod cli_handler;
mod cli_types;
mod collab;
mod config;
mod config_load;
mod config_validation;
mod db;
mod dto;
mod dynamic_orchestration;
mod events;
mod health;
mod metrics;
mod prehook;
mod qa_utils;
mod resource;
mod scheduler;
mod selection;
mod state;
mod task_ops;
mod ticket;

#[cfg(test)]
mod test_utils;

use crate::cli::{Cli, Commands, ConfigCommands};
use crate::cli_types::OrchestratorResource;
use crate::collab::MessageBus;
use crate::config_load::read_active_config;
use crate::config_load::{
    detect_app_root, load_or_seed_config, load_raw_config_from_db, persist_raw_config,
};
use crate::db::init_schema;
use crate::resource::{dispatch_resource, ApplyResult, RegisteredResource, Resource};
use crate::state::ManagedState;
use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

fn init_state(cli_config_path: Option<String>) -> Result<ManagedState> {
    let app_root = detect_app_root();
    let seed_config_path = cli_config_path
        .as_ref()
        .map(|p| resolve_input_path(&app_root, p));
    let config_path = seed_config_path
        .clone()
        .unwrap_or_else(|| app_root.join("config/default.yaml"));
    let (db_path, logs_dir) = initialize_runtime(&app_root)?;

    let (config, _yaml, _version, _updated_at) =
        load_or_seed_config(&db_path, seed_config_path.as_deref())?;
    let active = config_load::build_active_config(&app_root, config).with_context(|| {
        "active config is not runnable; continue applying resources or use a complete config bootstrap"
    })?;
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

    Ok(ManagedState {
        inner: Arc::new(crate::state::InnerState {
            app_root,
            db_path,
            logs_dir,
            config_path,
            active_config: RwLock::new(active),
            running: Mutex::new(std::collections::HashMap::new()),
            agent_health: std::sync::RwLock::new(std::collections::HashMap::new()),
            agent_metrics: std::sync::RwLock::new(std::collections::HashMap::new()),
            message_bus: Arc::new(MessageBus::new()),
            event_sink: std::sync::RwLock::new(Arc::new(crate::events::NoopSink)),
        }),
    })
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

fn parse_resources_from_yaml(content: &str) -> Result<Vec<OrchestratorResource>> {
    let mut resources = Vec::new();
    for document in serde_yaml::Deserializer::from_str(content) {
        let value = serde_yaml::Value::deserialize(document)?;
        if value.is_null() {
            continue;
        }
        let resource = serde_yaml::from_value::<OrchestratorResource>(value)?;
        resources.push(resource);
    }
    Ok(resources)
}

fn apply_resource_to_config(
    config: &mut crate::config::OrchestratorConfig,
    resource: &RegisteredResource,
) -> ApplyResult {
    match resource {
        RegisteredResource::Workspace(current) => {
            let existed = config.workspaces.contains_key(current.name());
            let _ = resource.apply(config);
            if existed {
                ApplyResult::Configured
            } else {
                ApplyResult::Created
            }
        }
        RegisteredResource::Agent(current) => {
            let existed = config.agents.contains_key(current.name());
            let _ = resource.apply(config);
            if existed {
                ApplyResult::Configured
            } else {
                ApplyResult::Created
            }
        }
        RegisteredResource::Workflow(current) => {
            let existed = config.workflows.contains_key(current.name());
            let _ = resource.apply(config);
            if existed {
                ApplyResult::Configured
            } else {
                ApplyResult::Created
            }
        }
    }
}

fn kind_as_str(kind: crate::cli_types::ResourceKind) -> &'static str {
    match kind {
        crate::cli_types::ResourceKind::Workspace => "workspace",
        crate::cli_types::ResourceKind::Agent => "agent",
        crate::cli_types::ResourceKind::Workflow => "workflow",
    }
}

fn run_apply_preflight(app_root: &Path, file: &str, dry_run: bool) -> Result<i32> {
    let (db_path, _logs_dir) = initialize_runtime(app_root)?;
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read manifest file: {}", file))?;
    let resources = parse_resources_from_yaml(&content)?;
    let mut merged_config = load_raw_config_from_db(&db_path)?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();

    let mut has_errors = false;
    let mut applied_results = Vec::new();
    for (index, manifest) in resources.into_iter().enumerate() {
        if let Err(error) = manifest.validate_version() {
            eprintln!("document {}: {}", index + 1, error);
            has_errors = true;
            continue;
        }

        let registered = match dispatch_resource(manifest) {
            Ok(resource) => resource,
            Err(error) => {
                eprintln!("document {}: {}", index + 1, error);
                has_errors = true;
                continue;
            }
        };
        if let Err(error) = registered.validate() {
            eprintln!(
                "{} / {} invalid: {}",
                kind_as_str(registered.kind()),
                registered.name(),
                error
            );
            has_errors = true;
            continue;
        }

        let result = apply_resource_to_config(&mut merged_config, &registered);
        applied_results.push(result);
        let action = match result {
            ApplyResult::Created => "created",
            ApplyResult::Configured | ApplyResult::Unchanged => "configured",
        };
        if dry_run {
            println!(
                "{}/{} would be {} (dry run)",
                kind_as_str(registered.kind()),
                registered.name(),
                action
            );
        } else {
            println!(
                "{}/{} {}",
                kind_as_str(registered.kind()),
                registered.name(),
                action
            );
        }
    }

    if has_errors {
        return Ok(1);
    }

    if !dry_run && !applied_results.is_empty() {
        let overview = persist_raw_config(&db_path, merged_config, "cli-apply")?;
        println!("configuration version: {}", overview.version);
    }

    Ok(0)
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
            println!(
                "Orchestrator initialized at {} (sqlite: {})",
                app_root.display(),
                db_path.display()
            );
            Ok(Some(0))
        }
        Commands::Config(ConfigCommands::Bootstrap { from_file, force }) => {
            let app_root = detect_app_root();
            let (db_path, _logs_dir) = initialize_runtime(&app_root)?;
            let source_path = resolve_input_path(&app_root, from_file);
            let overview = config_load::bootstrap_config_from_file(
                &app_root,
                &db_path,
                &source_path,
                *force,
                "cli-bootstrap",
            )?;
            println!(
                "Configuration bootstrapped from {} (version {})",
                source_path.display(),
                overview.version
            );
            Ok(Some(0))
        }
        Commands::Apply { file, dry_run } => {
            let app_root = detect_app_root();
            Ok(Some(run_apply_preflight(&app_root, file, *dry_run)?))
        }
        Commands::Config(ConfigCommands::Validate { config_file }) => {
            let app_root = detect_app_root();
            let content = std::fs::read_to_string(config_file)
                .with_context(|| format!("cannot read config file: {}", config_file))?;

            let validator =
                crate::config_validation::validator::ConfigValidator::new(&app_root)
                    .with_level(crate::config_validation::ValidationLevel::Full);
            let result = validator.validate_yaml(&content);

            if !result.warnings.is_empty() || !result.errors.is_empty() {
                eprintln!("{}", result.report());
            }

            if !result.is_valid {
                return Ok(Some(1));
            }

            let config: crate::config::OrchestratorConfig = serde_yaml::from_str(&content)?;
            match config_load::build_active_config(&app_root, config) {
                Ok(candidate) => {
                    let normalized = serde_yaml::to_string(&candidate.config)?;
                    println!("Configuration is valid:\n{}", normalized);
                    Ok(Some(0))
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    Ok(Some(1))
                }
            }
        }
        _ => Ok(None),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(exit_code) = try_handle_preflight_command(&cli)? {
        std::process::exit(exit_code);
    }
    let state = init_state(cli.config.clone())?;

    // Ensure config can be loaded before command dispatch.
    drop(read_active_config(&state.inner)?);

    cli::run_cli_mode(state.inner.clone(), cli)
}
