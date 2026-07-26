//! End-to-end round trip through the extracted persistence layer.
//!
//! FR-130 Phase A moved this crate out of `core`. The structural evidence for
//! that move — reference counts falling, symbols absent from `core/src`, the
//! member appearing in a ledger — would all be equally true of a layer that
//! persists nothing. So would `schema-snapshot.sql` staying byte-identical:
//! that proves the *schema* survived, not that anything can be written to it
//! and read back.
//!
//! This is the assertion that closes that gap. It bootstraps a real database
//! through the whole registered migration chain and then drives one task
//! through every module the extraction touched — `sqlite`, `schema`,
//! `async_database`, `task_repository`, `db_write`, `session_store` and `db` —
//! reading each write back through a different module than wrote it, so a
//! module that silently no-ops is not covered for by its own reader.
//!
//! The one thing seeded with raw SQL is the initial `tasks` row. Task creation
//! is domain logic and lives in `core::task_ops`, above this layer; core's own
//! `task_repository` tests drive it from there and assert the same round trip
//! from the other side.

use std::sync::Arc;

use orchestrator_persistence::async_database::AsyncDatabase;
use orchestrator_persistence::db;
use orchestrator_persistence::db_write::DbWriteCoordinator;
use orchestrator_persistence::schema::PersistenceBootstrap;
use orchestrator_persistence::session_store::{self, NewSession};
use orchestrator_persistence::sqlite::open_conn;
use orchestrator_persistence::task_repository::{
    AsyncSqliteTaskRepository, SqliteTaskRepository, trait_def::TaskItemMutRepository,
    trait_def::TaskQueryRepository, types::TaskRepositorySource,
};

const TASK_ID: &str = "task-round-trip";
const ITEM_ID: &str = "item-round-trip";
const WORKSPACE: &str = "workspace-round-trip";

fn seed_task(conn: &rusqlite::Connection) {
    let now = orchestrator_persistence::now_ts();
    conn.execute(
        "INSERT INTO tasks (
            id, name, status, goal, target_files_json, mode, workspace_id, workflow_id,
            project_id, workspace_root, qa_targets_json, ticket_dir, created_at, updated_at
         ) VALUES (?1, 'round trip', 'pending', 'prove the layer persists', '[]', 'qa',
                   ?2, 'wf-round-trip', 'default', '/tmp/round-trip', '[]', '/tmp/tickets', ?3, ?3)",
        rusqlite::params![TASK_ID, WORKSPACE, now],
    )
    .expect("seed task row");
    conn.execute(
        "INSERT INTO task_items (
            id, task_id, order_no, qa_file_path, status, ticket_files_json,
            ticket_content_json, created_at, updated_at
         ) VALUES (?1, ?2, 0, 'docs/qa/round-trip.md', 'pending', '[]', '{}', ?3, ?3)",
        rusqlite::params![ITEM_ID, TASK_ID, now],
    )
    .expect("seed task item row");
}

#[tokio::test]
async fn a_task_written_through_the_layer_reads_back_through_the_layer() {
    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("round-trip.db");

    // The whole registered chain, not a hand-built subset: the round trip has to
    // run against the schema production runs against.
    let status = PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap");
    assert!(
        status.current_version > 0,
        "bootstrap left the schema at version 0"
    );

    let conn = open_conn(&db_path).expect("open connection");
    seed_task(&conn);

    // Written synchronously, read back asynchronously.
    let sync_repo = SqliteTaskRepository::new(TaskRepositorySource::Path(db_path.clone()));
    sync_repo
        .set_task_item_terminal_status(ITEM_ID, "qa_passed")
        .expect("set terminal status");

    let async_db = Arc::new(AsyncDatabase::open(&db_path).await.expect("open async db"));
    let async_repo = AsyncSqliteTaskRepository::new(async_db.clone());

    let summary = async_repo
        .load_task_summary(TASK_ID)
        .await
        .expect("load task summary");
    assert_eq!(summary.id, TASK_ID);
    assert_eq!(summary.workspace_id, WORKSPACE);

    let items = async_repo
        .list_task_items_for_cycle(TASK_ID)
        .await
        .expect("list items");
    assert_eq!(items.len(), 1, "the seeded item did not come back");
    assert_eq!(
        items[0].status, "qa_passed",
        "the synchronous write is not visible to the asynchronous reader"
    );

    // Written through db_write, read back through the repository.
    let coordinator = DbWriteCoordinator::new(async_db.clone());
    coordinator
        .insert_event(TASK_ID, Some(ITEM_ID), "round_trip", r#"{"step":"one"}"#)
        .await
        .expect("insert event");
    let (_items, _runs, events, _bundles) = async_repo
        .load_task_detail_rows(TASK_ID)
        .await
        .expect("load task detail rows");
    assert!(
        events.iter().any(|event| event.event_type == "round_trip"),
        "the event written through db_write did not come back through the repository"
    );

    // session_store writes with its own synchronous connection; read it back on
    // a different one.
    let session = NewSession {
        id: "session-round-trip",
        task_id: TASK_ID,
        task_item_id: Some(ITEM_ID),
        step_id: "step-1",
        phase: "qa",
        agent_id: "agent-round-trip",
        state: "running",
        pid: 0,
        pty_backend: "none",
        cwd: "/tmp/round-trip",
        command: "true",
        input_fifo_path: "/tmp/round-trip/in",
        stdout_path: "/tmp/round-trip/out",
        stderr_path: "/tmp/round-trip/err",
        transcript_path: "/tmp/round-trip/transcript",
        output_json_path: None,
    };
    session_store::insert_session(&conn, &session).expect("insert session");

    let reopened = open_conn(&db_path).expect("reopen connection");
    let loaded = session_store::load_session(&reopened, "session-round-trip")
        .expect("load session")
        .expect("the session was not persisted");
    assert_eq!(loaded.task_id, TASK_ID);
    assert_eq!(loaded.agent_id, "agent-round-trip");

    // The admin facade sees the same database.
    let facade = open_conn(&db_path).expect("open a connection for the facade");
    let non_terminal = db::count_non_terminal_tasks_by_workspace(&facade, "default", WORKSPACE)
        .expect("count tasks");
    assert_eq!(
        non_terminal, 1,
        "the admin facade does not see the task the repository wrote"
    );
}

/// Phase B moves SQL out of core one statement at a time. Each moved statement
/// gets an assertion here, because the ledger entry disappearing proves the
/// reference moved and says nothing about whether the statement still does what
/// its caller needs. The negative half matters as much: `DELETE` against a
/// non-matching predicate succeeds and affects zero rows, so a function that
/// deleted the wrong project — or nothing at all — would satisfy a test that
/// only checked it returned `Ok`.
#[tokio::test]
async fn delete_project_resources_removes_one_project_and_leaves_the_others() {
    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("resources.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap");

    let conn = open_conn(&db_path).expect("open connection");
    let now = orchestrator_persistence::now_ts();
    for (project, name) in [("doomed", "a"), ("doomed", "b"), ("kept", "c")] {
        conn.execute(
            "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json,
                                    created_at, updated_at)
             VALUES ('Agent', ?1, ?2, 'orchestrator.dev/v2', '{}', '{}', ?3, ?3)",
            rusqlite::params![project, name, now],
        )
        .expect("seed resource row");
    }

    let removed = db::delete_project_resources(&db_path, "doomed").expect("delete");
    assert_eq!(removed, 2, "the reported row count is not what was deleted");

    let reopened = open_conn(&db_path).expect("reopen");
    let remaining: Vec<String> = {
        let mut statement = reopened
            .prepare("SELECT project || '/' || name FROM resources ORDER BY name")
            .expect("prepare");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect")
    };
    assert_eq!(
        remaining,
        vec!["kept/c".to_string()],
        "deleting one project's resources did not leave the other project's alone"
    );

    let again = db::delete_project_resources(&db_path, "doomed").expect("delete again");
    assert_eq!(
        again, 0,
        "a second delete reported rows it could not have removed"
    );
}

/// The terminal-task retention query moved out of `core::task_cleanup` in
/// FR-130 Phase B. Its contract is the filter, not the `Ok`: a query that
/// returned every task would satisfy a test that only checked the call
/// succeeded, and auto-cleanup would then delete running work.
#[tokio::test]
async fn the_retention_query_selects_only_old_terminal_tasks() {
    use orchestrator_persistence::task_repository::queries;

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("retention.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap");
    let conn = open_conn(&db_path).expect("open connection");

    // (id, status, age in days). The three exclusions are each a different
    // reason: wrong status, too recent, and both.
    for (id, status, age_days) in [
        ("old-completed", "completed", 30),
        ("old-failed", "failed", 30),
        ("old-cancelled", "cancelled", 30),
        ("old-running", "running", 30),
        ("new-completed", "completed", 0),
        ("new-running", "running", 0),
    ] {
        conn.execute(
            "INSERT INTO tasks (
                id, name, status, goal, target_files_json, mode, workspace_id, workflow_id,
                project_id, workspace_root, qa_targets_json, ticket_dir, created_at, updated_at
             ) VALUES (?1, ?1, ?2, '', '[]', 'auto', 'default', 'basic', 'default',
                       '/tmp', '[]', '/tmp/tickets', datetime('now'),
                       datetime('now', ?3))",
            rusqlite::params![id, status, format!("-{age_days} days")],
        )
        .expect("seed task");
    }

    let mut selected =
        queries::list_terminal_tasks_older_than(&conn, 7, 50).expect("retention query");
    selected.sort();
    assert_eq!(
        selected,
        vec![
            "old-cancelled".to_string(),
            "old-completed".to_string(),
            "old-failed".to_string()
        ],
        "the retention query selected the wrong set"
    );

    let capped = queries::list_terminal_tasks_older_than(&conn, 7, 2).expect("capped query");
    assert_eq!(capped.len(), 2, "LIMIT was not applied");

    let none = queries::list_terminal_tasks_older_than(&conn, 365, 50).expect("wide window");
    assert!(
        none.is_empty(),
        "a 365-day retention window still selected tasks 30 days old"
    );
}

/// The step-event row query moved out of `core::events` in FR-130 Phase B, and
/// its event-type filter moved *up* into core as a constant. Both halves of that
/// need pinning: the query must honour the list it is given, and must not carry
/// its own.
#[tokio::test]
async fn step_event_rows_honour_the_event_type_list_they_are_given() {
    use orchestrator_persistence::events::step_event_rows;

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("events.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap");
    let conn = open_conn(&db_path).expect("open connection");
    seed_task(&conn);

    for kind in ["step_started", "step_finished", "task_created"] {
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, '{}', datetime('now'))",
            rusqlite::params![TASK_ID, kind],
        )
        .expect("seed event");
    }

    let selected = step_event_rows(&conn, TASK_ID, &["step_started"]).expect("query");
    assert_eq!(
        selected
            .iter()
            .map(|row| row.event_type.as_str())
            .collect::<Vec<_>>(),
        ["step_started"],
        "the query did not restrict itself to the requested event type"
    );

    let two = step_event_rows(&conn, TASK_ID, &["step_started", "task_created"]).expect("query");
    assert_eq!(two.len(), 2, "a two-type list did not return both types");

    // The list is the caller's policy, so an empty list means nothing — not
    // everything, which is what a query carrying its own WHERE clause would do.
    let none = step_event_rows(&conn, TASK_ID, &[]).expect("query");
    assert!(none.is_empty(), "an empty event-type list returned rows");
}

/// The negative half. Every assertion above runs against a bootstrapped
/// database, so all of them would also pass if `PersistenceBootstrap` were the
/// only thing still working. Against a database that never ran the chain, the
/// layer must fail loudly rather than return an empty, plausible answer — which
/// is the failure a round trip on its own cannot distinguish from success.
#[tokio::test]
async fn the_layer_fails_loudly_on_a_database_that_never_migrated() {
    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("unmigrated.db");
    let _ = open_conn(&db_path).expect("create an empty database");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::Path(db_path.clone()));
    assert!(
        repo.load_task_summary(TASK_ID).is_err(),
        "reading a task from an unmigrated database succeeded"
    );
    let conn = open_conn(&db_path).expect("open the unmigrated database");
    assert!(
        db::count_non_terminal_tasks_by_workspace(&conn, "default", WORKSPACE).is_err(),
        "counting tasks in an unmigrated database succeeded"
    );
}

/// FR-130 B7: the six scope-backfill statements moved out of
/// `core::service::bootstrap`. Bootstrap runs them on every start, so the
/// property that matters is not that blank columns get filled — it is that
/// non-blank ones do not. A statement that dropped its `WHERE` clause would
/// still pass a "the blank row was filled" assertion while silently rewriting
/// every task in the database to the default workspace.
#[test]
fn the_scope_backfill_fills_blank_columns_and_leaves_the_rest_alone() {
    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("backfill.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    let conn = open_conn(&db_path).expect("open database");
    seed_task(&conn);

    // A second task with every scope column blank, plus the `'[]'` form that the
    // qa-targets statement treats as blank while the other four do not.
    let now = orchestrator_persistence::now_ts();
    conn.execute(
        "INSERT INTO tasks (
            id, name, status, goal, target_files_json, mode, workspace_id, workflow_id,
            project_id, workspace_root, qa_targets_json, ticket_dir, created_at, updated_at
         ) VALUES ('task-blank', 'blank scope', 'pending', 'be backfilled', '[]', 'qa',
                   '', '', 'default', '', '[]', '', ?1, ?1)",
        rusqlite::params![now],
    )
    .expect("seed blank task");
    conn.execute(
        "INSERT INTO command_runs (
            id, task_item_id, phase, command, cwd, workspace_id, stdout_path, stderr_path,
            started_at
         ) VALUES ('run-blank', ?1, 'qa', 'echo blank', '/tmp', '', '/tmp/o.log', '/tmp/e.log', ?2)",
        rusqlite::params![ITEM_ID, now],
    )
    .expect("seed blank command run");

    let values = db::DefaultScopeBackfill {
        workspace_id: "default",
        workflow_id: "basic",
        workspace_root: "/srv/workspace/default",
        qa_targets_json: r#"["docs/qa"]"#,
        ticket_dir: "docs/ticket",
    };
    let counts = db::backfill_blank_default_scope(&db_path, &values).expect("backfill");

    // Exactly one row per statement: the blank task and the blank command run.
    // `seed_task`'s row is already scoped, so a count of 2 anywhere means the
    // predicate stopped discriminating.
    assert_eq!(
        counts,
        db::ScopeBackfillCounts {
            tasks_workspace_id: 1,
            tasks_workflow_id: 1,
            tasks_workspace_root: 1,
            tasks_qa_targets_json: 2,
            tasks_ticket_dir: 1,
            command_runs_workspace_id: 1,
        },
        "the backfill claimed a different set of rows than the blank ones"
    );

    let backfilled: (String, String, String, String, String) = conn
        .query_row(
            "SELECT workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir
               FROM tasks WHERE id = 'task-blank'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("read the backfilled task");
    assert_eq!(
        backfilled,
        (
            "default".to_string(),
            "basic".to_string(),
            "/srv/workspace/default".to_string(),
            r#"["docs/qa"]"#.to_string(),
            "docs/ticket".to_string(),
        ),
        "the blank task was not filled with the values it was given"
    );

    // The already-scoped row keeps its own workspace. `qa_targets_json` is the
    // one column it does lose, because `'[]'` is what the seed wrote and the
    // statement reads that as blank — recorded here rather than asserted around,
    // since it is the shipped behaviour.
    let untouched: (String, String) = conn
        .query_row(
            "SELECT workspace_id, workflow_id FROM tasks WHERE id = ?1",
            rusqlite::params![TASK_ID],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read the already-scoped task");
    assert_eq!(
        untouched,
        (WORKSPACE.to_string(), "wf-round-trip".to_string()),
        "the backfill overwrote a task that already had a scope"
    );

    // Idempotent: bootstrap runs this every start, and the second run must claim
    // nothing.
    let again = db::backfill_blank_default_scope(&db_path, &values).expect("backfill again");
    assert_eq!(
        again,
        db::ScopeBackfillCounts::default(),
        "a second backfill claimed rows that were already filled"
    );
}

/// FR-130 B7: the revoked-key reference probe moved out of
/// `core::service::bootstrap`. The predicate has two halves — the kind filter and
/// the substring — and dropping either one still answers "yes" for the key that
/// really is referenced. So both are checked in the direction that must say no.
#[test]
fn the_secret_store_probe_matches_the_key_and_only_inside_a_secret_store() {
    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("secret-refs.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    let conn = open_conn(&db_path).expect("open database");
    let now = orchestrator_persistence::now_ts();

    conn.execute(
        "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json,
                                created_at, updated_at)
         VALUES ('SecretStore', 'default', 'store-a', 'v1',
                 '{\"key_id\":\"key-live\",\"backend\":\"file\"}', '{}', ?1, ?1)",
        rusqlite::params![now],
    )
    .expect("seed secret store resource");
    // Same needle, different kind. A probe that dropped `kind='SecretStore'`
    // would report this as a live reference and keep a rotation from finishing.
    conn.execute(
        "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json,
                                created_at, updated_at)
         VALUES ('Agent', 'default', 'agent-a', 'v1',
                 '{\"key_id\":\"key-elsewhere\"}', '{}', ?1, ?1)",
        rusqlite::params![now],
    )
    .expect("seed non-secret-store resource");

    assert!(
        db::secret_store_resources_reference_key(&conn, "key-live").expect("probe"),
        "the key a SecretStore names was reported as unreferenced"
    );
    assert!(
        !db::secret_store_resources_reference_key(&conn, "key-elsewhere").expect("probe"),
        "a key named only outside a SecretStore was reported as referenced"
    );
    assert!(
        !db::secret_store_resources_reference_key(&conn, "key-retired").expect("probe"),
        "a key no resource names was reported as referenced"
    );
    // Prefix, not a match: `instr` on the bare id would find this one.
    assert!(
        !db::secret_store_resources_reference_key(&conn, "key-li").expect("probe"),
        "a prefix of a referenced key id was reported as a reference"
    );
}
