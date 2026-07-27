//! An opaque handle to the SecretStore tables, owning its own connection.
//!
//! Until FR-141 this crate's key-lifecycle and audit functions each took a
//! `&rusqlite::Connection`. DD-147 exempts this crate from the persistence
//! chokepoint because it sits *below* core and cannot route through a layer
//! above it — but that exemption covers holding a connection, not requiring one
//! from callers. Eleven public functions demanded one, and the four
//! `db::open_conn` calls in `crates/daemon/src/server/secret.rs` existed for no
//! other reason than to build a connection to hand them. An exempt crate's API
//! shape was making a forbidden crate hold the driver.
//!
//! The handle inverts that. Callers name a database path; the connection is
//! opened here and never leaves. There is deliberately no accessor returning
//! the inner `Connection`: one would restore the exact API this module exists to
//! remove, and `scripts/qa/persistence-api-boundary.rb` would report it.
//!
//! **The session is the transaction scope.** Rotation is three calls —
//! [`SecretStoreSession::begin_rotation`], [`SecretStoreSession::re_encrypt_all_secrets`]
//! and [`SecretStoreSession::complete_rotation`] — and today they are atomic
//! only because the daemon happened to hold one connection across all three.
//! That was an accident of the call site, guaranteed by nothing. Binding them to
//! one handle makes it a property of the type: a caller cannot begin a rotation
//! on one connection and complete it on another without saying so.

use crate::secret_key_audit::{KeyAuditEvent, KeyAuditEventKind};
use crate::secret_key_lifecycle::{KeyRecord, ReEncryptionReport};
use crate::secret_store_crypto::SecretEncryption;
use anyhow::Result;
use std::path::Path;

/// An open session against the SecretStore tables of one database.
///
/// The connection is owned by the session and is closed when it is dropped.
pub struct SecretStoreSession {
    conn: rusqlite::Connection,
}

impl SecretStoreSession {
    /// Opens a session against the database at `db_path`.
    ///
    /// The connection carries the same busy timeout and foreign-key pragma
    /// every other connection to this database carries.
    pub fn open(db_path: &Path) -> Result<Self> {
        Ok(Self {
            conn: crate::open_conn(db_path)?,
        })
    }

    // ─── Key lifecycle ───────────────────────────────────────────

    /// Returns every key record, newest first.
    pub fn query_all_key_records(&self) -> Result<Vec<KeyRecord>> {
        crate::secret_key_lifecycle::query_all_key_records(&self.conn)
    }

    /// Creates the incoming key and moves the outgoing one to `decrypt_only`.
    ///
    /// Returns `(new, old)`. The rotation is not finished until
    /// [`Self::complete_rotation`] is called on this same session.
    pub fn begin_rotation(&self, data_dir: &Path) -> Result<(KeyRecord, KeyRecord)> {
        crate::secret_key_lifecycle::begin_rotation(&self.conn, data_dir)
    }

    /// Re-encrypts every stored secret from `old` to `new`.
    pub fn re_encrypt_all_secrets(
        &self,
        old: &SecretEncryption,
        new: &SecretEncryption,
    ) -> Result<ReEncryptionReport> {
        crate::secret_key_lifecycle::re_encrypt_all_secrets(&self.conn, old, new)
    }

    /// Retires the outgoing key, ending the rotation this session began.
    pub fn complete_rotation(&self, old_key_id: &str) -> Result<()> {
        crate::secret_key_lifecycle::complete_rotation(&self.conn, old_key_id)
    }

    /// Finishes a rotation that was interrupted after `begin_rotation`.
    pub fn resume_rotation(&self, data_dir: &Path) -> Result<ReEncryptionReport> {
        crate::secret_key_lifecycle::resume_rotation(&self.conn, data_dir)
    }

    /// Creates the first active key when none exists.
    pub fn bootstrap_key(&self, data_dir: &Path) -> Result<KeyRecord> {
        crate::secret_key_lifecycle::bootstrap_key(&self.conn, data_dir)
    }

    /// Revokes a key, refusing while it is still referenced unless forced.
    pub fn revoke_key(&self, key_id: &str, force: bool) -> Result<()> {
        crate::secret_key_lifecycle::revoke_key(&self.conn, key_id, force)
    }

    /// Adopts a pre-lifecycle key file as a key record, if one is present.
    pub fn import_legacy_key_record(&self, data_dir: &Path) -> Result<Option<KeyRecord>> {
        crate::secret_key_lifecycle::import_legacy_key_record(&self.conn, data_dir)
    }

    // ─── Audit ───────────────────────────────────────────────────

    /// Records one key-lifecycle audit event.
    pub fn insert_key_audit_event(&self, event: &KeyAuditEvent) -> Result<()> {
        crate::secret_key_audit::insert_key_audit_event(&self.conn, event)
    }

    /// Records one key-lifecycle audit event, ignoring any failure.
    ///
    /// Four call sites wrote `let _ = insert_key_audit_event(…)` because the
    /// audit table may not exist yet during bootstrap, and losing the row is
    /// preferable to failing the operation being audited. Naming that decision
    /// is better than repeating the discard, which reads like an oversight
    /// wherever it appears.
    pub fn record_audit_event_best_effort(&self, event: &KeyAuditEvent) {
        let _ = self.insert_key_audit_event(event);
    }

    /// Returns the most recent audit events across all keys.
    pub fn query_key_audit_events(&self, limit: usize) -> Result<Vec<KeyAuditEvent>> {
        crate::secret_key_audit::query_key_audit_events(&self.conn, limit)
    }

    /// Returns the most recent audit events for one key.
    pub fn query_key_audit_events_for_key(
        &self,
        key_id: &str,
        limit: usize,
    ) -> Result<Vec<KeyAuditEvent>> {
        crate::secret_key_audit::query_key_audit_events_for_key(&self.conn, key_id, limit)
    }
}

/// Builds a `DecryptFailed` audit event for a resource that could not be read.
///
/// It lives here rather than at the call site because the caller is in `core`,
/// which after FR-141 has no connection to write it with and no business
/// knowing the shape of this crate's audit rows.
pub fn decrypt_failed_event(
    project: &str,
    name: &str,
    actor: &str,
    error: &anyhow::Error,
) -> KeyAuditEvent {
    KeyAuditEvent {
        event_kind: KeyAuditEventKind::DecryptFailed,
        key_id: "unknown".to_string(),
        key_fingerprint: "unknown".to_string(),
        actor: actor.to_string(),
        detail_json: serde_json::json!({
            "project": project,
            "name": name,
            "error": error.to_string(),
        })
        .to_string(),
        created_at: crate::now_ts(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // The property the type exists to carry: a rotation begun on a session is
    // completed on the same connection. Before FR-141 this held only because
    // the one caller happened to keep `conn` in scope across all three calls.
    #[test]
    fn a_rotation_begins_and_completes_on_one_session() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("secrets.db");
        let data_dir = temp.path().to_path_buf();
        crate::init_test_schema(&db_path).expect("schema");

        let session = SecretStoreSession::open(&db_path).expect("open session");
        session.bootstrap_key(&data_dir).expect("bootstrap");

        let (new_rec, old_rec) = session.begin_rotation(&data_dir).expect("begin");
        assert_ne!(new_rec.key_id, old_rec.key_id);

        session
            .complete_rotation(&old_rec.key_id)
            .expect("complete on the same session");

        let records = session.query_all_key_records().expect("records");
        let old = records
            .iter()
            .find(|record| record.key_id == old_rec.key_id)
            .expect("the outgoing key is still recorded");
        assert_eq!(old.state.as_str(), "retired");
    }

    // Rotation is deliberately resumable rather than atomic: `begin_rotation`
    // and `complete_rotation` are separate transactions, and an interruption
    // between them leaves the outgoing key `decrypt_only`, which is a supported
    // state rather than a corrupt one. The invariant that matters, then, is not
    // "no half-finished rotation exists" but "a half-finished rotation is
    // recoverable by a later session" — and that is exactly what binding the
    // three calls to one handle could have broken, by moving state into the
    // handle instead of the database. Nothing asserted this before FR-141.
    #[test]
    fn a_rotation_interrupted_after_begin_is_finished_by_a_later_session() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("secrets.db");
        let data_dir = temp.path().to_path_buf();
        crate::init_test_schema(&db_path).expect("schema");

        let old_key_id = {
            let session = SecretStoreSession::open(&db_path).expect("open session");
            session.bootstrap_key(&data_dir).expect("bootstrap");
            let (_, old_rec) = session.begin_rotation(&data_dir).expect("begin");
            old_rec.key_id
        }; // the session is dropped here: the connection closes mid-rotation

        let interrupted = SecretStoreSession::open(&db_path).expect("reopen");
        let states = interrupted.query_all_key_records().expect("records");
        let old = states
            .iter()
            .find(|record| record.key_id == old_key_id)
            .expect("the outgoing key survived the interruption");
        assert_eq!(
            old.state.as_str(),
            "decrypt_only",
            "an interrupted rotation must leave the outgoing key recoverable, not retired"
        );

        interrupted.resume_rotation(&data_dir).expect("resume");

        let after = interrupted.query_all_key_records().expect("records");
        let old = after
            .iter()
            .find(|record| record.key_id == old_key_id)
            .expect("the outgoing key is still recorded");
        assert_eq!(old.state.as_str(), "retired");
    }

    // The audit half, through the same handle rather than through a connection
    // the caller had to produce.
    #[test]
    fn audit_events_are_written_and_read_through_the_session() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("secrets.db");
        crate::init_test_schema(&db_path).expect("schema");

        let session = SecretStoreSession::open(&db_path).expect("open session");
        let error = anyhow::anyhow!("no decryption key");
        session.record_audit_event_best_effort(&decrypt_failed_event(
            "default",
            "creds",
            "system:load_resources",
            &error,
        ));

        let events = session.query_key_audit_events(10).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_kind.as_str(), "decrypt_failed");
        assert!(events[0].detail_json.contains("no decryption key"));
    }
}
