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

use crate::collab::MessageBus;
use crate::config_load::read_active_config;
use crate::config_load::{detect_app_root, load_or_seed_config};
use crate::db::init_schema;
use crate::cli::Cli;
use clap::Parser;
use crate::state::ManagedState;
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

fn init_state(cli_config_path: Option<String>) -> Result<ManagedState> {
    let app_root = detect_app_root();
    let config_path = match cli_config_path {
        Some(p) => {
            if std::path::Path::new(&p).is_absolute() {
                std::path::PathBuf::from(p)
            } else {
                app_root.join(p)
            }
        }
        None => app_root.join("config/default.yaml"),
    };
    let data_dir = app_root.join("data");
    let logs_dir = data_dir.join("logs");
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create logs dir {}", logs_dir.display()))?;

    let db_path = data_dir.join("agent_orchestrator.db");
    init_schema(&db_path)?;

    let (config, _yaml, _version, _updated_at) = load_or_seed_config(&db_path, &config_path)?;
    let active = config_load::build_active_config(&app_root, config)?;
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let state = init_state(cli.config.clone())?;

    // Ensure config can be loaded before command dispatch.
    drop(read_active_config(&state.inner)?);

    cli::run_cli_mode(state.inner.clone(), cli)
}
