#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
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
mod ticket;

#[cfg(test)]
mod test_utils;

use crate::collab::{parse_artifacts_from_output, AgentOutput, Artifact, ExecutionMetrics, MessageBus};
use crate::config_load::{detect_app_root, load_or_seed_config, now_ts};
use crate::db::init_schema;
use crate::dto::CliOptions;
use crate::state::ManagedState;
use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

fn print_cli_help(binary_name: &str) {
    println!("Agent Orchestrator CLI");
    println!();
    println!(
        "Usage: {} --cli [--task-id ID] [--workspace ID] [--workflow ID] [--name NAME] [--goal GOAL] [--target-file PATH]... [--no-auto-resume]",
        binary_name
    );
}

fn parse_cli_options(args: &[String]) -> Result<CliOptions> {
    let mut options = CliOptions::new();
    let mut idx = 0usize;
    let mut seen_cli = false;

    while idx < args.len() {
        match args[idx].as_str() {
            "--cli" => {
                options.cli = true;
                seen_cli = true;
                idx += 1;
            }
            "--help" | "-h" => {
                if !seen_cli {
                    options.show_help = true;
                }
                idx += 1;
            }
            "--no-auto-resume" => {
                options.no_auto_resume = true;
                idx += 1;
            }
            "--task-id" => {
                let value = args.get(idx + 1).context("missing value for --task-id")?;
                options.task_id = Some(value.clone());
                idx += 2;
            }
            "--workspace" => {
                let value = args.get(idx + 1).context("missing value for --workspace")?;
                options.workspace_id = Some(value.clone());
                idx += 2;
            }
            "--project" => {
                let value = args.get(idx + 1).context("missing value for --project")?;
                options.project_id = Some(value.clone());
                idx += 2;
            }
            "--workflow" => {
                let value = args.get(idx + 1).context("missing value for --workflow")?;
                options.workflow_id = Some(value.clone());
                idx += 2;
            }
            "--name" => {
                let value = args.get(idx + 1).context("missing value for --name")?;
                options.name = Some(value.clone());
                idx += 2;
            }
            "--goal" => {
                let value = args.get(idx + 1).context("missing value for --goal")?;
                options.goal = Some(value.clone());
                idx += 2;
            }
            "--target-file" => {
                let value = args
                    .get(idx + 1)
                    .context("missing value for --target-file")?;
                options.target_files.push(value.clone());
                idx += 2;
            }
            _ => {
                idx += 1;
            }
        }
    }

    Ok(options)
}

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

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let binary_name = args.first().map(|s| s.as_str()).unwrap_or("agent-orchestrator");

    let cli_options = parse_cli_options(&args[1..])?;

    if cli_options.show_help {
        print_cli_help(binary_name);
        return Ok(());
    }

    let state = init_state(None)?;

    if cli_options.cli {
        cli::run_cli_mode(state.inner.clone(), cli_options).await?;
    } else {
        tauri::Builder::default()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                api::bootstrap,
                api::get_create_task_options,
                api::get_config_overview,
                api::save_config_from_form,
                api::save_config_from_yaml,
                api::validate_config_yaml,
                api::list_config_versions,
                api::get_config_version,
                api::create_task,
                api::list_tasks,
                api::get_task_details,
                api::start_task,
                api::pause_task,
                api::resume_task,
                api::retry_task_item,
                api::delete_task,
                api::stream_task_logs,
                api::simulate_prehook,
                api::get_agent_health,
            ])
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    }

    Ok(())
}
