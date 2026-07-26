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

/// FR-130 B8: the `control_action_audit` statements moved out of
/// `core::action_audit`. `reserve` is `INSERT OR IGNORE` followed by one of two
/// different reads, and which read ran is the fact the caller's conflict rule
/// turns on — so the assertion is on the case, not merely on getting a row back.
#[tokio::test]
async fn a_reservation_reports_which_prior_row_it_found() {
    use orchestrator_persistence::control_action_audit::{self as audit, Reservation};

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("audit.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    let db = AsyncDatabase::open(&db_path).await.expect("open async db");

    let row = |request_id: &str, key: Option<&str>, hash: &str| audit::NewActionAudit {
        request_id: request_id.to_string(),
        schema_version: 1,
        project_id: "default".to_string(),
        actor: Some("uid:501".to_string()),
        resolved_role: Some("operator".to_string()),
        transport: "uds".to_string(),
        target_type: "attention_item".to_string(),
        target_id: "attn-1".to_string(),
        action: "attention.claim".to_string(),
        reason_code: "operator_triage".to_string(),
        operator_reason: None,
        idempotency_key: key.map(str::to_string),
        expected_version: None,
        fencing_token: None,
        request_hash: hash.to_string(),
    };

    let first = audit::reserve(&db, row("req-1", Some("retry-1"), "hash-a"))
        .await
        .expect("first reservation");
    assert!(
        matches!(first, Reservation::Claimed(ref record) if record.request_id == "req-1"),
        "a fresh insert did not claim the row: {first:?}"
    );

    // A different request id under the same retry identity: the retry read is
    // the only one that can find this, and it must name the *original* request.
    let retry = audit::reserve(&db, row("req-2", Some("retry-1"), "hash-a"))
        .await
        .expect("retry reservation");
    match retry {
        Reservation::PriorByRetryIdentity(record) => {
            assert_eq!(record.request_id, "req-1", "the retry found the wrong row");
        }
        other => panic!("a retry under an existing identity reported {other:?}"),
    }

    // No retry identity, request id reused: the other read.
    audit::reserve(&db, row("req-3", None, "hash-b"))
        .await
        .expect("keyless reservation");
    let reused = audit::reserve(&db, row("req-3", None, "hash-b"))
        .await
        .expect("keyless re-reservation");
    assert!(
        matches!(reused, Reservation::PriorByRequestId(_)),
        "a reused request id without a retry identity reported {reused:?}"
    );

    // A denied row must not hold a retry identity hostage. Under a *fresh*
    // request id that is the partial unique index doing the work, since a denied
    // row is outside its predicate.
    audit::insert_terminal(
        &db,
        row("req-denied", Some("retry-denied"), "hash-c"),
        "denied".to_string(),
        "authorization_denied".to_string(),
    )
    .await
    .expect("record a denial");
    let after_denial = audit::reserve(&db, row("req-after", Some("retry-denied"), "hash-c"))
        .await
        .expect("reservation after a denial");
    assert!(
        matches!(after_denial, Reservation::Claimed(_)),
        "a denied row blocked a later authorized attempt: {after_denial:?}"
    );

    // Under the *same* request id it is the `status IN ('reserved','succeeded')`
    // filter on the retry read, and that one is load-bearing: the insert is
    // ignored on the primary key, and without the filter the denied row would
    // come back as a prior reservation whose hash matches — so the caller would
    // be told the action was already handled and would skip it silently.
    audit::insert_terminal(
        &db,
        row("req-both", Some("retry-both"), "hash-d"),
        "denied".to_string(),
        "authorization_denied".to_string(),
    )
    .await
    .expect("record a denial under req-both");
    let error = audit::reserve(&db, row("req-both", Some("retry-both"), "hash-d"))
        .await
        .expect_err("re-reserving a denied request id must not succeed");
    assert!(
        error
            .to_string()
            .contains("reservation conflict without existing row"),
        "a denied row was handed back as a prior reservation: {error}"
    );
}

/// FR-130 B8: `complete`'s `AND status='reserved'` guard, and the project scope
/// on the read and list statements. Both are the halves that say *no*, so both
/// are asserted in that direction.
#[tokio::test]
async fn completing_an_envelope_is_guarded_and_reads_stay_inside_the_project() {
    use orchestrator_persistence::control_action_audit as audit;

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("audit-complete.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    let db = AsyncDatabase::open(&db_path).await.expect("open async db");

    let row = |request_id: &str, project: &str| audit::NewActionAudit {
        request_id: request_id.to_string(),
        schema_version: 1,
        project_id: project.to_string(),
        actor: None,
        resolved_role: None,
        transport: "tcp".to_string(),
        target_type: "resource".to_string(),
        target_id: "res-1".to_string(),
        action: "resource.apply".to_string(),
        reason_code: "operator_apply".to_string(),
        operator_reason: None,
        idempotency_key: None,
        expected_version: None,
        fencing_token: None,
        request_hash: "hash".to_string(),
    };

    audit::reserve(&db, row("req-a", "alpha"))
        .await
        .expect("reserve alpha");
    audit::reserve(&db, row("req-b", "beta"))
        .await
        .expect("reserve beta");

    let completed = audit::complete(
        &db,
        "req-a".to_string(),
        "succeeded".to_string(),
        None,
        Some("resource".to_string()),
        Some("res-1".to_string()),
    )
    .await
    .expect("complete");
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.result_id.as_deref(), Some("res-1"));

    // Completing again must not rewrite a terminal row. Without the
    // `status='reserved'` guard this second call would silently turn a
    // succeeded envelope into a failed one.
    let second = audit::complete(
        &db,
        "req-a".to_string(),
        "failed".to_string(),
        Some("late_failure".to_string()),
        None,
        None,
    )
    .await
    .expect("complete twice");
    assert_eq!(
        second.status, "succeeded",
        "a terminal envelope was rewritten by a second completion"
    );
    assert_eq!(
        second.error_code, None,
        "a terminal envelope picked up a late error code"
    );

    // The project scope is on the statement, not on the caller.
    assert!(
        audit::get(&db, "alpha".to_string(), "req-b".to_string())
            .await
            .expect("cross-project get")
            .is_none(),
        "a read reached a row belonging to another project"
    );
    let listed = audit::list(
        &db,
        audit::ActionAuditFilter {
            project_id: "alpha".to_string(),
            limit: 100,
            ..Default::default()
        },
    )
    .await
    .expect("list alpha");
    assert_eq!(listed.len(), 1, "the list crossed a project boundary");
    assert_eq!(listed[0].request_id, "req-a");
}

/// FR-130 B9: the task-creation transaction moved out of `core::task_ops`. Its
/// contribution is atomicity, so the assertion is on the thing atomicity buys:
/// a failed insert must leave no task, no items and no events behind, not a
/// partial task nobody can explain.
#[test]
fn creating_a_task_writes_rows_items_and_events_or_none_of_them() {
    use orchestrator_persistence::task_repository::{
        NewTaskRow, insert_task_with_items, reset_task_item,
    };

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("creation.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    let now = orchestrator_persistence::now_ts();

    let task = |id: &str| NewTaskRow {
        id: id.to_string(),
        name: "creation round trip".to_string(),
        goal: "prove the commit is one commit".to_string(),
        target_files_json: "[]".to_string(),
        project_id: "default".to_string(),
        workspace_id: WORKSPACE.to_string(),
        workflow_id: "wf-creation".to_string(),
        workspace_root: "/srv/creation".to_string(),
        qa_targets_json: r#"["docs/qa"]"#.to_string(),
        ticket_dir: "docs/ticket".to_string(),
        execution_plan_json: "{}".to_string(),
        loop_mode: "once".to_string(),
        created_at: now.clone(),
        parent_task_id: None,
        spawn_reason: None,
        step_filter_json: String::new(),
        initial_vars_json: String::new(),
        artifacts_dir: "/srv/creation/artifacts".to_string(),
    };
    let events = vec![orchestrator_persistence::task_repository::DbEventRecord {
        task_id: "task-created".to_string(),
        task_item_id: None,
        event_type: "qa_directory_scan_triggered".to_string(),
        payload_json: r#"{"level":"info"}"#.to_string(),
    }];

    let item_ids = insert_task_with_items(
        &db_path,
        &task("task-created"),
        &["docs/qa/a.md".to_string(), "docs/qa/b.md".to_string()],
        &events,
    )
    .expect("create task");
    assert_eq!(item_ids.len(), 2, "one id per path was not returned");

    let conn = open_conn(&db_path).expect("open database");
    let order: Vec<(String, i64)> = conn
        .prepare(
            "SELECT qa_file_path, order_no FROM task_items WHERE task_id = ?1 ORDER BY order_no",
        )
        .expect("prepare")
        .query_map(rusqlite::params!["task-created"], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");
    assert_eq!(
        order,
        vec![
            ("docs/qa/a.md".to_string(), 1),
            ("docs/qa/b.md".to_string(), 2)
        ],
        "task items did not keep the order they were given, one-based"
    );
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE task_id = ?1",
            rusqlite::params!["task-created"],
            |row| row.get(0),
        )
        .expect("count events");
    assert_eq!(event_count, 1, "the creation event was not committed");

    // Now the rollback. Reusing a task id is the failure a crash-and-retry
    // actually produces, and nothing the call would have written may survive it
    // — not the items, and not the observability rows, which is the direction
    // that goes wrong quietly: an event describing a task that does not exist.
    //
    // The limit worth naming, because it was measured rather than assumed:
    // `tasks` is the first statement and the only one a well-formed call can
    // make fail — no later table in this transaction carries a constraint the
    // caller can violate. Two mutations confirm the consequence. Reordering the
    // events ahead of the task insert still passes, and so does deleting the
    // transaction outright: with the only possible failure at statement one,
    // rollback has nothing to undo. So what this pins is the no-op on a
    // duplicate id — the crash-and-retry case — and *not* the transaction. The
    // transaction's own guarantee has no reachable fixture through this API.
    let before_items: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_items", [], |row| row.get(0))
        .expect("count items");
    let failure = insert_task_with_items(
        &db_path,
        &task("task-created"),
        &["docs/qa/c.md".to_string()],
        &events,
    )
    .expect_err("a duplicate task id must not be accepted");
    assert!(
        failure.to_string().contains("UNIQUE constraint failed"),
        "unexpected failure: {failure}"
    );
    let after_items: i64 = conn
        .query_row("SELECT COUNT(*) FROM task_items", [], |row| row.get(0))
        .expect("count items");
    assert_eq!(
        before_items, after_items,
        "a failed creation left task items behind"
    );
    let events_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE task_id = ?1",
            rusqlite::params!["task-created"],
            |row| row.get(0),
        )
        .expect("count events");
    assert_eq!(
        events_after, 1,
        "a failed creation left an event describing a task it never wrote"
    );

    // `reset_task_item` resolves by unique prefix, clears the item's run
    // history, and reports the task it belongs to.
    let item_id = item_ids.first().expect("an item id").clone();
    conn.execute(
        "UPDATE task_items SET status = 'failed', last_error = 'boom' WHERE id = ?1",
        rusqlite::params![item_id],
    )
    .expect("fail the item");
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, stdout_path, stderr_path, started_at)
         VALUES ('run-stale', ?1, 'qa', 'echo stale', '/tmp', '/tmp/o.log', '/tmp/e.log', ?2)",
        rusqlite::params![item_id, now],
    )
    .expect("seed a stale run");

    let owner = reset_task_item(&db_path, &item_id[..8], &now).expect("reset by prefix");
    assert_eq!(owner, "task-created", "the reset named the wrong task");
    let (status, last_error): (String, String) = conn
        .query_row(
            "SELECT status, last_error FROM task_items WHERE id = ?1",
            rusqlite::params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read the reset item");
    assert_eq!(status, "pending");
    assert_eq!(last_error, "", "the previous error survived the reset");
    let stale_runs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM command_runs WHERE task_item_id = ?1",
            rusqlite::params![item_id],
            |row| row.get(0),
        )
        .expect("count runs");
    assert_eq!(
        stale_runs, 0,
        "a stale command run survived the reset and could re-finalize the item"
    );
}

/// FR-130 B11: the source-ingestion statements moved out of `core::source`.
/// Three of their guards decide whether an external event is delivered once,
/// twice, or forever, and none of them was pinned by a test before this one —
/// each was confirmed by mutating it and watching core's 96 `source::` tests
/// stay green.
#[tokio::test]
async fn source_routing_guards_hold_the_line_they_are_there_for() {
    use orchestrator_persistence::source_events as store;

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("source.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    // `source_events.routed_task_id` references `tasks(id)`, so a routing
    // decision needs a real task to point at.
    seed_task(&open_conn(&db_path).expect("open database"));
    let db = AsyncDatabase::open(&db_path).await.expect("open async db");
    let now = orchestrator_persistence::now_ts();

    let event = |id: &str, hash: &str| store::NewSourceEvent {
        id: id.to_string(),
        project_id: "default".to_string(),
        provider: "slack".to_string(),
        installation_id: "T123".to_string(),
        external_event_id: format!("ext-{id}"),
        event_type: "message".to_string(),
        external_actor_id: "U1".to_string(),
        conversation_id: Some("C1".to_string()),
        thread_id: None,
        occurred_at: now.clone(),
        received_at: now.clone(),
        normalized_payload_json: r#"{"kind":"message"}"#.to_string(),
        raw_payload_ref: None,
        payload_hash: hash.to_string(),
    };

    // Ingest is idempotent on the derived identifier, and reports which call
    // did the writing.
    let (row, inserted) = store::ingest_event(&db, event("src-1", "hash-1"))
        .await
        .expect("ingest");
    assert!(inserted, "a fresh event was not reported as inserted");
    assert_eq!(row.routing_state, "received");
    let (again, inserted_again) = store::ingest_event(&db, event("src-1", "hash-2"))
        .await
        .expect("ingest again");
    assert!(!inserted_again, "a repeat ingest claimed to have inserted");
    assert_eq!(
        again.payload_hash, "hash-1",
        "a repeat ingest overwrote the stored payload hash"
    );

    // Guard 1: `complete_routing` only closes an event that is actually being
    // routed. Without it, a late worker could overwrite a routing decision
    // another worker already committed.
    let closed_before_claim = store::complete_routing(
        &db,
        "src-1".to_string(),
        "routed".to_string(),
        Some(TASK_ID.to_string()),
        None,
        None,
        now.clone(),
    )
    .await
    .expect("complete before claim");
    assert!(
        !closed_before_claim,
        "an event was closed out while it was not being routed"
    );

    let claimed = store::claim_pending_events(&db, 10, now.clone(), now.clone())
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1, "the pending event was not claimed");
    assert_eq!(claimed[0].routing_state, "routing");
    assert_eq!(
        claimed[0].routing_attempts, 1,
        "the attempt was not counted"
    );

    assert!(
        store::complete_routing(
            &db,
            "src-1".to_string(),
            "routed".to_string(),
            Some(TASK_ID.to_string()),
            None,
            None,
            now.clone(),
        )
        .await
        .expect("complete after claim"),
        "a claimed event could not be closed out"
    );
    // And not twice.
    assert!(
        !store::complete_routing(
            &db,
            "src-1".to_string(),
            "failed".to_string(),
            None,
            Some("late".to_string()),
            None,
            now.clone(),
        )
        .await
        .expect("complete twice"),
        "a routed event was reopened by a second completion"
    );

    // The same guard on the automation hand-off, which is a separate statement
    // with a separate copy of it: a delivery nobody is routing must not be
    // handed to the automation worker, or two workers own the same delivery.
    assert!(
        !store::defer_to_automation(&db, "src-1".to_string(), "route-1".to_string(), now.clone(),)
            .await
            .expect("defer a routed event"),
        "a routed event was handed to the automation worker"
    );

    // Guard 2: the attempt ceiling. An event that keeps failing must eventually
    // stop being claimed, or a poison message is retried forever.
    store::ingest_event(&db, event("src-2", "hash-2"))
        .await
        .expect("ingest src-2");
    for attempt in 1..=store::MAX_ROUTING_ATTEMPTS {
        let batch = store::claim_pending_events(&db, 10, now.clone(), now.clone())
            .await
            .expect("claim");
        assert_eq!(
            batch.len(),
            1,
            "attempt {attempt} of {} was not claimable",
            store::MAX_ROUTING_ATTEMPTS
        );
        store::complete_routing(
            &db,
            "src-2".to_string(),
            "failed".to_string(),
            None,
            Some("boom".to_string()),
            // No backoff, so the only thing that can stop the next claim is the
            // ceiling.
            None,
            now.clone(),
        )
        .await
        .expect("fail it");
    }
    let exhausted = store::claim_pending_events(&db, 10, now.clone(), now.clone())
        .await
        .expect("claim past the ceiling");
    assert!(
        exhausted.is_empty(),
        "an event was claimed for attempt {} past a ceiling of {}",
        store::MAX_ROUTING_ATTEMPTS + 1,
        store::MAX_ROUTING_ATTEMPTS
    );

    // Guard 3: a retry identity reused under a different request must be
    // refused, not quietly restarted — restarting would run a command the
    // caller never asked for under an approval it did get.
    let action = |key: &str, hash: &str| store::NewCommandAction {
        id: format!("act-{key}-{hash}"),
        source_event_id: "src-1".to_string(),
        actor: "U1".to_string(),
        resolved_role: "operator".to_string(),
        target_type: "attention_item".to_string(),
        target_id: "attn-1".to_string(),
        action: "attention.claim".to_string(),
        idempotency_key: key.to_string(),
        request_hash: hash.to_string(),
        request_id: "req-1".to_string(),
    };
    assert_eq!(
        store::begin_command_action(&db, action("k1", "h1"), now.clone())
            .await
            .expect("begin"),
        store::CommandActionStart::Started
    );
    assert_eq!(
        store::begin_command_action(&db, action("k1", "h2"), now.clone())
            .await
            .expect("begin with a different request"),
        store::CommandActionStart::RequestMismatch,
        "a retry key was accepted for a different request"
    );
    assert_eq!(
        store::begin_command_action(&db, action("k1", "h1"), now.clone())
            .await
            .expect("begin again"),
        store::CommandActionStart::Restarted,
        "an unfinished attempt was not restartable"
    );
    assert!(
        store::complete_command_action(
            &db,
            "src-1".to_string(),
            "k1".to_string(),
            "succeeded".to_string(),
            None,
            None,
            now.clone(),
        )
        .await
        .expect("complete the action")
    );
    assert_eq!(
        store::begin_command_action(&db, action("k1", "h1"), now.clone())
            .await
            .expect("begin after success"),
        store::CommandActionStart::AlreadySucceeded,
        "a succeeded command was offered for a second run"
    );
}

/// FR-130 B13: the SourceConnection statements moved out of
/// `core::source_connection`. Nine of them carry a fence, and three of those
/// fences exist in more than one copy — `version=?3` in three statements,
/// `state='active'` in four, `owner_daemon_id=?3` in two. A fence with two
/// copies needs two mutations; B11 learned that by mutating the wrong copy and
/// watching the assertion pass.
#[tokio::test]
async fn source_connection_fences_are_each_load_bearing() {
    use orchestrator_persistence::source_connections as store;

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("connections.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    let db = AsyncDatabase::open(&db_path).await.expect("open async db");
    let now = orchestrator_persistence::now_ts();

    let activation = |version: i64, generation: i64, owner: &str| store::NewActivation {
        id: "conn-1".to_string(),
        project_id: "alpha".to_string(),
        provider: "slack".to_string(),
        display_label: "Alpha workspace".to_string(),
        provisioning_mode: "managed_shared".to_string(),
        installation_id: "T111".to_string(),
        installation_id_digest: "digest-T111".to_string(),
        enterprise_id_digest: None,
        owner_daemon_id: owner.to_string(),
        generation,
        version,
        capabilities_json: r#"["chat"]"#.to_string(),
        scopes_json: r#"["chat:write"]"#.to_string(),
        trigger_name: None,
        gateway_origin: Some("https://gw.example".to_string()),
        pairing_secret_ciphertext: Some("cipher".to_string()),
        last_acked_cursor: 5,
        app_ownership: "orchestrator".to_string(),
        app_id_digest: None,
        manifest_version: None,
        provision_state: None,
        provision_error_code: None,
        request_id: "req-activate".to_string(),
    };

    let created = store::activate(&db, activation(1, 1, "daemon-a"), now.clone())
        .await
        .expect("activate");
    assert!(
        matches!(created, store::Activation::Created(_)),
        "a first activation was not reported as a creation: {created:?}"
    );

    // The activation fences, one assertion each. All three are refusals, which
    // is the direction that matters: accepting any of them hands an
    // installation to the wrong owner or rolls a credential backwards.
    assert!(
        matches!(
            store::activate(&db, activation(1, 1, "daemon-b"), now.clone())
                .await
                .expect("activate under another owner"),
            store::Activation::OwnerConflict
        ),
        "a live installation was reauthorized by a different daemon"
    );
    assert!(
        matches!(
            store::activate(&db, activation(0, 1, "daemon-a"), now.clone())
                .await
                .expect("activate with a stale version"),
            store::Activation::StaleFence
        ),
        "an activation older than the stored version was accepted"
    );
    assert!(
        matches!(
            store::activate(&db, activation(2, 2, "daemon-a"), now.clone())
                .await
                .expect("reauthorize"),
            store::Activation::Reauthorized(_)
        ),
        "a legitimate reauthorization was refused"
    );

    let row = store::read_connection(&db, "alpha".to_string(), "conn-1".to_string())
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.version, 2);

    // `record_delivery`'s monotonic fence: forward is accepted, backward is not.
    assert!(
        store::record_delivery(
            &db,
            "alpha".to_string(),
            "conn-1".to_string(),
            9,
            1,
            now.clone()
        )
        .await
        .expect("advance the cursor")
    );
    assert!(
        !store::record_delivery(
            &db,
            "alpha".to_string(),
            "conn-1".to_string(),
            4,
            0,
            now.clone()
        )
        .await
        .expect("rewind the cursor"),
        "a delivery acknowledgement moved the cursor backwards"
    );

    // `last_acked_cursor=MAX(last_acked_cursor,?16)` in the reauthorization
    // statement. The cursor is now 9; this reauthorization offers 5, which is
    // what a provider re-handshake carrying a stale checkpoint looks like.
    // Taking it would replay every event between 5 and 9. The assertion has to
    // come *after* the cursor advanced — checking it before, when offered and
    // stored are both 5, passes with `MAX` removed.
    assert!(
        matches!(
            store::activate(&db, activation(3, 3, "daemon-a"), now.clone())
                .await
                .expect("reauthorize with a stale cursor"),
            store::Activation::Reauthorized(_)
        ),
        "a legitimate reauthorization was refused"
    );
    let row = store::read_connection(&db, "alpha".to_string(), "conn-1".to_string())
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        row.last_acked_cursor, 9,
        "a reauthorization rewound the delivery cursor and would replay events"
    );
    assert_eq!(row.version, 3);

    // The project fence on the read, separately from every write fence.
    assert!(
        store::read_connection(&db, "beta".to_string(), "conn-1".to_string())
            .await
            .expect("cross-project read")
            .is_none(),
        "a read reached a connection in another project"
    );

    // The credential fence is three conditions at once. Each is checked in the
    // direction that must say no, because any one of them alone releasing the
    // pairing secret is a credential leak.
    assert!(
        store::read_credential(
            &db,
            "alpha".to_string(),
            "conn-1".to_string(),
            "daemon-a".to_string()
        )
        .await
        .expect("credential")
        .is_some(),
        "the owning daemon could not read its own credential"
    );
    assert!(
        store::read_credential(
            &db,
            "alpha".to_string(),
            "conn-1".to_string(),
            "daemon-b".to_string()
        )
        .await
        .expect("credential under another owner")
        .is_none(),
        "a credential was released to a daemon that does not own the connection"
    );
    assert!(
        store::read_credential(
            &db,
            "beta".to_string(),
            "conn-1".to_string(),
            "daemon-a".to_string()
        )
        .await
        .expect("credential from another project")
        .is_none(),
        "a credential crossed a project boundary"
    );

    // `transition`: the optimistic version fence, copy one of three.
    assert!(
        store::transition(
            &db,
            "alpha".to_string(),
            "conn-1".to_string(),
            99,
            "suspended".to_string(),
            None,
            "req-t".to_string(),
            now.clone(),
        )
        .await
        .expect("transition on a wrong version")
        .is_none(),
        "a transition was applied against a version that does not match"
    );
    let suspended = store::transition(
        &db,
        "alpha".to_string(),
        "conn-1".to_string(),
        3,
        "suspended".to_string(),
        Some("operator".to_string()),
        "req-t".to_string(),
        now.clone(),
    )
    .await
    .expect("transition")
    .expect("row");
    assert_eq!(suspended.state, "suspended");
    assert_eq!(suspended.version, 4, "the version did not advance");

    // `transfer_owner`: version fence (copy two) *and* `state='active'`. The
    // connection is suspended right now, so the active fence is what refuses.
    assert!(
        store::transfer_owner(
            &db,
            store::OwnerTransfer {
                id: "conn-1".to_string(),
                project_id: "alpha".to_string(),
                expected_version: 4,
                target_daemon_id: "daemon-b".to_string(),
                generation: 4,
                request_id: "req-x".to_string(),
            },
            now.clone(),
        )
        .await
        .expect("transfer a suspended connection")
        .is_none(),
        "ownership of a non-active connection was transferred"
    );
    // Back to active, then refuse on the version fence alone.
    store::transition(
        &db,
        "alpha".to_string(),
        "conn-1".to_string(),
        4,
        "active".to_string(),
        None,
        "req-t2".to_string(),
        now.clone(),
    )
    .await
    .expect("reactivate")
    .expect("row");
    assert!(
        store::transfer_owner(
            &db,
            store::OwnerTransfer {
                id: "conn-1".to_string(),
                project_id: "alpha".to_string(),
                expected_version: 99,
                target_daemon_id: "daemon-b".to_string(),
                generation: 3,
                request_id: "req-x".to_string(),
            },
            now.clone(),
        )
        .await
        .expect("transfer on a wrong version")
        .is_none(),
        "ownership was transferred against a version that does not match"
    );
    let transferred = store::transfer_owner(
        &db,
        store::OwnerTransfer {
            id: "conn-1".to_string(),
            project_id: "alpha".to_string(),
            expected_version: 5,
            target_daemon_id: "daemon-b".to_string(),
            generation: 4,
            request_id: "req-x".to_string(),
        },
        now.clone(),
    )
    .await
    .expect("transfer")
    .expect("row");
    assert_eq!(transferred.owner_daemon_id, "daemon-b");
    assert_eq!(
        transferred.state, "suspended",
        "a transfer left the connection deliverable by its old owner"
    );

    // The credential goes in the same statement that moves ownership. A
    // transfer that changed owner and left the secret behind is the state that
    // fence exists to prevent, and it is not observable from `owner_daemon_id`.
    assert!(
        store::read_credential(
            &db,
            "alpha".to_string(),
            "conn-1".to_string(),
            "daemon-b".to_string()
        )
        .await
        .expect("credential after transfer")
        .is_none(),
        "the pairing secret survived an ownership transfer"
    );

    // `record_delivery`'s `state='active'` fence, separately from its monotonic
    // one: the connection is suspended after the transfer, so a *forward*
    // cursor must still be refused. Without this, the fence that stops a
    // fenced-out daemon from acknowledging deliveries is unpinned — the
    // monotonic assertion above passes with it gone.
    assert!(
        !store::record_delivery(
            &db,
            "alpha".to_string(),
            "conn-1".to_string(),
            20,
            0,
            now.clone()
        )
        .await
        .expect("advance the cursor on a suspended connection"),
        "a suspended connection accepted a delivery acknowledgement"
    );

    // Every one of those transitions wrote a change-log row, in the same
    // transaction as the state change.
    let changes = store::read_changes(&db, "alpha".to_string(), 0, 100)
        .await
        .expect("changes");
    let states: Vec<&str> = changes.iter().map(|c| c.state.as_str()).collect();
    assert_eq!(
        states,
        vec![
            "active",
            "active",
            "active",
            "suspended",
            "active",
            "suspended"
        ],
        "the change log did not record every state change in order"
    );
    assert!(
        store::read_changes(&db, "beta".to_string(), 0, 100)
            .await
            .expect("changes in another project")
            .is_empty(),
        "the change log crossed a project boundary"
    );

    // The dedicated-App lifecycle update carries two fences at once — the third
    // copy of `version=?3`, and `provisioning_mode='managed_dedicated'`. It gets
    // its own connection, in its own project, so the change log asserted above
    // stays what it was.
    let dedicated = store::NewActivation {
        id: "conn-ded".to_string(),
        project_id: "gamma".to_string(),
        provisioning_mode: "managed_dedicated".to_string(),
        installation_id: "T222".to_string(),
        installation_id_digest: "digest-T222".to_string(),
        app_ownership: "workspace".to_string(),
        app_id_digest: Some("digest-app".to_string()),
        manifest_version: Some("v1".to_string()),
        request_id: "req-ded".to_string(),
        ..activation(1, 1, "daemon-a")
    };
    store::activate(&db, dedicated, now.clone())
        .await
        .expect("activate the dedicated connection");

    let lifecycle = |id: &str, project: &str, version: i64| store::LifecycleUpdate {
        id: id.to_string(),
        project_id: project.to_string(),
        expected_version: version,
        state: "attention".to_string(),
        manifest_version: "v2".to_string(),
        provision_state: "reauthorization_required".to_string(),
        error_code: Some("manifest_upgraded".to_string()),
        request_id: "req-life".to_string(),
    };
    assert!(
        store::update_dedicated_lifecycle(&db, lifecycle("conn-ded", "gamma", 99), now.clone())
            .await
            .expect("lifecycle update on a wrong version")
            .is_none(),
        "a dedicated lifecycle update was applied against a version that does not match"
    );
    // `conn-1` is `managed_shared` and sits at version 6 after the transfer, so
    // only the mode fence can refuse this one.
    assert!(
        store::update_dedicated_lifecycle(&db, lifecycle("conn-1", "alpha", 6), now.clone())
            .await
            .expect("lifecycle update on a shared connection")
            .is_none(),
        "a dedicated-App lifecycle update was applied to a managed_shared connection"
    );
    let upgraded =
        store::update_dedicated_lifecycle(&db, lifecycle("conn-ded", "gamma", 1), now.clone())
            .await
            .expect("lifecycle update")
            .expect("row");
    assert_eq!(upgraded.state, "attention");
    assert_eq!(upgraded.manifest_version.as_deref(), Some("v2"));
    assert_eq!(upgraded.version, 2, "the version did not advance");
}

/// FR-130 B13: the intent and dedicated-provisioning fences, which are the
/// other half of `source_connection.rs` and guard credential release and
/// exactly-once OAuth completion.
#[tokio::test]
async fn source_connection_intent_and_provisioning_fences_hold() {
    use orchestrator_persistence::source_connections as store;

    let temp = tempfile::tempdir().expect("temp dir");
    let db_path = temp.path().join("intents.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
    let db = AsyncDatabase::open(&db_path).await.expect("open async db");
    let now = orchestrator_persistence::now_ts();

    // The daemon identity is a singleton: a second call must return the first
    // value, not the candidate it was handed.
    let first = store::daemon_id(&db, "daemon-first".to_string(), now.clone())
        .await
        .expect("daemon id");
    let second = store::daemon_id(&db, "daemon-second".to_string(), now.clone())
        .await
        .expect("daemon id again");
    assert_eq!(
        first, "daemon-first",
        "the first caller did not get its own candidate"
    );
    assert_eq!(
        second, first,
        "a second call minted a new daemon identity instead of returning the stored one"
    );
    // What this does *not* cover, measured rather than assumed: the read-back
    // after `INSERT OR IGNORE`. Replacing it with `Ok(candidate)` passes. The
    // ignore only fires when another writer inserted between this call's check
    // and its insert, and a single `AsyncDatabase` serializes its writer, so the
    // race needs two processes on one file. No fixture here reaches it. The
    // read-back stays because two daemons *can* share a database; it is
    // defensive code with no test, and saying so is better than implying the
    // assertion above covers it.

    store::store_intent(
        &db,
        store::NewIntent {
            id: "intent-1".to_string(),
            project_id: "alpha".to_string(),
            provider: "slack".to_string(),
            display_label: "Alpha".to_string(),
            provisioning_mode: "managed_shared".to_string(),
            owner_daemon_id: "daemon-a".to_string(),
            actor_digest: "actor".to_string(),
            gateway_intent_id: "gw-1".to_string(),
            authorize_url_ciphertext: "auth-cipher".to_string(),
            poll_secret_ciphertext: "poll-cipher".to_string(),
            expires_at: now.clone(),
        },
        now.clone(),
    )
    .await
    .expect("store intent");

    // `owner_daemon_id=?3`, copy two of two. The encrypted authorize URL and
    // polling secret are the payload; releasing them to a non-owner is the leak.
    assert!(
        store::read_intent_credential(
            &db,
            "alpha".to_string(),
            "intent-1".to_string(),
            "daemon-a".to_string()
        )
        .await
        .expect("intent credential")
        .is_some(),
        "the owning daemon could not read its own intent credential"
    );
    assert!(
        store::read_intent_credential(
            &db,
            "alpha".to_string(),
            "intent-1".to_string(),
            "daemon-b".to_string()
        )
        .await
        .expect("intent credential under another owner")
        .is_none(),
        "an intent credential was released to a daemon that does not own it"
    );
    assert!(
        store::read_intent_credential(
            &db,
            "beta".to_string(),
            "intent-1".to_string(),
            "daemon-a".to_string()
        )
        .await
        .expect("intent credential from another project")
        .is_none(),
        "an intent credential crossed a project boundary"
    );

    // `status='pending'` makes intent completion exactly-once.
    assert!(
        store::complete_intent(
            &db,
            "alpha".to_string(),
            "intent-1".to_string(),
            "completed".to_string(),
            None,
            None,
            now.clone(),
        )
        .await
        .expect("complete")
        .is_some()
    );
    assert!(
        store::complete_intent(
            &db,
            "alpha".to_string(),
            "intent-1".to_string(),
            "failed".to_string(),
            None,
            Some("late".to_string()),
            now.clone(),
        )
        .await
        .expect("complete twice")
        .is_none(),
        "a terminal OAuth intent was reopened by a second completion"
    );

    // The dedicated-provisioning status fence, plus `COALESCE` on the values a
    // later step must not blank.
    store::store_provisioning(
        &db,
        store::NewProvisioning {
            id: "prov-1".to_string(),
            project_id: "alpha".to_string(),
            display_label: "Alpha".to_string(),
            owner_daemon_id: "daemon-a".to_string(),
            target_connection_id: None,
            manifest_version: "v1".to_string(),
            manifest_digest: "digest-v1".to_string(),
            expires_at: now.clone(),
        },
        now.clone(),
    )
    .await
    .expect("store provisioning");

    let update = |expected: &str, status: &str, app_id: Option<&str>| store::ProvisioningUpdate {
        id: "prov-1".to_string(),
        project_id: "alpha".to_string(),
        expected_status: expected.to_string(),
        status: status.to_string(),
        app_id_ciphertext: app_id.map(str::to_string),
        app_id_digest: app_id.map(|value| format!("digest-{value}")),
        oauth_intent_id: None,
        error_code: None,
    };
    assert!(
        store::update_provisioning(&db, update("completed", "abandoned", None), now.clone())
            .await
            .expect("update from a status it does not hold")
            .is_none(),
        "a provisioning checkpoint advanced from a status it was not in"
    );
    let creating = store::update_provisioning(
        &db,
        update("awaiting_approval", "creating", Some("app-1")),
        now.clone(),
    )
    .await
    .expect("advance")
    .expect("row");
    assert_eq!(creating.app_id_digest.as_deref(), Some("digest-app-1"));

    // COALESCE: a later step that carries no App id must not erase the one the
    // earlier step recorded, or the dedicated App becomes unreachable.
    let handed_off = store::update_provisioning(
        &db,
        update("creating", "handoff_pending", None),
        now.clone(),
    )
    .await
    .expect("advance again")
    .expect("row");
    assert_eq!(
        handed_off.app_id_digest.as_deref(),
        Some("digest-app-1"),
        "a later provisioning step erased the App identity an earlier one recorded"
    );
}
