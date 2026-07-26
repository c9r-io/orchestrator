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
    use crate::persistence::migration::{Migration, registered_migrations, run_pending};
    use crate::persistence::schema::PersistenceBootstrap;
    use crate::persistence::sqlite::open_conn;
    use rusqlite::Connection;
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
