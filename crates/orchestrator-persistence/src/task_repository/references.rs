//! What happens to the rows that reference a task when the task is deleted.
//!
//! Ten tables reference `tasks(id)`. Two declare `ON DELETE CASCADE` and are
//! SQLite's problem. Of the eight that remain the delete routine has always
//! cleared exactly one — `task_items`, with the `command_runs` hanging off it
//! and the foreign-key-less `events` rows — and the other seven refused every
//! delete with a bare `FOREIGN KEY constraint failed` naming nothing.
//!
//! FR-168 rules on those seven. The ruling is recorded as a map from
//! `table.column` to a [`Disposition`], and the important property is which
//! half of that is derived and which is written down:
//!
//! - **The set of references is derived from the schema**, every time, by
//!   [`blocking_references`]. A table added later appears on its own.
//! - **The set of decisions is written down**, because a disposition is a
//!   judgement about what a row means and no query can infer it.
//! - **Anything derived but not decided is [`Disposition::BlockAndReport`]**,
//!   so the combination fails closed. A new table refuses deletes and names
//!   itself until somebody rules on it; it does not get silently destroyed by
//!   a cascade that never heard of it.
//!
//! That asymmetry is the whole design. A hand-written list of the seven would
//! be correct today and silently short by one the next time somebody adds a
//! table — the shape this repository keeps finding — while a purely derived
//! policy would have to guess whether a row is owned by the task or is an
//! independent record of something that happened.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

/// What a task delete does to a row that references the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Delete the row with the task: it is owned by the task and means nothing
    /// without it.
    DeleteWithTask,
    /// Null the foreign-key column and keep the row: it records a fact that
    /// happened independently of the task's continued existence.
    NullTheReference,
    /// Refuse the delete and name this reference. The default for anything not
    /// listed in [`DISPOSITIONS`].
    BlockAndReport,
}

/// The FR-168 rulings, one per blocking reference.
///
/// The nullability of each column is not incidental — it was derived from the
/// schema before these were chosen, and it lines up exactly: every `NOT NULL`
/// reference belongs to a row the task owns, and every nullable one belongs to
/// an inbound audit record or points at a *different* task. Where the two
/// disagree the schema wins, because `null-the-reference` is not available on a
/// `NOT NULL` column at all.
///
/// The reasons live in `docs/design_doc/orchestrator/184-*.md`. They are worth
/// reading before adding a row here: the cost of getting one wrong is either a
/// destroyed audit record or a delete that never works.
const DISPOSITIONS: &[(&str, &str, Disposition)] = &[
    // Owned by the task. All three columns are NOT NULL, so nulling is not an
    // option even if it were desirable.
    ("handoff_snapshots", "task_id", Disposition::DeleteWithTask),
    ("resume_plans", "task_id", Disposition::DeleteWithTask),
    ("source_bindings", "task_id", Disposition::DeleteWithTask),
    // Independent records. All four columns are nullable.
    //
    // `resume_executions.child_task_id` points at the task the resume created,
    // not at the task being deleted; the execution is an audit of an operator
    // action either way.
    (
        "resume_executions",
        "child_task_id",
        Disposition::NullTheReference,
    ),
    // Destroying these because a task was deleted would lose the record that
    // the event ever arrived, which is the one thing inbound ingest exists to
    // remember.
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
    // This row additionally carries a `UNIQUE deterministic_task_id`, which is
    // the idempotency key for the delivery that produced the task. Deleting the
    // row frees that key and a replay of the same delivery would fire again;
    // nulling the task reference keeps the key and the replay stays suppressed.
    (
        "source_automation_routes",
        "task_id",
        Disposition::NullTheReference,
    ),
];

/// The disposition recorded for `table.column`, or [`Disposition::BlockAndReport`].
///
/// The fallthrough is the point rather than an oversight: see the module docs.
pub fn disposition_for(table: &str, column: &str) -> Disposition {
    DISPOSITIONS
        .iter()
        .find(|(t, c, _)| *t == table && *c == column)
        .map(|(_, _, d)| *d)
        .unwrap_or(Disposition::BlockAndReport)
}

/// Every recorded disposition, as `(table, column, disposition)`.
///
/// Exposed so a test can check the map against the live schema. An entry naming
/// a table or column that no longer exists is a stale ruling: it matches
/// nothing, so it changes no behaviour and produces no diagnostic, and the
/// table it was meant to govern has silently reverted to blocking. Nothing in
/// the map's own lookup can notice that, which is why the check is a test over
/// this accessor and not a condition inside [`disposition_for`].
pub fn recorded_dispositions() -> &'static [(&'static str, &'static str, Disposition)] {
    DISPOSITIONS
}

/// The columns that reference `tasks(id)` and would refuse a delete.
///
/// Read from the schema rather than listed here. Ten tables reference
/// `tasks(id)`; `task_graph_runs` and `task_graph_snapshots` declare
/// `ON DELETE CASCADE` and so never refuse anything, and SQLite clears them
/// itself. Of the eight that remain, the task cascade clears exactly one —
/// `task_items`, together with the `command_runs` hanging off it and the
/// `events` rows, which carry no foreign key at all — and that one name is the
/// only literal below.
///
/// A table added later that references `tasks(id)` without a cascade appears
/// in this list on its own. That is the point: a hand-written list of the
/// seven would be correct today and silently short by one the next time
/// somebody adds a table, which is the shape this repository keeps finding.
///
/// Deliberately `pub(crate)`: it takes a `Connection`, and FR-141 governs how
/// many public items of this crate demand a driver type — the reviewed count is
/// zero. The assertions that need the raw derivation are unit tests below
/// rather than integration tests, which would have required widening that
/// boundary to reach them.
pub(crate) fn blocking_references(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        r#"SELECT m.name, f."from"
             FROM sqlite_master m
             JOIN pragma_foreign_key_list(m.name) f
            WHERE m.type = 'table'
              AND f."table" = 'tasks'
              AND UPPER(COALESCE(f.on_delete, '')) <> 'CASCADE'
              AND m.name <> 'task_items'
            ORDER BY m.name, f."from""#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Which of `references` currently hold a row naming `task_id`.
///
/// `pub(crate)` for the same reason as [`blocking_references`].
pub(crate) fn references_holding(
    conn: &Connection,
    references: &[(String, String)],
    task_id: &str,
) -> Result<Vec<String>> {
    let mut holding = Vec::new();
    for (table, column) in references {
        // Both identifiers come from `sqlite_master` in this same database, not
        // from a caller, and neither can be bound as a parameter.
        let found = conn
            .query_row(
                &format!(r#"SELECT 1 FROM "{table}" WHERE "{column}" = ?1 LIMIT 1"#),
                params![task_id],
                |_| Ok(()),
            )
            .optional()?;
        if found.is_some() {
            holding.push(format!("{table}.{column}"));
        }
    }
    Ok(holding)
}

/// A task that was not deleted because a reference with no ruling holds it.
///
/// Carried as a typed error rather than a message so the three delete paths can
/// each render it their own way — a retention sweep records a skip, an operator
/// command prints a diagnostic — without any of them parsing a string or
/// re-deriving the attribution the delete routine already computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDeleteBlocked {
    /// The task that stayed.
    pub task_id: String,
    /// `table.column` for every blocking reference holding a row, schema order.
    pub blocked_by: Vec<String>,
}

impl std::fmt::Display for TaskDeleteBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "task {} is still referenced by {}, and no disposition is recorded for {}; \
             delete refused",
            self.task_id,
            self.blocked_by.join(", "),
            if self.blocked_by.len() == 1 {
                "it"
            } else {
                "them"
            },
        )
    }
}

impl std::error::Error for TaskDeleteBlocked {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::PersistenceBootstrap;
    use crate::test_support::open_conn;

    /// A bootstrapped database, schema identical to production's.
    fn schema_conn(dir: &std::path::Path) -> Connection {
        let db_path = dir.join("references.db");
        PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap the schema");
        open_conn(&db_path).expect("open database")
    }

    /// The blocking set is derived from the schema, so a table added after this
    /// code was written appears in it with nothing edited.
    ///
    /// That derivation is the line between this design and a hand-written list
    /// of the seven, and it cannot be asserted by naming an existing table: the
    /// live ones are all ruled on, and a fixture naming one would break the day
    /// somebody ruled differently. The table is created here instead.
    #[test]
    fn a_table_added_later_appears_in_the_blocking_set() {
        let temp = tempfile::tempdir().expect("temp dir");
        let conn = schema_conn(temp.path());

        let before = blocking_references(&conn).expect("derive before");
        assert!(
            !before.iter().any(|(t, _)| t == "later_addition"),
            "the fixture table already existed, so this proves nothing"
        );

        conn.execute_batch(
            "CREATE TABLE later_addition (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL,
                 FOREIGN KEY(task_id) REFERENCES tasks(id)
             );",
        )
        .expect("add a table");

        let after = blocking_references(&conn).expect("derive after");
        assert!(
            after
                .iter()
                .any(|(t, c)| t == "later_addition" && c == "task_id"),
            "a table added after this code was written did not appear in the derived \
             blocking set: {after:?}"
        );
        // And it is unruled, so it refuses rather than being silently disposed of.
        assert_eq!(
            disposition_for("later_addition", "task_id"),
            Disposition::BlockAndReport
        );
    }

    /// A cascading reference is not a blocker and must not be reported as one.
    ///
    /// The mutation this guards is dropping the `on_delete` filter from the
    /// query, which would name `task_graph_runs` — a table that has never
    /// refused anything — and send the next reader after the wrong thing.
    #[test]
    fn a_cascading_reference_is_not_a_blocker() {
        let temp = tempfile::tempdir().expect("temp dir");
        let conn = schema_conn(temp.path());
        let refs = blocking_references(&conn).expect("derive the blocking set");
        for cascading in ["task_graph_runs", "task_graph_snapshots"] {
            assert!(
                !refs.iter().any(|(t, _)| t == cascading),
                "{cascading} declares ON DELETE CASCADE and never refuses a delete, \
                 but it was reported as a blocker: {refs:?}"
            );
        }
        // `task_items` is cleared by the cascade itself and is likewise not a blocker.
        assert!(!refs.iter().any(|(t, _)| t == "task_items"));
    }

    /// Every ruling names a live reference, and every live reference is ruled on.
    ///
    /// A ruling whose table or column was renamed or dropped matches nothing: it
    /// changes no behaviour, produces no diagnostic and appears in no log, while
    /// the reference it governed silently reverts to refusing every delete.
    /// `disposition_for` cannot notice — it returns the fail-closed default and
    /// cannot tell "nobody ruled on this" from "somebody ruled on a name that no
    /// longer exists". This is the check that can.
    #[test]
    fn the_ruling_and_the_live_schema_agree_in_both_directions() {
        let temp = tempfile::tempdir().expect("temp dir");
        let conn = schema_conn(temp.path());
        let live = blocking_references(&conn).expect("derive the blocking set");

        for (table, column, _) in recorded_dispositions() {
            assert!(
                live.iter().any(|(t, c)| t == table && c == column),
                "a ruling names {table}.{column}, which is not a blocking reference in the \
                 live schema. Either it was renamed or dropped and this entry is dead, or it \
                 gained a cascade and the ruling is now a second opinion. Both are silent."
            );
        }

        // The converse is allowed to fail when somebody adds a table — that is
        // the design — but it should fail here rather than in front of an
        // operator whose delete stopped working.
        for (table, column) in &live {
            assert_ne!(
                disposition_for(table, column),
                Disposition::BlockAndReport,
                "{table}.{column} references tasks(id) with no cascade and nobody has ruled \
                 on it. Deletes of any task it holds now refuse and name it, which is the \
                 intended fail-closed behaviour — but the ruling is what closes it. See \
                 docs/design_doc/orchestrator/184-task-delete-reference-disposition.md."
            );
        }
    }
}
