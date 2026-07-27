//! The configuration and resource tables: `orchestrator_config_versions`,
//! `config_heal_log`, `resources` and `resource_versions`.
//!
//! What lives here is the statements. What deliberately does not is the shape
//! of what they store: a `CustomResource` becomes a row by being serialised and
//! encrypted, and both of those are `core`'s business — `crd` decides what a
//! resource *is*, and `orchestrator-security` decides what a `SecretStore`
//! spec looks like once encrypted. This module takes rows.
//!
//! That split is why FR-130 Phase B could not move
//! `core/src/persistence/repository/config.rs` wholesale: the file reads `crd`
//! and `crd` calls back into `db`, so the *file* cycles. Phase B's closing note
//! recorded the correction — the unlock condition it had written down answered
//! Phase A's question (can the file go down) rather than Phase B's (can the
//! statements go down), and none of these statements needs a `crd` type. This
//! module is that answer made real.
//!
//! [`ConfigStore::write`] hands the caller a [`ConfigTx`] rather than a
//! transaction. The distinction matters for one call in particular:
//! `enforce_deletion_guards_for_removals` must run *inside* the same
//! transaction as the deletes it guards, or a count taken outside could be
//! stale by the time the delete lands. [`ConfigTx::deletion_guards`] gives the
//! caller exactly that and nothing else — it returns the guard port, not the
//! connection the port is implemented for.
//!
//! The statements are the ones that ran in
//! `core/src/persistence/repository/config.rs` before FR-141 B4, transcribed
//! rather than rewritten. Phase B recorded that this file's SQL invariants had
//! never been audited, because the mechanism that audits them is having to read
//! each statement while moving it — which is what happened here. What that
//! reading found is in DD-151.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::path::{Path, PathBuf};

use crate::db::DeletionGuardQueries;
use crate::now_ts;

/// One `resources` row, already serialised and encrypted by the caller.
#[derive(Debug, Clone)]
pub struct ResourceRow {
    /// Resource kind.
    pub kind: String,
    /// Project scope, already defaulted by the caller.
    pub project: String,
    /// Resource name.
    pub name: String,
    /// API version.
    pub api_version: String,
    /// Spec, serialised and encrypted where the kind requires it.
    pub spec_json: String,
    /// Metadata, serialised.
    pub metadata_json: String,
    /// Generation as the caller holds it.
    pub generation: u64,
    /// Creation timestamp as the caller holds it.
    pub created_at: String,
}

/// One `config_heal_log` row.
#[derive(Debug, Clone)]
pub struct HealLogRow {
    /// Workflow the change applied to.
    pub workflow_id: String,
    /// Step the change applied to.
    pub step_id: String,
    /// Rule label.
    pub rule: String,
    /// Human-readable detail.
    pub detail: String,
}

/// One `config_heal_log` entry as read back.
#[derive(Debug, Clone)]
pub struct HealLogEntryRow {
    /// Config version the heal applied to.
    pub version: i64,
    /// The error that triggered the heal.
    pub original_error: String,
    /// Workflow the change applied to.
    pub workflow_id: String,
    /// Step the change applied to.
    pub step_id: String,
    /// Rule label.
    pub rule: String,
    /// Human-readable detail.
    pub detail: String,
    /// When it was recorded.
    pub created_at: String,
}

/// One `resources` row as read back.
#[derive(Debug, Clone)]
pub struct StoredResourceRow {
    /// Resource kind.
    pub kind: String,
    /// Project scope.
    pub project: String,
    /// Resource name.
    pub name: String,
    /// API version.
    pub api_version: String,
    /// Spec, still encrypted where the kind requires it.
    pub spec_json: String,
    /// Metadata, serialised.
    pub metadata_json: String,
    /// Generation.
    pub generation: i64,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// The configuration and resource tables of one database.
pub struct ConfigStore {
    db_path: PathBuf,
}

impl ConfigStore {
    /// Binds a store to a database path.
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn open(&self) -> Result<Connection> {
        crate::sqlite::open_conn(&self.db_path)
    }

    /// Runs `write` inside one transaction, committing when it returns `Ok`.
    ///
    /// The closure receives a [`ConfigTx`], not a `Transaction`: the caller
    /// gets the operations, not the connection.
    pub fn write<T>(&self, write: impl FnOnce(&ConfigTx<'_>) -> Result<T>) -> Result<T> {
        let conn = self.open()?;
        let tx = conn.unchecked_transaction()?;
        let outcome = write(&ConfigTx { tx: &tx })?;
        tx.commit()?;
        Ok(outcome)
    }

    /// The most recent heal-log header, and how many rows share its version.
    ///
    /// Returns `None` when the newest entry belongs to a different config
    /// version than `current_config_version`: a heal recorded against an older
    /// config is not a summary of this one.
    pub fn latest_heal_summary(
        &self,
        current_config_version: i64,
    ) -> Result<Option<(i64, String, usize, String)>> {
        let conn = self.open()?;
        let row: Option<(i64, String, String)> = conn
            .query_row(
                "SELECT version, original_error, created_at FROM config_heal_log ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let Some((version, original_error, created_at)) = row else {
            return Ok(None);
        };
        if version != current_config_version {
            return Ok(None);
        }

        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM config_heal_log WHERE version = ?1",
            params![version],
            |row| row.get(0),
        )?;
        Ok(Some((version, original_error, count, created_at)))
    }

    /// The most recent heal-log entries, newest first.
    pub fn heal_log_entries(&self, limit: usize) -> Result<Vec<HealLogEntryRow>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT version, original_error, workflow_id, step_id, rule, detail, created_at
             FROM config_heal_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(HealLogEntryRow {
                version: row.get(0)?,
                original_error: row.get(1)?,
                workflow_id: row.get(2)?,
                step_id: row.get(3)?,
                rule: row.get(4)?,
                detail: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Every stored resource row, or `None` when the table does not exist yet.
    ///
    /// `None` and "no rows" are different answers: during bootstrap the
    /// migration chain may not have created `resources` yet, and a caller that
    /// read that as an empty configuration would overwrite one it never saw.
    pub fn all_resource_rows(&self) -> Result<Option<Vec<StoredResourceRow>>> {
        let conn = self.open()?;
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='resources'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !table_exists {
            return Ok(None);
        }

        let mut stmt = conn.prepare(
            "SELECT kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at
             FROM resources",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StoredResourceRow {
                kind: row.get(0)?,
                project: row.get(1)?,
                name: row.get(2)?,
                api_version: row.get(3)?,
                spec_json: row.get(4)?,
                metadata_json: row.get(5)?,
                generation: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        let mut collected = Vec::new();
        for row in rows {
            collected.push(row?);
        }
        Ok(Some(collected))
    }

    /// The highest live resource version, ignoring the `-1` deletion markers.
    pub fn max_resource_version(&self) -> Result<i64> {
        let conn = self.open()?;
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='resource_versions'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !table_exists {
            return Ok(0);
        }
        let version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM resource_versions WHERE version > 0",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }
}

/// The operations available inside a [`ConfigStore::write`] transaction.
///
/// There is deliberately no accessor for the transaction or the connection.
pub struct ConfigTx<'a> {
    tx: &'a Transaction<'a>,
}

impl ConfigTx<'_> {
    /// The deletion-guard port, evaluated inside this transaction.
    ///
    /// `Transaction` derefs to `Connection`, which is what implements the port,
    /// so a guard obtained here reads the same uncommitted state the deletes
    /// will land in. A count taken outside the transaction could be stale by
    /// the time the delete runs, which is the whole reason the guard exists.
    pub fn deletion_guards(&self) -> &dyn DeletionGuardQueries {
        &**self.tx
    }

    /// Appends the next config version, returning `(version, created_at)`.
    pub fn insert_config_version(
        &self,
        yaml: &str,
        json_raw: &str,
        author: &str,
    ) -> Result<(i64, String)> {
        let current_version: i64 = self.tx.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM orchestrator_config_versions",
            [],
            |row| row.get(0),
        )?;
        let next_version = current_version + 1;
        let now = now_ts();
        self.tx.execute(
            "INSERT INTO orchestrator_config_versions (version, config_yaml, config_json, created_at, author)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![next_version, yaml, json_raw, now, author],
        )?;
        Ok((next_version, now))
    }

    /// Records one heal-log row per change, all against the same version.
    pub fn insert_heal_log(
        &self,
        version: i64,
        original_error: &str,
        changes: &[HealLogRow],
    ) -> Result<()> {
        let now = now_ts();
        for change in changes {
            self.tx.execute(
                "INSERT INTO config_heal_log (version, original_error, workflow_id, step_id, rule, detail, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    version,
                    original_error,
                    change.workflow_id,
                    change.step_id,
                    change.rule,
                    change.detail,
                    now
                ],
            )?;
        }
        Ok(())
    }

    /// Upserts one resource row and appends its next version.
    ///
    /// The two statements are one operation: a `resources` row whose history
    /// has no matching `resource_versions` entry is a resource with no record
    /// of how it got there.
    pub fn upsert_resource(&self, row: &ResourceRow, author: &str) -> Result<()> {
        let now = now_ts();
        self.tx.execute(
            "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(kind, project, name) DO UPDATE SET
               api_version=excluded.api_version,
               spec_json=excluded.spec_json,
               metadata_json=excluded.metadata_json,
               generation=generation+1,
               updated_at=excluded.updated_at",
            params![
                row.kind,
                row.project,
                row.name,
                row.api_version,
                row.spec_json,
                row.metadata_json,
                row.generation,
                row.created_at,
                now
            ],
        )?;

        let next_version: i64 = self.tx.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM resource_versions WHERE kind=?1 AND project=?2 AND name=?3",
            params![row.kind, row.project, row.name],
            |query_row| query_row.get(0),
        )?;
        self.tx.execute(
            "INSERT INTO resource_versions (kind, project, name, spec_json, metadata_json, version, author, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.kind,
                row.project,
                row.name,
                row.spec_json,
                row.metadata_json,
                next_version,
                author,
                now
            ],
        )?;
        Ok(())
    }

    /// Upserts a `CustomResourceDefinition` row.
    ///
    /// A separate statement from [`Self::upsert_resource`] because it is a
    /// different one: the kind, api version, metadata and generation are
    /// literals, and no `resource_versions` row is appended. Merging the two
    /// would rewrite both.
    pub fn upsert_crd(&self, system_project: &str, kind_name: &str, spec_json: &str) -> Result<()> {
        let now = now_ts();
        self.tx.execute(
            "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at)
             VALUES ('CustomResourceDefinition', ?1, ?2, 'orchestrator.dev/v2', ?3, '{}', 1, ?4, ?5)
             ON CONFLICT(kind, project, name) DO UPDATE SET
               spec_json=excluded.spec_json, generation=generation+1, updated_at=excluded.updated_at",
            params![system_project, kind_name, spec_json, now, now],
        )?;
        Ok(())
    }

    /// Deletes one resource row, recording a `-1` tombstone version if it existed.
    pub fn delete_resource(
        &self,
        kind: &str,
        project: &str,
        name: &str,
        author: &str,
    ) -> Result<bool> {
        let deleted = self.tx.execute(
            "DELETE FROM resources WHERE kind=?1 AND project=?2 AND name=?3",
            params![kind, project, name],
        )? > 0;
        if deleted {
            let now = now_ts();
            self.tx.execute(
                "INSERT INTO resource_versions (kind, project, name, spec_json, metadata_json, version, author, created_at)
                 VALUES (?1, ?2, ?3, '\"deleted\"', '{}', -1, ?4, ?5)",
                params![kind, project, name, author, now],
            )?;
        }
        Ok(deleted)
    }
}
