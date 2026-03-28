//! SecretStore encryption, key lifecycle, audit, and secure file helpers.
//!
//! This crate provides the security primitives used by the agent orchestrator
//! for encrypting/decrypting SecretStore values, managing key rotation, emitting
//! audit events, and creating files/directories with safe permissions.

#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]

/// SecretStore key audit event types and database helpers.
pub mod secret_key_audit;
/// SecretStore key lifecycle state machine and rotation logic.
pub mod secret_key_lifecycle;
/// SecretStore encryption/decryption helpers (AES-256-GCM-SIV envelope scheme).
pub mod secret_store_crypto;
/// Secure file and directory creation helpers.
pub mod secure_files;

/// Initializes the minimal database schema required by security tests.
///
/// Creates the `secret_keys`, `secret_key_audit`, and `resources` tables
/// needed by lifecycle and crypto integration tests without pulling in
/// the full persistence bootstrap.
#[cfg(test)]
pub(crate) fn init_test_schema(db_path: &std::path::Path) -> anyhow::Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );
        INSERT INTO schema_version (version) VALUES (99);

        CREATE TABLE IF NOT EXISTS secret_keys (
            key_id TEXT PRIMARY KEY,
            state TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            file_path TEXT NOT NULL,
            created_at TEXT NOT NULL,
            activated_at TEXT,
            rotated_out_at TEXT,
            retired_at TEXT,
            revoked_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_secret_keys_state ON secret_keys(state);

        CREATE TABLE IF NOT EXISTS secret_key_audit (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_kind TEXT NOT NULL,
            key_id TEXT NOT NULL,
            key_fingerprint TEXT NOT NULL,
            actor TEXT NOT NULL,
            detail_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_secret_key_audit_created ON secret_key_audit(created_at);
        CREATE INDEX IF NOT EXISTS idx_secret_key_audit_key_id ON secret_key_audit(key_id, created_at);

        CREATE TABLE IF NOT EXISTS resources (
            kind TEXT NOT NULL,
            project TEXT NOT NULL,
            name TEXT NOT NULL,
            api_version TEXT NOT NULL,
            spec_json TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            generation INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (kind, project, name)
        );

        CREATE TABLE IF NOT EXISTS resource_versions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            project TEXT NOT NULL,
            name TEXT NOT NULL,
            spec_json TEXT NOT NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            version INTEGER NOT NULL,
            author TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_resource_versions_lookup
            ON resource_versions(kind, project, name, version DESC);
        "#,
    )?;
    Ok(())
}

/// Returns the current UTC timestamp as an RFC 3339 string.
pub(crate) fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Opens a SQLite connection and applies standard busy-timeout and pragma settings.
pub(crate) fn open_conn(db_path: &std::path::Path) -> anyhow::Result<rusqlite::Connection> {
    use anyhow::Context;
    let conn = rusqlite::Connection::open(db_path).context("failed to open sqlite db")?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))
        .context("failed to set sqlite busy timeout")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .context("failed to configure sqlite pragmas")?;
    Ok(conn)
}
