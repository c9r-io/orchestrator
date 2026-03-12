use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ─── Audit Event Types ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Enumerates audit events emitted during SecretStore key lifecycle operations.
pub enum KeyAuditEventKind {
    /// A new key file was generated and registered.
    KeyCreated,
    /// A key became the active encryption key.
    KeyActivated,
    /// Rotation started and the previous active key was demoted.
    RotateStarted,
    /// Rotation completed and the old key was retired.
    RotateCompleted,
    /// A key was revoked from further use.
    KeyRevoked,
    /// Decryption failed because a matching key could not process ciphertext.
    DecryptFailed,
    /// Audit record describing a missing key or similar diagnostic condition.
    MissingKeyDiagnostic,
}

impl KeyAuditEventKind {
    /// Returns the stable storage label used in persistence rows and JSON payloads.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::KeyCreated => "key_created",
            Self::KeyActivated => "key_activated",
            Self::RotateStarted => "rotate_started",
            Self::RotateCompleted => "rotate_completed",
            Self::KeyRevoked => "key_revoked",
            Self::DecryptFailed => "decrypt_failed",
            Self::MissingKeyDiagnostic => "missing_key_diagnostic",
        }
    }

    /// Parses a persisted audit-event label.
    pub fn from_str_value(s: &str) -> Result<Self> {
        match s {
            "key_created" => Ok(Self::KeyCreated),
            "key_activated" => Ok(Self::KeyActivated),
            "rotate_started" => Ok(Self::RotateStarted),
            "rotate_completed" => Ok(Self::RotateCompleted),
            "key_revoked" => Ok(Self::KeyRevoked),
            "decrypt_failed" => Ok(Self::DecryptFailed),
            "missing_key_diagnostic" => Ok(Self::MissingKeyDiagnostic),
            other => anyhow::bail!("unknown key audit event kind: {other}"),
        }
    }
}

impl std::fmt::Display for KeyAuditEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── Audit Event ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents one persisted audit event for SecretStore key lifecycle activity.
pub struct KeyAuditEvent {
    /// Kind of lifecycle event that occurred.
    pub event_kind: KeyAuditEventKind,
    /// Identifier of the key affected by the event.
    pub key_id: String,
    /// Fingerprint of the affected key at the time of the event.
    pub key_fingerprint: String,
    /// Actor label that initiated or reported the event.
    pub actor: String,
    /// JSON payload with event-specific details.
    pub detail_json: String,
    /// Timestamp when the event was recorded.
    pub created_at: String,
}

// ─── DB Operations ───────────────────────────────────────────────

/// Inserts a SecretStore key audit event into the database.
pub fn insert_key_audit_event(conn: &Connection, event: &KeyAuditEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO secret_key_audit (event_kind, key_id, key_fingerprint, actor, detail_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            event.event_kind.as_str(),
            event.key_id,
            event.key_fingerprint,
            event.actor,
            event.detail_json,
            event.created_at,
        ],
    )
    .context("failed to insert key audit event")?;
    Ok(())
}

/// Returns the most recent SecretStore key audit events across all keys.
pub fn query_key_audit_events(conn: &Connection, limit: usize) -> Result<Vec<KeyAuditEvent>> {
    let mut stmt = conn.prepare(
        "SELECT event_kind, key_id, key_fingerprint, actor, detail_json, created_at
         FROM secret_key_audit ORDER BY created_at DESC, id DESC LIMIT ?1",
    )?;
    collect_audit_rows(&mut stmt, params![limit])
}

/// Returns the most recent SecretStore key audit events for a single key.
pub fn query_key_audit_events_for_key(
    conn: &Connection,
    key_id: &str,
    limit: usize,
) -> Result<Vec<KeyAuditEvent>> {
    let mut stmt = conn.prepare(
        "SELECT event_kind, key_id, key_fingerprint, actor, detail_json, created_at
         FROM secret_key_audit WHERE key_id = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2",
    )?;
    collect_audit_rows(&mut stmt, params![key_id, limit])
}

fn collect_audit_rows(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
) -> Result<Vec<KeyAuditEvent>> {
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (kind_str, key_id, key_fingerprint, actor, detail_json, created_at) = row?;
        events.push(KeyAuditEvent {
            event_kind: KeyAuditEventKind::from_str_value(&kind_str)?,
            key_id,
            key_fingerprint,
            actor,
            detail_json,
            created_at,
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(
            "CREATE TABLE secret_key_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_kind TEXT NOT NULL,
                key_id TEXT NOT NULL,
                key_fingerprint TEXT NOT NULL,
                actor TEXT NOT NULL,
                detail_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );",
        )
        .expect("create table");
        conn
    }

    #[test]
    fn insert_and_query_audit_events() {
        let conn = setup_db();
        let event = KeyAuditEvent {
            event_kind: KeyAuditEventKind::KeyCreated,
            key_id: "primary".to_string(),
            key_fingerprint: "abc123".to_string(),
            actor: "system".to_string(),
            detail_json: "{}".to_string(),
            created_at: "2026-03-12T00:00:00Z".to_string(),
        };
        insert_key_audit_event(&conn, &event).expect("insert");

        let events = query_key_audit_events(&conn, 10).expect("query");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_kind, KeyAuditEventKind::KeyCreated);
        assert_eq!(events[0].key_id, "primary");
    }

    #[test]
    fn query_events_for_specific_key() {
        let conn = setup_db();
        for (key_id, kind) in [
            ("key-a", KeyAuditEventKind::KeyCreated),
            ("key-b", KeyAuditEventKind::KeyCreated),
            ("key-a", KeyAuditEventKind::KeyActivated),
        ] {
            insert_key_audit_event(
                &conn,
                &KeyAuditEvent {
                    event_kind: kind,
                    key_id: key_id.to_string(),
                    key_fingerprint: "fp".to_string(),
                    actor: "test".to_string(),
                    detail_json: "{}".to_string(),
                    created_at: "2026-03-12T00:00:00Z".to_string(),
                },
            )
            .expect("insert");
        }

        let key_a = query_key_audit_events_for_key(&conn, "key-a", 10).expect("query");
        assert_eq!(key_a.len(), 2);

        let key_b = query_key_audit_events_for_key(&conn, "key-b", 10).expect("query");
        assert_eq!(key_b.len(), 1);
    }

    #[test]
    fn event_kind_round_trip() {
        for kind in [
            KeyAuditEventKind::KeyCreated,
            KeyAuditEventKind::KeyActivated,
            KeyAuditEventKind::RotateStarted,
            KeyAuditEventKind::RotateCompleted,
            KeyAuditEventKind::KeyRevoked,
            KeyAuditEventKind::DecryptFailed,
            KeyAuditEventKind::MissingKeyDiagnostic,
        ] {
            assert_eq!(
                KeyAuditEventKind::from_str_value(kind.as_str()).unwrap(),
                kind
            );
        }
    }
}
