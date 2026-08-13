//! FR-168: what a task delete does to the rows that reference the task.
//!
//! Seven tables reference `tasks(id)` without a cascade and were not cleared by
//! the delete routine, so every one of them refused every delete with a bare
//! `FOREIGN KEY constraint failed` naming nothing. FR-168 rules on all seven.
//!
//! The assertions here are deliberately *not* "the delete succeeded". A cascade
//! that destroyed all seven tables would satisfy that, and would also destroy
//! the record that an inbound event ever arrived. Each case asserts the
//! disposition that was chosen: owned rows gone, audit rows still present with
//! a null reference. The difference between those two outcomes is the entire
//! content of the ruling, and an assertion that cannot see it is not evidence.

use orchestrator_persistence::async_database::AsyncDatabase;
use orchestrator_persistence::schema::PersistenceBootstrap;
use orchestrator_persistence::task_repository::{
    AsyncSqliteTaskRepository, Disposition, TaskDeleteBlocked, disposition_for,
    recorded_dispositions,
};
use orchestrator_persistence::test_support::open_conn;
use std::sync::Arc;

const TASK_ID: &str = "task-disposition";

/// A database with the full migration chain applied and one task seeded.
async fn fixture(dir: &std::path::Path) -> (rusqlite::Connection, AsyncDatabase) {
    let db_path = dir.join("disposition.db");
    PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap the schema");
    let conn = open_conn(&db_path).expect("open database");
    let db = AsyncDatabase::open(&db_path).await.expect("open async db");
    seed_task(&conn, TASK_ID);
    (conn, db)
}

fn seed_task(conn: &rusqlite::Connection, id: &str) {
    let now = orchestrator_persistence::now_ts();
    conn.execute(
        "INSERT INTO tasks (
            id, name, status, goal, target_files_json, mode, workspace_id, workflow_id,
            project_id, workspace_root, qa_targets_json, ticket_dir, created_at, updated_at
         ) VALUES (?1, 'disposition', 'completed', 'g', '[]', 'qa', 'ws', 'wf', 'default',
                   '/tmp', '[]', '/tmp', ?2, ?2)",
        rusqlite::params![id, now],
    )
    .expect("seed task row");
}

/// Seeds one row in `table` referencing the task through `column`.
///
/// Foreign keys are disabled for the duration of the seed and restored after.
/// The rows these tables normally hang off — source connections, resume plans,
/// events — are not what is under test, and requiring each of them would make
/// the fixture large enough to hide the thing it is asserting. The delete
/// itself runs on a different connection with foreign keys enforced, which is
/// the connection whose behaviour is the subject.
fn seed_reference(conn: &rusqlite::Connection, table: &str, column: &str, task_id: &str) {
    let now = orchestrator_persistence::now_ts();
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("relax foreign keys for seeding");
    let sql = match table {
        "handoff_snapshots" => format!(
            "INSERT INTO handoff_snapshots (id, project_id, {column}, source_event_cursor,
                 projection_version, briefing_json, content_hash, state_version, generated_by,
                 created_at)
             VALUES ('ref-1', 'default', '{task_id}', 0, 0, '{{}}', 'h', 'v1', 'op', '{now}')"
        ),
        "resume_plans" => format!(
            "INSERT INTO resume_plans (id, project_id, {column}, boundary_id, mode,
                 expected_state_version, side_effect_class, replay_safe,
                 elevated_confirmation_required, consequence_json, status, expires_at,
                 created_by, created_at)
             VALUES ('ref-1', 'default', '{task_id}', 'b', 'resume', 'v1', 'none', 1, 0, '{{}}',
                     'pending', '{now}', 'op', '{now}')"
        ),
        "source_bindings" => format!(
            "INSERT INTO source_bindings (id, project_id, {column}, provider, installation_id,
                 correlation_key, binding_type, created_by_event_id, created_at)
             VALUES ('ref-1', 'default', '{task_id}', 'slack', 'inst', 'corr', 'thread', 'evt',
                     '{now}')"
        ),
        "resume_executions" => format!(
            "INSERT INTO resume_executions (id, plan_id, {column}, actor, operator_reason,
                 idempotency_key, request_hash, status, created_at)
             VALUES ('ref-1', 'plan-1', '{task_id}', 'op', 'because', 'idem', 'hash', 'executing',
                     '{now}')"
        ),
        "source_events" => format!(
            "INSERT INTO source_events (id, project_id, {column}, provider, installation_id,
                 external_event_id, event_type, occurred_at, received_at,
                 normalized_payload_json, payload_hash, routing_state)
             VALUES ('ref-1', 'default', '{task_id}', 'slack', 'inst', 'ext', 'message', '{now}',
                     '{now}', '{{}}', 'hash', 'routed')"
        ),
        "source_routing_attempts" => format!(
            "INSERT INTO source_routing_attempts (source_event_id, attempt_no, result, {column},
                 created_at)
             VALUES ('evt-1', 1, 'routed', '{task_id}', '{now}')"
        ),
        "source_automation_routes" => format!(
            "INSERT INTO source_automation_routes (id, project_id, {column}, automation_key,
                 source_event_id, provider, installation_id, message_identity, channel_id,
                 message_ts, reaction, resolved_role, binding_name, binding_revision,
                 template_name, template_hash, binding_snapshot_json, template_snapshot_json,
                 credential_store, credential_key, request_id, deterministic_task_id, status,
                 created_at, updated_at)
             VALUES ('ref-1', 'default', '{task_id}', 'key', 'evt-1', 'slack', 'inst', 'ident',
                     'chan', 'ts', 'tada', 'role', 'bind', 'rev', 'tmpl', 'thash', '{{}}', '{{}}',
                     'store', 'ckey', 'req', 'det-{task_id}', 'matched', '{now}', '{now}')"
        ),
        other => panic!("no seed recipe for {other}"),
    };
    conn.execute_batch(&sql).expect("seed the referencing row");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("restore foreign keys");
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).expect(sql)
}

/// The FR-168 ruling itself, restated independently of the map that implements
/// it.
///
/// Restating a value the code already holds is normally the wrong shape — the
/// rule is to derive the expectation from the ledger, never to echo it. There
/// is no ledger for this one: a disposition is a judgement about what a row
/// means, and nothing in the schema or the tree can be queried for it. The
/// ledger is `docs/design_doc/orchestrator/184-*.md`, and this is its mechanical
/// echo, so that changing a ruling takes an edit in two places and one of them
/// says out loud that a design decision is being changed.
///
/// This exists because the first version of this suite did not have it. That
/// version iterated `recorded_dispositions()` and asserted the behaviour
/// matched the recorded value — which cannot fail, because flipping a ruling
/// flips the expectation with it. Flipping `source_events.routed_task_id` from
/// null-the-reference to delete-with-task, which silently destroys the record
/// that an inbound event ever arrived, passed all five tests.
const RULING: &[(&str, &str, Disposition)] = &[
    ("handoff_snapshots", "task_id", Disposition::DeleteWithTask),
    ("resume_plans", "task_id", Disposition::DeleteWithTask),
    ("source_bindings", "task_id", Disposition::DeleteWithTask),
    (
        "resume_executions",
        "child_task_id",
        Disposition::NullTheReference,
    ),
    (
        "source_events",
        "routed_task_id",
        Disposition::NullTheReference,
    ),
    (
        "source_routing_attempts",
        "task_id",
        Disposition::NullTheReference,
    ),
    (
        "source_automation_routes",
        "task_id",
        Disposition::NullTheReference,
    ),
];

/// The implementation still carries the ruling FR-168 made.
#[test]
fn the_recorded_map_is_the_ruling() {
    for (table, column, expected) in RULING {
        assert_eq!(
            disposition_for(table, column),
            *expected,
            "{table}.{column} no longer carries the disposition FR-168 ruled for it. \
             If this is deliberate, the design record is what has to change first: \
             docs/design_doc/orchestrator/184-task-delete-reference-disposition.md"
        );
    }
    assert_eq!(
        recorded_dispositions().len(),
        RULING.len(),
        "the map and the ruling disagree about how many references have been ruled on"
    );
}

/// Each ruling matches the property that justified it.
///
/// The reasoning behind the seven was that the schema already encodes the
/// answer: a `NOT NULL` reference belongs to a row the task owns, a nullable one
/// to a record that outlives it. That correspondence is derived here rather than
/// restated, so it also catches the case the echo above cannot — a ruling
/// changed in both places by someone who did not notice that
/// `null-the-reference` is not available on a `NOT NULL` column at all, and
/// would fail at runtime on the first task that used it.
#[tokio::test]
async fn each_ruling_matches_the_nullability_that_justified_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (conn, _db) = fixture(temp.path()).await;

    for (table, column, disposition) in recorded_dispositions() {
        let not_null: i64 = conn
            .query_row(
                &format!(
                    "SELECT \"notnull\" FROM pragma_table_info('{table}') WHERE name = '{column}'"
                ),
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|e| panic!("read nullability of {table}.{column}: {e}"));

        match disposition {
            Disposition::DeleteWithTask => assert_eq!(
                not_null, 1,
                "{table}.{column} is nullable but ruled delete-with-task. That may be right, \
                 but it is no longer the ownership argument the ruling was made on, and the \
                 design record has to say why the row is owned rather than independent."
            ),
            Disposition::NullTheReference => assert_eq!(
                not_null, 0,
                "{table}.{column} is NOT NULL but ruled null-the-reference. This cannot work: \
                 the UPDATE will fail on the first task that uses it."
            ),
            Disposition::BlockAndReport => {
                panic!("{table}.{column} is recorded as the default and should not be listed")
            }
        }
    }
}

/// Every reference is disposed of as ruled, and the two dispositions are
/// distinguishable from each other.
#[tokio::test]
async fn each_reference_is_disposed_of_as_ruled() {
    for (table, column, expected) in recorded_dispositions() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (conn, db) = fixture(temp.path()).await;
        seed_reference(&conn, table, column, TASK_ID);

        let repo = AsyncSqliteTaskRepository::new(Arc::new(db.clone()));
        repo.delete_task_and_collect_log_paths(TASK_ID)
            .await
            .unwrap_or_else(|e| panic!("delete a task held only by {table}.{column}: {e}"));

        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM tasks"),
            0,
            "{table}.{column} still refused the delete after being ruled on"
        );

        let rows = count(&conn, &format!("SELECT COUNT(*) FROM \"{table}\""));
        match expected {
            Disposition::DeleteWithTask => assert_eq!(
                rows, 0,
                "{table}.{column} is ruled delete-with-task but the row outlived the task"
            ),
            Disposition::NullTheReference => {
                assert_eq!(
                    rows, 1,
                    "{table}.{column} is ruled null-the-reference but the row was destroyed; \
                     the record that this happened is gone"
                );
                assert_eq!(
                    count(
                        &conn,
                        &format!("SELECT COUNT(*) FROM \"{table}\" WHERE \"{column}\" IS NOT NULL")
                    ),
                    0,
                    "{table}.{column} kept its row but not by nulling the reference"
                );
            }
            Disposition::BlockAndReport => {
                panic!(
                    "{table}.{column} is recorded as block-and-report, which is the default \
                        and should never be written down explicitly"
                )
            }
        }
    }
}

/// A reference nobody has ruled on refuses the delete, names itself, and leaves
/// the task whole.
///
/// The table is created inside the test rather than named from the schema: no
/// table in the tree is currently undisposed, so any fixture naming one would
/// break the moment it was ruled on. Creating one also asserts the stronger
/// property — the blocking set is derived from the schema at runtime, so a
/// table that did not exist when the delete routine was written is picked up
/// with no list edited anywhere. That derivation is the line between this
/// design and a hand-written list of seven.
#[tokio::test]
async fn an_unruled_reference_refuses_the_delete_and_names_itself() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (conn, db) = fixture(temp.path()).await;
    conn.execute_batch(
        "CREATE TABLE later_addition (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL,
             FOREIGN KEY(task_id) REFERENCES tasks(id)
         );
         INSERT INTO later_addition (id, task_id) VALUES ('row-1', 'task-disposition');",
    )
    .expect("add a table nobody has ruled on");

    // That the table lands in the derived blocking set at all is asserted by
    // `references::tests::a_table_added_later_appears_in_the_blocking_set`,
    // inside the crate: the derivation helper takes a `Connection` and stays
    // `pub(crate)` so this crate's public API keeps demanding no driver type
    // (FR-141). What is asserted here is the consequence an operator sees.

    let repo = AsyncSqliteTaskRepository::new(Arc::new(db.clone()));
    let error = repo
        .delete_task_and_collect_log_paths(TASK_ID)
        .await
        .expect_err("an unruled reference must refuse the delete");

    // The diagnostic, not the exit status. A failure that merely *failed*
    // cannot be told apart from a disk error, and the whole point of the
    // requirement is that the operator learns which table is holding the task.
    let blocked = error
        .downcast_ref::<TaskDeleteBlocked>()
        .unwrap_or_else(|| panic!("refusal did not carry the attribution: {error}"));
    assert_eq!(
        blocked.blocked_by,
        vec!["later_addition.task_id".to_string()]
    );
    assert!(
        error.to_string().contains("later_addition.task_id"),
        "the rendered message did not name the table holding the task: {error}"
    );

    // Refused before anything was mutated, not rolled back after.
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM tasks WHERE id = 'task-disposition'"
        ),
        1,
        "a refused delete removed the task anyway"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM later_addition"),
        1,
        "a refused delete disposed of the reference that refused it"
    );
}

/// `--force` is a confirmation gate and does not widen the blast radius.
///
/// There is no force parameter on this layer at all, which is the assertion:
/// the disposition is a property of the reference, and no caller can pass
/// anything that changes it. A future `force` flag threaded down to here would
/// have to change this signature to have any effect, and that is the point at
/// which somebody has to argue for it.
#[tokio::test]
async fn no_caller_can_widen_the_disposition() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (conn, db) = fixture(temp.path()).await;
    conn.execute_batch(
        "CREATE TABLE later_addition (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL,
             FOREIGN KEY(task_id) REFERENCES tasks(id)
         );
         INSERT INTO later_addition (id, task_id) VALUES ('row-1', 'task-disposition');",
    )
    .expect("add a table nobody has ruled on");

    let repo = AsyncSqliteTaskRepository::new(Arc::new(db.clone()));
    // The only delete entry point this layer offers, called the only way it can
    // be called, still refuses.
    assert!(
        repo.delete_task_and_collect_log_paths(TASK_ID)
            .await
            .is_err(),
        "an unruled reference was destroyed by the only available delete path"
    );
}

/// After a delete, no `events` row references a task that no longer exists.
///
/// `events` carries no foreign key, so nothing in the schema would catch an
/// orphan and no constraint failure would ever be raised. The assertion is the
/// closure property over the whole table rather than the spelling of any
/// particular `DELETE`: a routine that dropped the events statement would still
/// contain the word `events` and would still pass a text check.
#[tokio::test]
async fn no_event_outlives_the_task_it_names() {
    let temp = tempfile::tempdir().expect("temp dir");
    let (conn, db) = fixture(temp.path()).await;
    let now = orchestrator_persistence::now_ts();
    seed_task(&conn, "task-survivor");
    for (task, kind) in [
        (TASK_ID, "trigger_fired"),
        (TASK_ID, "task_completed"),
        ("task-survivor", "task_completed"),
    ] {
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, '{}', ?3)",
            rusqlite::params![task, kind, now],
        )
        .expect("seed an event");
    }
    // A reference of each disposition, so the closure is asserted over a delete
    // that both destroyed and nulled things rather than a bare one.
    seed_reference(&conn, "handoff_snapshots", "task_id", TASK_ID);
    seed_reference(&conn, "source_events", "routed_task_id", TASK_ID);

    let repo = AsyncSqliteTaskRepository::new(Arc::new(db.clone()));
    repo.delete_task_and_collect_log_paths(TASK_ID)
        .await
        .expect("delete the task");

    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM events
              WHERE task_id IS NOT NULL
                AND task_id NOT IN (SELECT id FROM tasks)"
        ),
        0,
        "an events row outlived the task it names, and no constraint would ever say so"
    );
    // And the sweep took only what it was given.
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM events WHERE task_id = 'task-survivor'"
        ),
        1,
        "the delete took events belonging to a task it was not asked about"
    );
}
