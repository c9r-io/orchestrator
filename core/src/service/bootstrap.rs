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
/// When called from outside an async runtime (e.g., a sync CLI `main()`), this
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
            if let Some(default_workspace) = active
                .projects
                .get(crate::config::DEFAULT_PROJECT_ID)
                .and_then(|p| p.workspaces.get("default"))
            {
                backfill_default_scope_data(db_path, "default", "basic", default_workspace)?;
            }
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
    let task_repo = Arc::new(crate::task_repository::AsyncSqliteTaskRepository::new(
        async_database.clone(),
    ));
    let store_manager = crate::store::StoreManager::new(async_database.clone(), app_root.clone());

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
    }
}

fn backfill_default_scope_data(
    db_path: &Path,
    workspace_id: &str,
    workflow_id: &str,
    workspace: &crate::config::ResolvedWorkspace,
) -> Result<()> {
    let conn = crate::db::open_conn(db_path)?;
    let workspace_root = workspace.root_path.to_string_lossy().to_string();
    let qa_targets = serde_json::to_string(&workspace.qa_targets)?;
    conn.execute(
        "UPDATE tasks SET workspace_id = ?1 WHERE workspace_id = ''",
        rusqlite::params![workspace_id],
    )?;
    conn.execute(
        "UPDATE tasks SET workflow_id = ?1 WHERE workflow_id = ''",
        rusqlite::params![workflow_id],
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
        rusqlite::params![workspace_id],
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
    crate::secret_store_crypto::ensure_secret_key(app_root, &db_path)?;
    Ok((db_path, logs_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::read_active_config;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    #[test]
    fn initialize_runtime_creates_logs_dir_and_database() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let (db_path, logs_dir) = initialize_runtime(temp.path()).expect("initialize runtime");

        assert!(db_path.exists());
        assert!(logs_dir.exists());
        assert!(crate::secret_store_crypto::secret_key_path(temp.path()).exists());
    }

    #[test]
    fn blank_database_bootstraps_runnable_state_without_persisted_resources() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let (db_path, logs_dir) = initialize_runtime(temp.path()).expect("initialize runtime");

        let (config, _yaml, version, _updated_at) =
            load_or_seed_config(&db_path).expect("load blank config");
        assert!(config.projects.is_empty());
        assert!(config.custom_resource_definitions.is_empty());
        assert!(config.custom_resources.is_empty());
        assert!(config.resource_store.is_empty());
        assert_eq!(version, 0);
        assert!(
            crate::config_load::load_config(&db_path)
                .expect("load persisted resources")
                .is_none(),
            "synthetic bootstrap config must not be persisted into sqlite"
        );

        let (active, active_config_error, active_config_notice) =
            build_active_config_result(temp.path(), &db_path, config)
                .expect("build active config from blank bootstrap state");
        assert!(active.workspaces.is_empty());
        assert!(active_config_notice.is_none());
        assert!(
            active.projects.contains_key(crate::config::DEFAULT_PROJECT_ID),
            "bootstrap should synthesize the built-in default project"
        );
        assert!(
            active_config_error.is_none(),
            "blank bootstrap state should still be readable so apply/init can proceed"
        );

        let async_database = Arc::new(
            tokio::runtime::Runtime::new()
                .expect("create runtime")
                .block_on(crate::async_database::AsyncDatabase::open(&db_path))
                .expect("open async database"),
        );
        let managed = build_managed_state(
            temp.path().to_path_buf(),
            db_path,
            logs_dir,
            false,
            async_database,
            active,
            active_config_error,
            active_config_notice,
        )
        .expect("build managed state");

        let loaded = read_active_config(&managed.inner).expect("read active config");
        assert!(loaded.workspaces.is_empty());
        assert!(loaded.projects.contains_key(crate::config::DEFAULT_PROJECT_ID));
    }

    #[test]
    fn placeholder_active_config_keeps_config_but_clears_resolved_views() {
        let config = crate::config::OrchestratorConfig::default();
        let placeholder = placeholder_active_config(config.clone());

        assert_eq!(placeholder.config.projects.len(), config.projects.len());
        assert!(placeholder.projects.is_empty());
        assert!(placeholder.workspaces.is_empty());
    }

    #[test]
    fn build_managed_state_populates_expected_subsystems() {
        let mut fixture = TestState::new();
        let seeded = fixture.build();
        let managed = build_managed_state(
            seeded.app_root.clone(),
            seeded.db_path.clone(),
            seeded.logs_dir.clone(),
            true,
            seeded.async_database.clone(),
            crate::config_load::read_active_config(&seeded)
                .expect("read active config")
                .clone(),
            Some("config-error".to_string()),
            None,
        )
        .expect("build managed state");

        assert!(managed.inner.unsafe_mode);
        assert_eq!(managed.inner.app_root, seeded.app_root);
        assert!(
            managed
                .inner
                .active_config_error
                .read()
                .expect("read config error")
                .as_deref()
                == Some("config-error")
        );
    }

    #[test]
    fn backfill_default_scope_data_updates_blank_task_and_command_run_fields() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/bootstrap-backfill.md");
        std::fs::write(&qa_file, "# bootstrap backfill\n").expect("seed qa file");
        let created = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");

        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET workspace_id = '', workflow_id = '', workspace_root = '', qa_targets_json = '[]', ticket_dir = '' WHERE id = ?1",
            rusqlite::params![created.id],
        )
        .expect("blank task fields");
        let item_id: String = conn
            .query_row(
                "SELECT id FROM task_items WHERE task_id = ?1 LIMIT 1",
                rusqlite::params![created.id],
                |row| row.get(0),
            )
            .expect("load item id");
        tokio::runtime::Runtime::new()
            .expect("create runtime")
            .block_on(
                state
                    .task_repo
                    .insert_command_run(crate::task_repository::NewCommandRun {
                        id: "bootstrap-run".to_string(),
                        task_item_id: item_id,
                        phase: "qa".to_string(),
                        command: "echo bootstrap".to_string(),
                        cwd: state.app_root.display().to_string(),
                        workspace_id: "".to_string(),
                        agent_id: "echo".to_string(),
                        exit_code: 0,
                        stdout_path: state
                            .app_root
                            .join("logs/bootstrap-stdout.log")
                            .display()
                            .to_string(),
                        stderr_path: state
                            .app_root
                            .join("logs/bootstrap-stderr.log")
                            .display()
                            .to_string(),
                        started_at: crate::config_load::now_ts(),
                        ended_at: crate::config_load::now_ts(),
                        interrupted: 0,
                        output_json: "{}".to_string(),
                        artifacts_json: "[]".to_string(),
                        confidence: Some(1.0),
                        quality_score: Some(1.0),
                        validation_status: "passed".to_string(),
                        session_id: None,
                        machine_output_source: "stdout".to_string(),
                        output_json_path: None,
                    }),
            )
            .expect("insert command run");

        let active = crate::config_load::read_active_config(&state).expect("read active config");
        let workspace = active
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .and_then(|project| project.workspaces.get("default"))
            .expect("default workspace");
        backfill_default_scope_data(&state.db_path, "default", "basic", workspace)
            .expect("backfill default scope");

        let task_row: (String, String, String, String, String) = conn
            .query_row(
                "SELECT workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir FROM tasks WHERE id = ?1",
                rusqlite::params![created.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("load backfilled task");
        assert_eq!(task_row.0, "default");
        assert_eq!(task_row.1, "basic");
        assert!(!task_row.2.is_empty());
        assert!(task_row.3.contains("docs/qa"));
        assert_eq!(task_row.4, "docs/ticket");

        let workspace_id: String = conn
            .query_row(
                "SELECT workspace_id FROM command_runs WHERE id = 'bootstrap-run'",
                [],
                |row| row.get(0),
            )
            .expect("load command run workspace");
        assert_eq!(workspace_id, "default");
    }
}
