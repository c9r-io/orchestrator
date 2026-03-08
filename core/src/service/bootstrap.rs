use crate::collab::MessageBus;
use crate::config_load::{
    build_active_config_with_self_heal, detect_app_root, load_or_seed_config,
};
use crate::db::init_schema;
use crate::state::{InnerState, ManagedState};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

/// Initialize orchestrator state — extracted from the former binary's `init_state()`.
/// This is the single entry point for both daemon and any future standalone usage.
///
/// When called from outside an async runtime (e.g., legacy CLI `main()`), this
/// creates a temporary runtime for async DB initialization. When called from
/// within an async runtime (e.g., daemon), use `init_state_async()` instead.
pub fn init_state(unsafe_mode: bool) -> Result<ManagedState> {
    let app_root = detect_app_root();
    let (db_path, logs_dir) = initialize_runtime(&app_root)?;

    let (config, _yaml, _version, _updated_at) = load_or_seed_config(&db_path)?;
    let (active, active_config_error, active_config_notice) =
        build_active_config_result(&app_root, &db_path, config)?;

    let async_database = Arc::new(
        tokio::runtime::Runtime::new()
            .context("failed to create tokio runtime for async db init")?
            .block_on(crate::async_database::AsyncDatabase::open(&db_path))
            .context("failed to open async database")?,
    );

    build_managed_state(
        app_root,
        db_path,
        logs_dir,
        unsafe_mode,
        async_database,
        active,
        active_config_error,
        active_config_notice,
    )
}

/// Async variant of `init_state` — safe to call from within an existing tokio runtime.
pub async fn init_state_async(unsafe_mode: bool) -> Result<ManagedState> {
    let app_root = detect_app_root();
    let (db_path, logs_dir) = initialize_runtime(&app_root)?;

    let (config, _yaml, _version, _updated_at) = load_or_seed_config(&db_path)?;
    let (active, active_config_error, active_config_notice) =
        build_active_config_result(&app_root, &db_path, config)?;

    let async_database = Arc::new(
        crate::async_database::AsyncDatabase::open(&db_path)
            .await
            .context("failed to open async database")?,
    );

    build_managed_state(
        app_root,
        db_path,
        logs_dir,
        unsafe_mode,
        async_database,
        active,
        active_config_error,
        active_config_notice,
    )
}

fn build_active_config_result(
    app_root: &Path,
    db_path: &Path,
    config: crate::config::OrchestratorConfig,
) -> Result<(
    crate::config::ActiveConfig,
    Option<String>,
    Option<crate::config_load::ConfigSelfHealReport>,
)> {
    match build_active_config_with_self_heal(app_root, db_path, config.clone()) {
        Ok((active, report)) => {
            let default_workspace = active
                .workspaces
                .get(&active.default_workspace_id)
                .context("default workspace is missing after config validation")?;
            backfill_legacy_data(
                db_path,
                &active.default_workspace_id,
                &active.default_workflow_id,
                default_workspace,
            )?;
            Ok((active, None, report))
        }
        Err(error) => Ok((
            placeholder_active_config(config),
            Some(format!(
                "active config is not runnable; continue applying resources until configuration is complete: {error}"
            )),
            None,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_managed_state(
    app_root: std::path::PathBuf,
    db_path: std::path::PathBuf,
    logs_dir: std::path::PathBuf,
    unsafe_mode: bool,
    async_database: Arc<crate::async_database::AsyncDatabase>,
    active: crate::config::ActiveConfig,
    active_config_error: Option<String>,
    active_config_notice: Option<crate::config_load::ConfigSelfHealReport>,
) -> Result<ManagedState> {
    let db_writer = Arc::new(crate::db_write::DbWriteCoordinator::new(
        async_database.clone(),
    ));
    let session_store = Arc::new(crate::session_store::AsyncSessionStore::new(
        async_database.clone(),
    ));
    let task_repo = Arc::new(
        crate::task_repository::AsyncSqliteTaskRepository::new(async_database.clone()),
    );
    let store_manager =
        crate::store::StoreManager::new(async_database.clone(), app_root.clone());

    Ok(ManagedState {
        inner: Arc::new(InnerState {
            app_root,
            db_path,
            unsafe_mode,
            async_database,
            logs_dir,
            active_config: RwLock::new(active),
            active_config_error: RwLock::new(active_config_error),
            active_config_notice: RwLock::new(active_config_notice),
            running: Mutex::new(HashMap::new()),
            agent_health: std::sync::RwLock::new(HashMap::new()),
            agent_metrics: std::sync::RwLock::new(HashMap::new()),
            message_bus: Arc::new(MessageBus::new()),
            event_sink: std::sync::RwLock::new(Arc::new(crate::events::TracingEventSink::new())),
            db_writer,
            session_store,
            task_repo,
            store_manager,
        }),
    })
}

fn placeholder_active_config(
    config: crate::config::OrchestratorConfig,
) -> crate::config::ActiveConfig {
    crate::config::ActiveConfig {
        config,
        workspaces: HashMap::new(),
        projects: HashMap::new(),
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
    drop(conn);
    Ok(())
}

fn initialize_runtime(app_root: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let data_dir = app_root.join("data");
    let logs_dir = data_dir.join("logs");
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create logs dir {}", logs_dir.display()))?;
    let db_path = data_dir.join("agent_orchestrator.db");
    init_schema(&db_path)?;
    Ok((db_path, logs_dir))
}
