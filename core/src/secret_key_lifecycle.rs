use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config_load::now_ts;
use crate::secret_store_crypto::{SecretEncryption, SecretKeyHandle};

// ─── Key State Machine ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Active,
    DecryptOnly,
    Revoked,
    Retired,
}

impl KeyState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::DecryptOnly => "decrypt_only",
            Self::Revoked => "revoked",
            Self::Retired => "retired",
        }
    }

    pub fn from_str_value(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(Self::Active),
            "decrypt_only" => Ok(Self::DecryptOnly),
            "revoked" => Ok(Self::Revoked),
            "retired" => Ok(Self::Retired),
            other => bail!("unknown key state: {other}"),
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Revoked | Self::Retired)
    }
}

impl std::fmt::Display for KeyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── Key Record ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecord {
    pub key_id: String,
    pub state: KeyState,
    pub fingerprint: String,
    pub file_path: String,
    pub created_at: String,
    pub activated_at: Option<String>,
    pub rotated_out_at: Option<String>,
    pub retired_at: Option<String>,
    pub revoked_at: Option<String>,
}

// ─── KeyRing ─────────────────────────────────────────────────────

pub struct KeyRing {
    records: Vec<KeyRecord>,
    active_key: Option<SecretKeyHandle>,
    decrypt_keys: HashMap<String, SecretKeyHandle>,
}

impl KeyRing {
    pub fn active_key(&self) -> Result<&SecretKeyHandle> {
        self.active_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "SecretStore write blocked: no active encryption key (all keys revoked or retired)"
            )
        })
    }

    pub fn decrypt_key(&self, key_id: &str) -> Result<&SecretKeyHandle> {
        self.decrypt_keys.get(key_id).ok_or_else(|| {
            anyhow::anyhow!(
                "no decryption key available for key_id '{key_id}' (key may be revoked or missing)"
            )
        })
    }

    pub fn all_records(&self) -> &[KeyRecord] {
        &self.records
    }

    pub fn active_record(&self) -> Option<&KeyRecord> {
        self.records.iter().find(|r| r.state == KeyState::Active)
    }

    pub fn has_active_key(&self) -> bool {
        self.active_key.is_some()
    }

    pub fn decrypt_only_records(&self) -> Vec<&KeyRecord> {
        self.records
            .iter()
            .filter(|r| r.state == KeyState::DecryptOnly)
            .collect()
    }

    pub fn decrypt_keys_iter(&self) -> impl Iterator<Item = (&str, &SecretKeyHandle)> {
        self.decrypt_keys.iter().map(|(k, v)| (k.as_str(), v))
    }
}

fn audit_event_for_record(
    event_kind: crate::secret_key_audit::KeyAuditEventKind,
    record: &KeyRecord,
    actor: &str,
    detail_json: String,
    created_at: &str,
) -> crate::secret_key_audit::KeyAuditEvent {
    crate::secret_key_audit::KeyAuditEvent {
        event_kind,
        key_id: record.key_id.clone(),
        key_fingerprint: record.fingerprint.clone(),
        actor: actor.to_owned(),
        detail_json,
        created_at: created_at.to_owned(),
    }
}

// ─── Load KeyRing ────────────────────────────────────────────────

pub fn load_keyring(app_root: &Path, db_path: &Path) -> Result<KeyRing> {
    let conn = crate::db::open_conn(db_path)?;
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='secret_keys'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !table_exists {
        // Pre-migration: fall back to legacy single-key loading
        return load_keyring_legacy(app_root, db_path);
    }

    let records = query_all_key_records(&conn)?;
    if records.is_empty() {
        // Table exists but empty — fall back to legacy
        return load_keyring_legacy(app_root, db_path);
    }

    build_keyring_from_records(app_root, records)
}

fn load_keyring_legacy(app_root: &Path, db_path: &Path) -> Result<KeyRing> {
    let handle = crate::secret_store_crypto::ensure_secret_key(app_root, db_path)?;
    let key_id = handle.key_id().to_string();
    let record = KeyRecord {
        key_id: key_id.clone(),
        state: KeyState::Active,
        fingerprint: handle.fingerprint().to_string(),
        file_path: crate::secret_store_crypto::secret_key_path(app_root)
            .to_string_lossy()
            .to_string(),
        created_at: now_ts(),
        activated_at: Some(now_ts()),
        rotated_out_at: None,
        retired_at: None,
        revoked_at: None,
    };
    let mut decrypt_keys = HashMap::new();
    decrypt_keys.insert(key_id, handle.clone());
    Ok(KeyRing {
        records: vec![record],
        active_key: Some(handle),
        decrypt_keys,
    })
}

fn build_keyring_from_records(app_root: &Path, records: Vec<KeyRecord>) -> Result<KeyRing> {
    let mut active_key = None;
    let mut decrypt_keys = HashMap::new();

    for record in &records {
        if record.state.is_terminal() {
            continue;
        }
        let key_path = resolve_key_file_path(app_root, &record.file_path);
        if let Some(handle) = load_key_file(&key_path, &record.key_id)? {
            if record.state == KeyState::Active {
                active_key = Some(handle.clone());
            }
            decrypt_keys.insert(record.key_id.clone(), handle);
        }
    }

    Ok(KeyRing {
        records,
        active_key,
        decrypt_keys,
    })
}

fn resolve_key_file_path(app_root: &Path, file_path: &str) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        app_root.join(file_path)
    }
}

fn load_key_file(path: &Path, key_id: &str) -> Result<Option<SecretKeyHandle>> {
    if !path.exists() {
        return Ok(None);
    }
    crate::secret_store_crypto::load_key_file_as_handle(path, key_id)
        .map(Some)
        .with_context(|| format!("failed to load key file for key_id '{key_id}'"))
}

// ─── DB queries ──────────────────────────────────────────────────

pub fn query_all_key_records(conn: &Connection) -> Result<Vec<KeyRecord>> {
    let mut stmt = conn.prepare(
        "SELECT key_id, state, fingerprint, file_path, created_at, activated_at, rotated_out_at, retired_at, revoked_at
         FROM secret_keys ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;

    let mut records = Vec::new();
    for row in rows {
        let (
            key_id,
            state_str,
            fingerprint,
            file_path,
            created_at,
            activated_at,
            rotated_out_at,
            retired_at,
            revoked_at,
        ) = row?;
        records.push(KeyRecord {
            key_id,
            state: KeyState::from_str_value(&state_str)?,
            fingerprint,
            file_path,
            created_at,
            activated_at,
            rotated_out_at,
            retired_at,
            revoked_at,
        });
    }
    Ok(records)
}

fn query_active_key_record(conn: &Connection) -> Result<Option<KeyRecord>> {
    Ok(query_all_key_records(conn)?
        .into_iter()
        .find(|r| r.state == KeyState::Active))
}

// ─── Key ID generation ───────────────────────────────────────────

fn generate_key_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let rand_part: u16 = rand::random();
    format!("k-{ts}-{rand_part:04x}")
}

// ─── Rotation ────────────────────────────────────────────────────

/// Begin key rotation: generate new key, set old active to decrypt_only.
/// Returns (new_active_record, old_decrypt_only_record).
pub fn begin_rotation(conn: &Connection, app_root: &Path) -> Result<(KeyRecord, KeyRecord)> {
    let old_active = query_active_key_record(conn)?
        .ok_or_else(|| anyhow::anyhow!("no active key found; cannot begin rotation"))?;

    // Check for existing incomplete rotation
    let records = query_all_key_records(conn)?;
    if records.iter().any(|r| r.state == KeyState::DecryptOnly) {
        bail!("incomplete rotation detected: a key is already in decrypt_only state; use --resume to complete the previous rotation first");
    }

    let new_key_id = generate_key_id();
    let keys_dir = app_root.join("data/secrets/keys");
    std::fs::create_dir_all(&keys_dir)
        .with_context(|| format!("failed to create keys dir {}", keys_dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&keys_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let new_key_path = keys_dir.join(format!("{new_key_id}.key"));
    let handle =
        crate::secret_store_crypto::generate_and_write_key_file(&new_key_path, &new_key_id)?;
    let now = now_ts();

    let new_file_path = format!("data/secrets/keys/{new_key_id}.key");
    let new_record = KeyRecord {
        key_id: new_key_id.clone(),
        state: KeyState::Active,
        fingerprint: handle.fingerprint().to_string(),
        file_path: new_file_path.clone(),
        created_at: now.clone(),
        activated_at: Some(now.clone()),
        rotated_out_at: None,
        retired_at: None,
        revoked_at: None,
    };

    // Insert new active key
    conn.execute(
        "INSERT INTO secret_keys (key_id, state, fingerprint, file_path, created_at, activated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            new_key_id,
            KeyState::Active.as_str(),
            handle.fingerprint(),
            new_file_path,
            now,
            now
        ],
    )?;

    // Demote old active to decrypt_only
    conn.execute(
        "UPDATE secret_keys SET state = ?1, rotated_out_at = ?2 WHERE key_id = ?3",
        params![KeyState::DecryptOnly.as_str(), now, old_active.key_id],
    )?;

    let old_record = KeyRecord {
        state: KeyState::DecryptOnly,
        rotated_out_at: Some(now.clone()),
        ..old_active
    };

    // Audit events
    crate::secret_key_audit::insert_key_audit_event(
        conn,
        &audit_event_for_record(
            crate::secret_key_audit::KeyAuditEventKind::KeyCreated,
            &new_record,
            "cli:rotate",
            "{}".to_string(),
            &now,
        ),
    )?;
    crate::secret_key_audit::insert_key_audit_event(
        conn,
        &audit_event_for_record(
            crate::secret_key_audit::KeyAuditEventKind::KeyActivated,
            &new_record,
            "cli:rotate",
            "{}".to_string(),
            &now,
        ),
    )?;
    crate::secret_key_audit::insert_key_audit_event(
        conn,
        &audit_event_for_record(
            crate::secret_key_audit::KeyAuditEventKind::RotateStarted,
            &old_record,
            "cli:rotate",
            serde_json::json!({
                "new_key_id": new_record.key_id,
                "old_key_id": old_record.key_id,
            })
            .to_string(),
            &now,
        ),
    )?;

    Ok((new_record, old_record))
}

// ─── Re-encryption ──────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ReEncryptionReport {
    pub resources_updated: usize,
    pub versions_updated: usize,
    pub errors: Vec<String>,
}

/// Re-encrypt all SecretStore resources from old_encryption to new_encryption.
/// Runs in a single transaction for atomicity.
pub fn re_encrypt_all_secrets(
    conn: &Connection,
    old_encryption: &SecretEncryption,
    new_encryption: &SecretEncryption,
) -> Result<ReEncryptionReport> {
    let mut report = ReEncryptionReport::default();

    let tx = conn
        .unchecked_transaction()
        .context("failed to begin re-encryption transaction")?;

    // Re-encrypt resources table
    {
        let mut stmt = tx.prepare(
            "SELECT rowid, project, name, spec_json FROM resources WHERE kind = 'SecretStore'",
        )?;
        let rows: Vec<(i64, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for (rowid, project, name, spec_json) in rows {
            match re_encrypt_single(old_encryption, new_encryption, &project, &name, &spec_json) {
                Ok(new_spec_json) => {
                    tx.execute(
                        "UPDATE resources SET spec_json = ?1 WHERE rowid = ?2",
                        params![new_spec_json, rowid],
                    )?;
                    report.resources_updated += 1;
                }
                Err(e) => {
                    report
                        .errors
                        .push(format!("SecretStore/{project}/{name}: {e}"));
                }
            }
        }
    }

    // Re-encrypt resource_versions table
    {
        let mut stmt = tx.prepare(
            "SELECT id, project, name, spec_json FROM resource_versions WHERE kind = 'SecretStore' AND version > 0",
        )?;
        let rows: Vec<(i64, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for (id, project, name, spec_json) in rows {
            match re_encrypt_single(old_encryption, new_encryption, &project, &name, &spec_json) {
                Ok(new_spec_json) => {
                    tx.execute(
                        "UPDATE resource_versions SET spec_json = ?1 WHERE id = ?2",
                        params![new_spec_json, id],
                    )?;
                    report.versions_updated += 1;
                }
                Err(e) => {
                    report.errors.push(format!(
                        "resource_versions SecretStore/{project}/{name}: {e}"
                    ));
                }
            }
        }
    }

    tx.commit()
        .context("failed to commit re-encryption transaction")?;
    Ok(report)
}

fn re_encrypt_single(
    old_enc: &SecretEncryption,
    new_enc: &SecretEncryption,
    project: &str,
    name: &str,
    spec_json: &str,
) -> Result<String> {
    if !crate::secret_store_crypto::is_encrypted_secret_store_json(spec_json) {
        return Ok(spec_json.to_string());
    }
    let plaintext = old_enc.decrypt_secret_store_spec(project, name, spec_json)?;
    new_enc.encrypt_secret_store_spec(project, name, &plaintext)
}

// ─── Complete Rotation ───────────────────────────────────────────

/// Verify no data references old key, then retire it.
pub fn complete_rotation(conn: &Connection, old_key_id: &str) -> Result<()> {
    let now = now_ts();

    // Check that old key is decrypt_only
    let records = query_all_key_records(conn)?;
    let old_record = records
        .iter()
        .find(|r| r.key_id == old_key_id)
        .ok_or_else(|| anyhow::anyhow!("key '{old_key_id}' not found"))?;

    if old_record.state != KeyState::DecryptOnly {
        bail!(
            "key '{old_key_id}' is in state '{}', expected 'decrypt_only'",
            old_record.state
        );
    }

    // Verify no resources still reference old key
    let still_referenced: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM resources
            WHERE kind = 'SecretStore'
              AND instr(spec_json, ?1) > 0
        )",
        params![format!("\"key_id\":\"{old_key_id}\"")],
        |row| row.get(0),
    )?;

    if still_referenced {
        bail!("cannot complete rotation: some resources still reference key '{old_key_id}'");
    }

    conn.execute(
        "UPDATE secret_keys SET state = ?1, retired_at = ?2 WHERE key_id = ?3",
        params![KeyState::Retired.as_str(), now, old_key_id],
    )?;

    crate::secret_key_audit::insert_key_audit_event(
        conn,
        &crate::secret_key_audit::KeyAuditEvent {
            event_kind: crate::secret_key_audit::KeyAuditEventKind::RotateCompleted,
            key_id: old_key_id.to_string(),
            key_fingerprint: old_record.fingerprint.clone(),
            actor: "cli:rotate".to_string(),
            detail_json: "{}".to_string(),
            created_at: now,
        },
    )?;

    Ok(())
}

// ─── Resume Rotation ─────────────────────────────────────────────

/// Resume an incomplete rotation: find decrypt_only key and re-encrypt remaining data.
pub fn resume_rotation(conn: &Connection, app_root: &Path) -> Result<ReEncryptionReport> {
    let records = query_all_key_records(conn)?;
    let old_record = records
        .iter()
        .find(|r| r.state == KeyState::DecryptOnly)
        .ok_or_else(|| {
            anyhow::anyhow!("no incomplete rotation found (no key in decrypt_only state)")
        })?;
    let new_record = records
        .iter()
        .find(|r| r.state == KeyState::Active)
        .ok_or_else(|| anyhow::anyhow!("no active key found to complete rotation"))?;

    let old_key_path = resolve_key_file_path(app_root, &old_record.file_path);
    let new_key_path = resolve_key_file_path(app_root, &new_record.file_path);

    let old_handle =
        crate::secret_store_crypto::load_key_file_as_handle(&old_key_path, &old_record.key_id)?;
    let new_handle =
        crate::secret_store_crypto::load_key_file_as_handle(&new_key_path, &new_record.key_id)?;

    let old_encryption = SecretEncryption::from_key(old_handle);
    let new_encryption = SecretEncryption::from_key(new_handle);

    let report = re_encrypt_all_secrets(conn, &old_encryption, &new_encryption)?;

    if report.errors.is_empty() {
        complete_rotation(conn, &old_record.key_id)?;
    }

    Ok(report)
}

// ─── Revoke ──────────────────────────────────────────────────────

pub fn revoke_key(conn: &Connection, key_id: &str, force: bool) -> Result<()> {
    let records = query_all_key_records(conn)?;
    let record = records
        .iter()
        .find(|r| r.key_id == key_id)
        .ok_or_else(|| anyhow::anyhow!("key '{key_id}' not found"))?;

    if record.state.is_terminal() {
        bail!(
            "key '{key_id}' is already in terminal state '{}'",
            record.state
        );
    }

    if record.state == KeyState::Active && !force {
        bail!("refusing to revoke active key '{key_id}' without --force; this will block all SecretStore writes");
    }

    let now = now_ts();
    conn.execute(
        "UPDATE secret_keys SET state = ?1, revoked_at = ?2 WHERE key_id = ?3",
        params![KeyState::Revoked.as_str(), now, key_id],
    )?;

    crate::secret_key_audit::insert_key_audit_event(
        conn,
        &crate::secret_key_audit::KeyAuditEvent {
            event_kind: crate::secret_key_audit::KeyAuditEventKind::KeyRevoked,
            key_id: key_id.to_string(),
            key_fingerprint: record.fingerprint.clone(),
            actor: "cli:revoke".to_string(),
            detail_json: serde_json::json!({ "force": force }).to_string(),
            created_at: now,
        },
    )?;

    Ok(())
}

// ─── Migration helper: import legacy key ─────────────────────────

pub fn import_legacy_key_record(conn: &Connection, app_root: &Path) -> Result<Option<KeyRecord>> {
    let legacy_path = crate::secret_store_crypto::secret_key_path(app_root);
    if !legacy_path.exists() {
        return Ok(None);
    }

    let handle = match crate::secret_store_crypto::load_existing_secret_key(app_root)? {
        Some(h) => h,
        None => return Ok(None),
    };

    let now = now_ts();
    let relative_path = "data/secrets/secretstore.key";

    let record = KeyRecord {
        key_id: handle.key_id().to_string(),
        state: KeyState::Active,
        fingerprint: handle.fingerprint().to_string(),
        file_path: relative_path.to_string(),
        created_at: now.clone(),
        activated_at: Some(now.clone()),
        rotated_out_at: None,
        retired_at: None,
        revoked_at: None,
    };

    conn.execute(
        "INSERT OR IGNORE INTO secret_keys (key_id, state, fingerprint, file_path, created_at, activated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            record.key_id,
            record.state.as_str(),
            record.fingerprint,
            record.file_path,
            now,
            now
        ],
    )?;

    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("data/agent_orchestrator.db");
        std::fs::create_dir_all(db_path.parent().expect("parent")).expect("create data dir");
        crate::db::init_schema(&db_path).expect("init schema");
        (temp, db_path)
    }

    #[test]
    fn key_state_round_trip() {
        for state in [
            KeyState::Active,
            KeyState::DecryptOnly,
            KeyState::Revoked,
            KeyState::Retired,
        ] {
            assert_eq!(KeyState::from_str_value(state.as_str()).unwrap(), state);
        }
    }

    #[test]
    fn terminal_states() {
        assert!(!KeyState::Active.is_terminal());
        assert!(!KeyState::DecryptOnly.is_terminal());
        assert!(KeyState::Revoked.is_terminal());
        assert!(KeyState::Retired.is_terminal());
    }

    #[test]
    fn load_keyring_from_legacy_key() {
        let (temp, db_path) = setup_test_db();
        // Ensure legacy key exists
        crate::secret_store_crypto::ensure_secret_key(temp.path(), &db_path).expect("ensure key");

        let keyring = load_keyring(temp.path(), &db_path).expect("load keyring");
        assert!(keyring.has_active_key());
        assert_eq!(keyring.all_records().len(), 1);
        assert_eq!(keyring.all_records()[0].state, KeyState::Active);
    }

    #[test]
    fn begin_rotation_creates_new_key_and_demotes_old() {
        let (temp, db_path) = setup_test_db();
        crate::secret_store_crypto::ensure_secret_key(temp.path(), &db_path).expect("ensure key");

        let conn = crate::db::open_conn(&db_path).expect("open");
        // Import legacy key to DB
        import_legacy_key_record(&conn, temp.path()).expect("import legacy");

        let (new_rec, old_rec) = begin_rotation(&conn, temp.path()).expect("begin rotation");
        assert_eq!(new_rec.state, KeyState::Active);
        assert_eq!(old_rec.state, KeyState::DecryptOnly);
        assert!(new_rec.key_id.starts_with("k-"));
    }

    #[test]
    fn revoke_active_key_requires_force() {
        let (temp, db_path) = setup_test_db();
        crate::secret_store_crypto::ensure_secret_key(temp.path(), &db_path).expect("ensure key");

        let conn = crate::db::open_conn(&db_path).expect("open");
        import_legacy_key_record(&conn, temp.path()).expect("import legacy");

        let records = query_all_key_records(&conn).expect("query");
        let active_id = &records[0].key_id;

        let err = revoke_key(&conn, active_id, false).expect_err("should require force");
        assert!(err.to_string().contains("--force"));

        revoke_key(&conn, active_id, true).expect("force revoke should succeed");

        let records_after = query_all_key_records(&conn).expect("query after");
        assert_eq!(records_after[0].state, KeyState::Revoked);
    }

    #[test]
    fn full_rotation_lifecycle() {
        let (temp, db_path) = setup_test_db();
        let handle =
            crate::secret_store_crypto::ensure_secret_key(temp.path(), &db_path).expect("ensure");
        let enc = SecretEncryption::from_key(handle);

        // Encrypt some data
        let spec = serde_json::json!({"data": {"API_KEY": "sk-test"}});
        let cipher = enc
            .encrypt_secret_store_spec("default", "test-secret", &spec)
            .expect("encrypt");

        let conn = crate::db::open_conn(&db_path).expect("open");
        conn.execute(
            "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at)
             VALUES ('SecretStore', 'default', 'test-secret', 'v2', ?1, '{}', 1, datetime('now'), datetime('now'))",
            params![cipher],
        ).expect("insert");

        // Import legacy key and begin rotation
        import_legacy_key_record(&conn, temp.path()).expect("import");
        let (new_rec, old_rec) = begin_rotation(&conn, temp.path()).expect("begin rotation");

        // Build encryptions for re-encryption
        let old_key_path = resolve_key_file_path(temp.path(), &old_rec.file_path);
        let new_key_path = resolve_key_file_path(temp.path(), &new_rec.file_path);
        let old_handle =
            crate::secret_store_crypto::load_key_file_as_handle(&old_key_path, &old_rec.key_id)
                .expect("load old");
        let new_handle =
            crate::secret_store_crypto::load_key_file_as_handle(&new_key_path, &new_rec.key_id)
                .expect("load new");

        let report = re_encrypt_all_secrets(
            &conn,
            &SecretEncryption::from_key(old_handle),
            &SecretEncryption::from_key(new_handle.clone()),
        )
        .expect("re-encrypt");
        assert_eq!(report.resources_updated, 1);
        assert!(report.errors.is_empty());

        // Complete rotation
        complete_rotation(&conn, &old_rec.key_id).expect("complete rotation");

        let records = query_all_key_records(&conn).expect("query");
        let old = records
            .iter()
            .find(|r| r.key_id == old_rec.key_id)
            .expect("find old");
        assert_eq!(old.state, KeyState::Retired);

        // Verify data is readable with new key
        let new_enc = SecretEncryption::from_key(new_handle);
        let spec_json: String = conn
            .query_row(
                "SELECT spec_json FROM resources WHERE kind='SecretStore' AND name='test-secret'",
                [],
                |row| row.get(0),
            )
            .expect("load");
        let decrypted = new_enc
            .decrypt_secret_store_spec("default", "test-secret", &spec_json)
            .expect("decrypt");
        assert_eq!(decrypted, spec);
    }
}
