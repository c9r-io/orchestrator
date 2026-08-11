//! Reviewed schema baseline for the registered migration chain.
//!
//! FR-130 proposes extracting `orchestrator-persistence` out of `core`. The
//! extraction's acceptance criterion is that the migration chain produces the
//! same schema before and after — but that comparison has no subject unless the
//! "before" side is recorded first, and recording it after the extraction proves
//! nothing. `config/governance/schema-snapshot.sql` is that recording, and the
//! tests below are what keep it honest.
//!
//! It is useful on its own terms too: adding a migration today changes the
//! schema of 49 tables with no reviewable artifact. With the snapshot committed,
//! every migration arrives in a diff that shows exactly what it did to the
//! schema, in the same commit as the migration.
//!
//! Regenerating, after a deliberate schema change:
//!
//! ```text
//! UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p agent-orchestrator schema_snapshot
//! ```
//!
//! then read the diff and commit it together with the migration that caused it.

#[cfg(test)]
mod tests {
    use crate::persistence::migration::{Migration, registered_migrations};
    use crate::persistence::schema::PersistenceBootstrap;
    use orchestrator_persistence::test_support::open_conn;
    use orchestrator_persistence::test_support::run_pending;
    use rusqlite::Connection;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    /// Overriding the path is what makes the negative fixture in
    /// `scripts/qa/test-core-boundary.sh` cheap: the gate points this at a
    /// doctored copy under a temporary directory and asserts the comparison
    /// fails, with no rebuild and no write to the working tree.
    fn snapshot_path() -> PathBuf {
        match std::env::var_os("SCHEMA_SNAPSHOT_PATH") {
            Some(value) => PathBuf::from(value),
            None => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../config/governance/schema-snapshot.sql"),
        }
    }

    /// Renders every schema object SQLite reports, normalised.
    ///
    /// Runs of whitespace are collapsed so that reindenting a migration's DDL
    /// does not show up as a schema change, while any change to a column, type,
    /// constraint, or index still does. `sqlite_%` objects are excluded: SQLite
    /// creates and names autoindexes itself, so they are an artifact of the
    /// engine rather than a decision anyone reviewed.
    fn render_schema(conn: &Connection) -> String {
        let mut statement = conn
            .prepare(
                "SELECT sql FROM sqlite_master \
                 WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' \
                 ORDER BY type, name",
            )
            .expect("prepare sqlite_master query");
        let mut rendered: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query sqlite_master")
            .map(|sql| {
                let sql = sql.expect("read schema sql");
                format!("{};", sql.split_whitespace().collect::<Vec<_>>().join(" "))
            })
            .collect();
        rendered.sort();
        rendered.join("\n") + "\n"
    }

    fn bootstrapped_schema(dir: &Path) -> String {
        let db_path = dir.join("snapshot.db");
        PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");
        let conn = open_conn(&db_path).expect("open bootstrapped db");
        render_schema(&conn)
    }

    fn compare_or_update(actual: &str, label: &str) {
        let path = snapshot_path();
        if std::env::var_os("UPDATE_SCHEMA_SNAPSHOT").is_some() {
            std::fs::write(&path, actual).expect("write schema snapshot");
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "cannot read the reviewed schema snapshot at {}: {error}. \
                 Generate it with UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p agent-orchestrator schema_snapshot",
                path.display()
            )
        });
        if expected == actual {
            return;
        }

        let expected_lines: Vec<&str> = expected.lines().collect();
        let actual_lines: Vec<&str> = actual.lines().collect();
        let mut detail = Vec::new();
        for line in &actual_lines {
            if !expected_lines.contains(line) {
                detail.push(format!("  + {line}"));
            }
        }
        for line in &expected_lines {
            if !actual_lines.contains(line) {
                detail.push(format!("  - {line}"));
            }
        }
        panic!(
            "{label} does not match the reviewed schema snapshot at {}:\n{}\n\
             Regenerate with UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p agent-orchestrator schema_snapshot, \
             review the diff, and commit it with the migration that caused it",
            path.display(),
            detail.join("\n")
        );
    }

    #[test]
    fn full_chain_reproduces_the_reviewed_snapshot() {
        let temp = tempfile::tempdir().expect("temp dir");
        let actual = bootstrapped_schema(temp.path());
        compare_or_update(&actual, "the schema produced by the full migration chain");
    }

    #[test]
    fn registered_versions_are_unique_and_ascending() {
        let migrations = registered_migrations();
        assert!(
            !migrations.is_empty(),
            "the registered migration chain is empty"
        );
        let mut previous = 0u32;
        for migration in &migrations {
            assert!(
                migration.version > previous,
                "migration {} has version {}, which does not follow {}",
                migration.name,
                migration.version,
                previous
            );
            previous = migration.version;
        }
    }

    /// A second bootstrap must apply nothing and change nothing. Asserting only
    /// "applies zero" would pass for a chain that re-ran its DDL idempotently
    /// while silently altering the schema, so the snapshot is compared too.
    #[test]
    fn a_second_bootstrap_applies_nothing_and_changes_nothing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("snapshot.db");

        PersistenceBootstrap::ensure_current(&db_path).expect("first bootstrap");
        let conn = open_conn(&db_path).expect("open db");
        let after_first = render_schema(&conn);

        let migrations = registered_migrations();
        let applied = run_pending(&conn, &migrations).expect("second run");
        assert!(
            applied.is_empty(),
            "a second run applied {} migration(s); the chain is not idempotent",
            applied.count()
        );
        assert_eq!(
            after_first,
            render_schema(&conn),
            "a second run left the schema different"
        );
    }

    /// Where the previous release's frozen schema lives. Overridable for the
    /// same reason [`snapshot_path`] is: the negative fixtures point it at a
    /// doctored copy rather than writing to the working tree.
    fn previous_release_snapshot_path() -> PathBuf {
        match std::env::var_os("PREVIOUS_RELEASE_SCHEMA_SNAPSHOT_PATH") {
            Some(value) => PathBuf::from(value),
            None => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../config/governance/schema-snapshot-previous-release.sql"),
        }
    }

    /// Executes a rendered snapshot into a fresh in-memory database.
    ///
    /// The snapshot is sorted by object type and then name, so every
    /// `CREATE INDEX` precedes every `CREATE TABLE` and executing the file in
    /// order fails on the first index. Rather than partition the statements by
    /// their text — which would be a lexical guess about SQL — this applies what
    /// it can and retries the rest until a pass makes no progress. Nothing here
    /// depends on recognising a statement: a dependency order this cannot
    /// resolve ends as a panic naming the statements left over, so a wrong guess
    /// about SQLite's ordering cannot pass quietly.
    fn execute_snapshot(sql: &str, label: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        let mut pending: Vec<&str> = sql
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("--"))
            .collect();

        while !pending.is_empty() {
            let mut deferred = Vec::new();
            let mut errors = Vec::new();
            for statement in &pending {
                match conn.execute_batch(statement) {
                    Ok(()) => {}
                    Err(error) => {
                        deferred.push(*statement);
                        errors.push(format!("  {statement}\n    {error}"));
                    }
                }
            }
            assert!(
                deferred.len() < pending.len(),
                "{label} has {} statement(s) that cannot be applied in any order:\n{}",
                deferred.len(),
                errors.join("\n")
            );
            pending = deferred;
        }
        conn
    }

    /// Every table, and every column of every table, that a database has.
    ///
    /// Read back through `sqlite_master` and `PRAGMA table_info` rather than
    /// parsed out of the snapshot text. A per-line regex over `CREATE TABLE x (
    /// ... )` is the cheap version and it is a §4.4 shape 3 proxy — counting or
    /// matching standing in for parsing. The first draft of this comparison did
    /// exactly that and silently found zero tables, because it required no space
    /// before the opening parenthesis. SQLite is already a dependency here; let
    /// it do the parsing.
    fn tables_and_columns(conn: &Connection) -> BTreeMap<String, BTreeSet<String>> {
        let names: Vec<String> = {
            let mut statement = conn
                .prepare(
                    "SELECT name FROM sqlite_master \
                     WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
                )
                .expect("prepare table query");
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("query tables")
                .map(|name| name.expect("read table name"))
                .collect()
        };

        names
            .into_iter()
            .map(|table| {
                let mut statement = conn
                    .prepare(&format!("PRAGMA table_info(\"{table}\")"))
                    .expect("prepare table_info");
                let columns = statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .expect("query table_info")
                    .map(|column| column.expect("read column name"))
                    .collect::<BTreeSet<String>>();
                (table, columns)
            })
            .collect()
    }

    /// Every index a database has, by name.
    fn index_names(conn: &Connection) -> BTreeSet<String> {
        let mut statement = conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'index' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .expect("prepare index query");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query indexes")
            .map(|name| name.expect("read index name"))
            .collect()
    }

    /// Clause 2 of the forward-only rollback contract, mechanically.
    ///
    /// The contract is stated in `crates/orchestrator-persistence/src/migration.rs`:
    /// the previous release binary must be able to serve the current schema.
    /// Mechanically that is a superset property — every table, column and index
    /// the previous release knew about must still be there. Adding is always
    /// allowed; this test says nothing about additions and must not, or it would
    /// block the forward motion the contract exists to permit.
    ///
    /// Before FR-165 nothing asserted this. A migration that dropped a column
    /// would regenerate `schema-snapshot.sql`, arrive in a reviewable diff, and
    /// pass every test in this file — the diff was the only guard, and a diff is
    /// only a guard if someone reads it knowing what to look for.
    ///
    /// What this cannot see: a column that is kept but changes type or loses a
    /// constraint, and a data remap like migration 29's `exited` -> `closed`.
    /// The first is covered by `full_chain_reproduces_the_reviewed_snapshot`,
    /// which compares whole normalised statements. The second is deliberately
    /// outside the contract — see clause 2's note in `migration.rs`.
    #[test]
    fn previous_release_schema_is_a_subset_of_current() {
        let previous_path = previous_release_snapshot_path();
        let current_path = snapshot_path();
        let previous_sql = std::fs::read_to_string(&previous_path).unwrap_or_else(|error| {
            panic!(
                "cannot read the previous release schema at {}: {error}",
                previous_path.display()
            )
        });
        let current_sql = std::fs::read_to_string(&current_path).unwrap_or_else(|error| {
            panic!(
                "cannot read the reviewed schema snapshot at {}: {error}",
                current_path.display()
            )
        });

        let previous = execute_snapshot(&previous_sql, "the previous release schema");
        let current = execute_snapshot(&current_sql, "the reviewed schema snapshot");

        let previous_tables = tables_and_columns(&previous);
        let current_tables = tables_and_columns(&current);
        let previous_indexes = index_names(&previous);
        let current_indexes = index_names(&current);

        // Either side reading empty is a broken read, not a clean comparison,
        // and only one of those is evidence (§4.4 shape 5). Without this the
        // subset assertion below is vacuously true against an empty previous
        // side — which is exactly the state a truncated or unparseable artifact
        // produces.
        assert!(
            !previous_tables.is_empty(),
            "the previous release schema at {} yielded no tables; \
             the comparison read nothing and every check below would pass vacuously",
            previous_path.display()
        );
        assert!(
            !current_tables.is_empty(),
            "the reviewed schema snapshot at {} yielded no tables; \
             the comparison read nothing",
            current_path.display()
        );

        let mut removals = Vec::new();
        for (table, columns) in &previous_tables {
            match current_tables.get(table) {
                None => removals.push(format!(
                    "  table {table} existed in the previous release and is gone"
                )),
                Some(current_columns) => {
                    for column in columns.difference(current_columns) {
                        removals.push(format!(
                            "  column {table}.{column} existed in the previous release and is gone"
                        ));
                    }
                }
            }
        }
        for index in previous_indexes.difference(&current_indexes) {
            removals.push(format!(
                "  index {index} existed in the previous release and is gone"
            ));
        }

        assert!(
            removals.is_empty(),
            "the previous release binary can no longer serve this schema, which breaks clause 2 \
             of the forward-only rollback contract in \
             crates/orchestrator-persistence/src/migration.rs:\n{}\n\
             {} table(s) and {} index(es) in {}, {} and {} now.\n\
             A normal binary rollback keeps the upgraded database, so anything the previous \
             release reads must still exist. If the removal is intended, it is a breaking \
             change: it needs a release boundary, not a snapshot refresh.",
            removals.join("\n"),
            previous_tables.len(),
            previous_indexes.len(),
            previous_path.display(),
            current_tables.len(),
            current_indexes.len(),
        );
    }

    /// A chain interrupted after any step must reach the same schema when it is
    /// resumed. This is the behaviour a crashed or killed daemon depends on, and
    /// it is checked at every step rather than at a sampled few: a resume defect
    /// lives in one specific migration, and sampling is how you miss it.
    #[test]
    fn an_interrupted_chain_resumes_to_the_same_schema() {
        let one_shot_dir = tempfile::tempdir().expect("temp dir");
        let expected = bootstrapped_schema(one_shot_dir.path());

        // What the database itself says ran, rather than what this test intends
        // to sweep. The two are compared after the loop: `1..=total` looks
        // exhaustive by construction, but `.step_by(5)` or a `take(10)` inserted
        // for speed would leave the loop passing while covering a seventh of the
        // chain. Counting the iterations against the applied rows is the
        // difference between a sweep and a claim of one (FR-130 Phase A).
        let applied_rows: usize = {
            let conn = open_conn(&one_shot_dir.path().join("snapshot.db")).expect("open one-shot");
            conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count applied migrations") as usize
        };

        let total = registered_migrations().len();
        let mut exercised = 0usize;
        for stop_after in 1..=total {
            exercised += 1;
            let temp = tempfile::tempdir().expect("temp dir");
            let db_path = temp.path().join("resume.db");
            let conn = open_conn(&db_path).expect("open db");

            let prefix: Vec<Migration> = registered_migrations()
                .into_iter()
                .take(stop_after)
                .collect();
            let first = run_pending(&conn, &prefix).expect("apply prefix");
            assert_eq!(
                first.count() as usize,
                stop_after,
                "applying the first {stop_after} migration(s) applied {} instead",
                first.count()
            );

            let rest = registered_migrations();
            run_pending(&conn, &rest).expect("resume the chain");

            assert_eq!(
                expected,
                render_schema(&conn),
                "resuming after migration {stop_after} of {total} produced a different schema \
                 than running the chain in one pass"
            );
        }

        assert_eq!(
            exercised, applied_rows,
            "the resume sweep exercised {exercised} interrupt point(s) but the chain applied \
             {applied_rows} migration(s); every applied migration must be an interrupt point"
        );
        assert_eq!(
            total, applied_rows,
            "{total} migration(s) are registered but {applied_rows} were applied"
        );
    }
}
