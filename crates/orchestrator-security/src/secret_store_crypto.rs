use aes_gcm_siv::Aes256GcmSiv;
use aes_gcm_siv::aead::{Aead, KeyInit, Payload};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use orchestrator_config::resource_store::SYSTEM_PROJECT;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const KEY_RELATIVE_PATH: &str = "secrets/secretstore.key";
const KEY_META_RELATIVE_PATH: &str = "secrets/secretstore.key.meta.json";
const KEY_ID_PRIMARY: &str = "primary";
const KEY_SIZE_BYTES: usize = 32;
const NONCE_SIZE_BYTES: usize = 12;
/// SecretStore envelope scheme label persisted in encrypted specs.
pub const SECRETSTORE_ENCRYPTION_SCHEME: &str = "secretstore.aead.v1";
/// Redaction placeholder written when secret values must be hidden.
pub const ENCRYPTED_PLACEHOLDER: &str = "[ENCRYPTED]";

#[derive(Debug, Clone)]
/// In-memory handle for one loaded SecretStore encryption key.
pub struct SecretKeyHandle {
    key_bytes: [u8; KEY_SIZE_BYTES],
    key_id: String,
    fingerprint: String,
    #[allow(dead_code)]
    path: PathBuf,
}

impl SecretKeyHandle {
    /// Returns the stable identifier associated with this key handle.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Returns the stable fingerprint derived from the key material.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn key_bytes(&self) -> &[u8; KEY_SIZE_BYTES] {
        &self.key_bytes
    }
}

#[derive(Debug, Clone)]
/// Encrypts and decrypts SecretStore specs using one active key plus optional decrypt-only keys.
pub struct SecretEncryption {
    key: SecretKeyHandle,
    /// Additional keys available for decryption (key_id → handle).
    /// When constructed via `from_key`, this is empty (single-key mode).
    decrypt_keys: std::collections::HashMap<String, SecretKeyHandle>,
}

impl SecretEncryption {
    /// Creates a single-key encryptor and decryptor from one key handle.
    pub fn from_key(key: SecretKeyHandle) -> Self {
        Self {
            key,
            decrypt_keys: std::collections::HashMap::new(),
        }
    }

    /// Build from a KeyRing — active key for encryption, all non-terminal keys for decryption.
    pub fn from_keyring(keyring: &crate::secret_key_lifecycle::KeyRing) -> Result<Self> {
        let active = keyring.active_key()?.clone();
        let mut decrypt_keys = std::collections::HashMap::new();
        for (kid, handle) in keyring.decrypt_keys_iter() {
            decrypt_keys.insert(kid.to_string(), handle.clone());
        }
        Ok(Self {
            key: active,
            decrypt_keys,
        })
    }

    /// Encrypts a SecretStore spec into an authenticated envelope bound to resource identity.
    pub fn encrypt_secret_store_spec(
        &self,
        project: &str,
        name: &str,
        spec: &Value,
    ) -> Result<String> {
        let plain = serde_json::to_vec(spec).context("failed to serialize secret store spec")?;
        let aad = SecretEnvelopeAad {
            kind: "SecretStore".to_string(),
            project: project.to_string(),
            name: name.to_string(),
        };
        let cipher = Aes256GcmSiv::new_from_slice(self.key.key_bytes())
            .map_err(|_| anyhow!("failed to initialize secret store cipher"))?;
        let mut nonce_bytes = [0_u8; NONCE_SIZE_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = aes_gcm_siv::Nonce::from_slice(&nonce_bytes);
        let aad_json = serde_json::to_vec(&aad).context("failed to serialize secret AAD")?;
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &plain,
                    aad: &aad_json,
                },
            )
            .map_err(|_| anyhow!("failed to encrypt secret store spec"))?;
        let envelope = SecretEnvelope {
            encrypted: true,
            scheme: SECRETSTORE_ENCRYPTION_SCHEME.to_string(),
            key_id: self.key.key_id().to_string(),
            nonce: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
            ciphertext: base64::engine::general_purpose::STANDARD.encode(ciphertext),
            aad,
        };
        serde_json::to_string(&envelope).context("failed to serialize encrypted secret envelope")
    }

    /// Decrypts a SecretStore envelope and verifies its authenticated resource identity.
    pub fn decrypt_secret_store_spec(
        &self,
        project: &str,
        name: &str,
        spec_json: &str,
    ) -> Result<Value> {
        let envelope: SecretEnvelope =
            serde_json::from_str(spec_json).context("failed to parse encrypted secret envelope")?;
        if !envelope.encrypted {
            bail!("secret store envelope missing encrypted marker");
        }
        if envelope.scheme != SECRETSTORE_ENCRYPTION_SCHEME {
            bail!(
                "unsupported secret store encryption scheme: {}",
                envelope.scheme
            );
        }
        if envelope.aad.kind != "SecretStore"
            || envelope.aad.project != project
            || envelope.aad.name != name
        {
            bail!(
                "secret store envelope AAD mismatch for SecretStore/{}/{}",
                project,
                name
            );
        }

        // Multi-key dispatch: look up the key matching envelope.key_id
        let decrypt_handle = self.resolve_decrypt_key(&envelope.key_id)?;

        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(&envelope.nonce)
            .context("failed to decode secret envelope nonce")?;
        if nonce_bytes.len() != NONCE_SIZE_BYTES {
            bail!("invalid secret envelope nonce length");
        }
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(&envelope.ciphertext)
            .context("failed to decode secret envelope ciphertext")?;
        let cipher = Aes256GcmSiv::new_from_slice(decrypt_handle.key_bytes())
            .map_err(|_| anyhow!("failed to initialize secret store cipher"))?;
        let aad_json =
            serde_json::to_vec(&envelope.aad).context("failed to serialize envelope AAD")?;
        let plain = cipher
            .decrypt(
                aes_gcm_siv::Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &ciphertext,
                    aad: &aad_json,
                },
            )
            .map_err(|_| {
                anyhow!(
                    "failed to decrypt secret store spec (key_id: {})",
                    envelope.key_id
                )
            })?;
        serde_json::from_slice(&plain).context("failed to parse decrypted secret store spec")
    }

    /// Resolve which key handle to use for decryption.
    /// Checks decrypt_keys map first, then falls back to the primary key.
    fn resolve_decrypt_key(&self, key_id: &str) -> Result<&SecretKeyHandle> {
        if let Some(handle) = self.decrypt_keys.get(key_id) {
            return Ok(handle);
        }
        if self.key.key_id() == key_id {
            return Ok(&self.key);
        }
        bail!(
            "no decryption key available for key_id '{}'; available keys: [{}]",
            key_id,
            {
                let mut ids: Vec<&str> = self.decrypt_keys.keys().map(|s| s.as_str()).collect();
                ids.push(self.key.key_id());
                ids.join(", ")
            }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretKeyMetadata {
    key_id: String,
    created_at: String,
    last_rotated_at: String,
    fingerprint: String,
    format_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretEnvelopeAad {
    kind: String,
    project: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretEnvelope {
    #[serde(rename = "_encrypted")]
    encrypted: bool,
    scheme: String,
    key_id: String,
    nonce: String,
    ciphertext: String,
    aad: SecretEnvelopeAad,
}

/// Load a key file and return a SecretKeyHandle with the given key_id.
pub fn load_key_file_as_handle(path: &Path, key_id: &str) -> Result<SecretKeyHandle> {
    validate_secret_key_permissions(path)?;
    let encoded = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read key file {}", path.display()))?;
    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded.trim())
            .context("failed to decode key file")?;
    if decoded.len() != KEY_SIZE_BYTES {
        bail!(
            "invalid key length in {}: expected {} bytes",
            path.display(),
            KEY_SIZE_BYTES
        );
    }
    let mut key_bytes = [0_u8; KEY_SIZE_BYTES];
    key_bytes.copy_from_slice(&decoded);
    let fingerprint = key_fingerprint(&key_bytes);
    Ok(SecretKeyHandle {
        key_bytes,
        key_id: key_id.to_string(),
        fingerprint,
        path: path.to_path_buf(),
    })
}

/// Generate a new random key and write it to the given path with proper permissions.
/// Returns the SecretKeyHandle for the new key.
pub fn generate_and_write_key_file(path: &Path, key_id: &str) -> Result<SecretKeyHandle> {
    let mut key_bytes = [0_u8; KEY_SIZE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);
    let encoded = base64::engine::general_purpose::STANDARD.encode(key_bytes);
    write_atomic_secret_file(path, encoded.as_bytes())?;
    let fingerprint = key_fingerprint(&key_bytes);
    Ok(SecretKeyHandle {
        key_bytes,
        key_id: key_id.to_string(),
        fingerprint,
        path: path.to_path_buf(),
    })
}

/// Resolves the canonical path of the primary SecretStore key file.
pub fn secret_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join(KEY_RELATIVE_PATH)
}

/// Resolves the path of the metadata file associated with the primary SecretStore key.
pub fn secret_key_meta_path(data_dir: &Path) -> PathBuf {
    data_dir.join(KEY_META_RELATIVE_PATH)
}

/// Resolves the application root from a database path in either nested or flat layouts.
pub fn resolve_data_dir_from_db_path(db_path: &Path) -> Result<PathBuf> {
    let parent = db_path
        .parent()
        .with_context(|| format!("db path has no parent: {}", db_path.display()))?;
    if parent.file_name().and_then(|s| s.to_str()) == Some("data") {
        parent
            .parent()
            .map(Path::to_path_buf)
            .with_context(|| format!("data dir has no parent: {}", parent.display()))
    } else {
        Ok(parent.to_path_buf())
    }
}

/// Loads the existing primary key or initializes one when no encrypted SecretStore data exists.
pub fn ensure_secret_key(data_dir: &Path, db_path: &Path) -> Result<SecretKeyHandle> {
    if let Some(existing) = load_existing_secret_key(data_dir)? {
        return Ok(existing);
    }
    if encrypted_secret_data_exists(db_path)? {
        bail!(
            "secret store key missing at {} while encrypted SecretStore data exists; restore the original key before starting",
            secret_key_path(data_dir).display()
        );
    }
    initialize_secret_key(data_dir)
}

/// Loads the primary SecretStore key if it already exists on disk.
pub fn load_existing_secret_key(data_dir: &Path) -> Result<Option<SecretKeyHandle>> {
    let path = secret_key_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    validate_secret_key_permissions(&path)?;
    let encoded = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read secret key file {}", path.display()))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .context("failed to decode secret key file")?;
    if decoded.len() != KEY_SIZE_BYTES {
        bail!(
            "invalid secret key length: expected {} bytes",
            KEY_SIZE_BYTES
        );
    }
    let mut key_bytes = [0_u8; KEY_SIZE_BYTES];
    key_bytes.copy_from_slice(&decoded);
    let fingerprint = key_fingerprint(&key_bytes);
    Ok(Some(SecretKeyHandle {
        key_bytes,
        key_id: KEY_ID_PRIMARY.to_string(),
        fingerprint,
        path,
    }))
}

/// Returns `true` when the serialized spec looks like an encrypted SecretStore envelope.
pub fn is_encrypted_secret_store_json(spec_json: &str) -> bool {
    spec_json.contains("\"scheme\":\"secretstore.aead.v1\"")
        || spec_json.contains("\"_encrypted\":true")
}

/// Replaces every secret value in a `data` map with the standard redaction marker.
pub fn redact_secret_data_map(map: &mut serde_json::Map<String, Value>) {
    for value in map.values_mut() {
        *value = Value::String(ENCRYPTED_PLACEHOLDER.to_string());
    }
}

fn initialize_secret_key(data_dir: &Path) -> Result<SecretKeyHandle> {
    let key_path = secret_key_path(data_dir);
    let meta_path = secret_key_meta_path(data_dir);
    let secrets_dir = key_path
        .parent()
        .with_context(|| format!("secret key path has no parent: {}", key_path.display()))?;
    std::fs::create_dir_all(secrets_dir)
        .with_context(|| format!("failed to create secrets dir {}", secrets_dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(secrets_dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| {
                format!(
                    "failed to set permissions on secrets dir {}",
                    secrets_dir.display()
                )
            })?;
    }
    let mut key_bytes = [0_u8; KEY_SIZE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut key_bytes);
    let encoded = base64::engine::general_purpose::STANDARD.encode(key_bytes);
    write_atomic_secret_file(&key_path, encoded.as_bytes())?;
    let now = crate::now_ts();
    let metadata = SecretKeyMetadata {
        key_id: KEY_ID_PRIMARY.to_string(),
        created_at: now.clone(),
        last_rotated_at: now,
        fingerprint: key_fingerprint(&key_bytes),
        format_version: 1,
    };
    let meta_json =
        serde_json::to_vec_pretty(&metadata).context("failed to serialize key metadata")?;
    write_atomic_secret_file(&meta_path, &meta_json)?;
    Ok(SecretKeyHandle {
        key_bytes,
        key_id: metadata.key_id,
        fingerprint: metadata.fingerprint,
        path: key_path,
    })
}

fn write_atomic_secret_file(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp_path = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!("{ext}."))
            .unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .with_context(|| {
            format!(
                "failed to create temporary secret file {}",
                tmp_path.display()
            )
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .with_context(|| {
                format!(
                    "failed to set permissions on temporary secret file {}",
                    tmp_path.display()
                )
            })?;
    }
    file.write_all(contents).with_context(|| {
        format!(
            "failed to write temporary secret file {}",
            tmp_path.display()
        )
    })?;
    file.sync_all().with_context(|| {
        format!(
            "failed to fsync temporary secret file {}",
            tmp_path.display()
        )
    })?;
    drop(file);
    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename temporary secret file {} -> {}",
            tmp_path.display(),
            path.display()
        )
    })?;
    validate_secret_key_permissions(path)?;
    Ok(())
}

fn encrypted_secret_data_exists(db_path: &Path) -> Result<bool> {
    let conn = crate::open_conn(db_path)?;
    let resources_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='resources'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    let versions_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='resource_versions'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    let mut encrypted = false;
    if resources_exists {
        encrypted = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM resources
                WHERE kind = 'SecretStore'
                  AND (instr(spec_json, '\"scheme\":\"secretstore.aead.v1\"') > 0
                       OR instr(spec_json, '\"_encrypted\":true') > 0)
            )",
            [],
            |row| row.get(0),
        )?;
    }
    if !encrypted && versions_exists {
        encrypted = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM resource_versions
                WHERE kind = 'SecretStore'
                  AND version > 0
                  AND (instr(spec_json, '\"scheme\":\"secretstore.aead.v1\"') > 0
                       OR instr(spec_json, '\"_encrypted\":true') > 0)
            )",
            [],
            |row| row.get(0),
        )?;
    }
    Ok(encrypted)
}

fn validate_secret_key_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("failed to read secret key metadata {}", path.display()))?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            bail!(
                "secret key file {} must have permissions 0600 or stricter (found {:o})",
                path.display(),
                mode
            );
        }
    }
    Ok(())
}

fn key_fingerprint(key_bytes: &[u8; KEY_SIZE_BYTES]) -> String {
    let digest = Sha256::digest(key_bytes);
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Decodes a stored resource spec, decrypting SecretStore payloads when needed.
pub fn decrypt_resource_spec_json(
    encryption: Option<&SecretEncryption>,
    kind: &str,
    project: &str,
    name: &str,
    spec_json: &str,
) -> Result<Value> {
    if kind != "SecretStore" {
        return serde_json::from_str(spec_json).context("failed to parse resource spec json");
    }
    if !is_encrypted_secret_store_json(spec_json) {
        return serde_json::from_str(spec_json)
            .context("failed to parse plaintext secret store spec json");
    }
    let encryption = encryption.ok_or_else(|| {
        anyhow!(
            "encrypted SecretStore/{}/{} cannot be loaded because the secret key is unavailable",
            project,
            name
        )
    })?;
    encryption.decrypt_secret_store_spec(project, name, spec_json)
}

/// Encodes a resource spec, encrypting SecretStore payloads and plain-serializing others.
pub fn encrypt_resource_spec_json(
    encryption: &SecretEncryption,
    kind: &str,
    project: &str,
    name: &str,
    spec: &Value,
) -> Result<String> {
    if kind == "SecretStore" {
        encryption.encrypt_secret_store_spec(project, name, spec)
    } else {
        serde_json::to_string(spec).context("failed to serialize resource spec json")
    }
}

/// Returns the provided project identifier or the system project when missing.
pub fn secret_project_or_default(project: Option<&str>) -> &str {
    project
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(SYSTEM_PROJECT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ensure_secret_key_creates_and_reuses_key_file() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("data/agent_orchestrator.db");
        std::fs::create_dir_all(db_path.parent().expect("db path should have parent"))
            .expect("create data dir");
        crate::init_test_schema(&db_path).expect("init schema");

        let first = ensure_secret_key(temp.path(), &db_path).expect("create key");
        let second = ensure_secret_key(temp.path(), &db_path).expect("reuse key");

        assert_eq!(first.fingerprint(), second.fingerprint());
        assert!(secret_key_path(temp.path()).exists());
        assert!(secret_key_meta_path(temp.path()).exists());
    }

    #[test]
    fn encrypt_and_decrypt_secret_store_round_trip() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("agent_orchestrator.db");
        crate::init_test_schema(&db_path).expect("init schema");
        let key = ensure_secret_key(temp.path(), &db_path).expect("create key");
        let encryption = SecretEncryption::from_key(key);
        let spec = serde_json::json!({"data": {"API_KEY": "sk-123"}});

        let cipher = encryption
            .encrypt_secret_store_spec("default", "api-keys", &spec)
            .expect("encrypt");
        assert!(is_encrypted_secret_store_json(&cipher));
        assert!(!cipher.contains("sk-123"));

        let plain = encryption
            .decrypt_secret_store_spec("default", "api-keys", &cipher)
            .expect("decrypt");
        assert_eq!(plain, spec);
    }

    #[test]
    fn ensure_secret_key_refuses_to_regenerate_when_encrypted_data_exists() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("agent_orchestrator.db");
        crate::init_test_schema(&db_path).expect("init schema");
        let key = ensure_secret_key(temp.path(), &db_path).expect("create key");
        let encryption = SecretEncryption::from_key(key);
        let spec = serde_json::json!({"data": {"API_KEY": "sk-123"}});
        let cipher = encryption
            .encrypt_secret_store_spec("default", "api-keys", &spec)
            .expect("encrypt");
        let conn = crate::open_conn(&db_path).expect("open sqlite");
        conn.execute(
            "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at)
             VALUES ('SecretStore', 'default', 'api-keys', 'orchestrator.dev/v2', ?1, '{}', 1, datetime('now'), datetime('now'))",
            rusqlite::params![cipher],
        )
        .expect("insert encrypted secret resource");
        std::fs::remove_file(secret_key_path(temp.path())).expect("remove secret key");

        let err =
            ensure_secret_key(temp.path(), &db_path).expect_err("should refuse to regenerate");
        assert!(
            err.to_string()
                .contains("encrypted SecretStore data exists")
        );
    }

    #[test]
    fn resolve_data_dir_from_db_path_accepts_data_and_flat_layouts() {
        let temp = tempdir().expect("tempdir");
        let nested = temp.path().join("data/agent_orchestrator.db");
        let flat = temp.path().join("agent_orchestrator.db");

        assert_eq!(
            resolve_data_dir_from_db_path(&nested).expect("nested root"),
            temp.path()
        );
        assert_eq!(
            resolve_data_dir_from_db_path(&flat).expect("flat root"),
            temp.path()
        );
    }
}
