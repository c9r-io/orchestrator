use crate::config::OrchestratorConfig;
use crate::config_load::{now_ts, ConfigSelfHealChange, ResourceRemoval};
use crate::dto::ConfigOverview;
use crate::resource::export_manifest_resources;
use crate::secret_store_crypto::{
    decrypt_resource_spec_json, encrypt_resource_spec_json, ensure_secret_key,
    load_existing_secret_key, redact_secret_data_map, resolve_app_root_from_db_path,
    SecretEncryption,
};
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
/// One persisted config self-heal log entry.
pub struct HealLogEntry {
    /// Config version associated with the heal event.
    pub version: i64,
    /// Original validation error that triggered the heal.
    pub original_error: String,
    /// Workflow identifier containing the healed step.
    pub workflow_id: String,
    /// Step identifier affected by the heal.
    pub step_id: String,
    /// Stable self-heal rule label.
    pub rule: String,
    /// Human-readable change detail.
    pub detail: String,
    /// Timestamp when the heal log row was created.
    pub created_at: String,
}

/// Persistence interface for versioned orchestrator configuration snapshots.
pub trait ConfigRepository: Send + Sync {
    /// Loads the latest config snapshot or seeds the initial one when absent.
    fn load_or_seed_config(&self) -> Result<(OrchestratorConfig, String, i64, String)>;
    /// Loads the latest persisted config snapshot without seeding.
    fn load_config(&self) -> Result<Option<(OrchestratorConfig, i64, String)>>;
    /// Returns aggregate information about the latest self-heal run for the current version.
    fn query_latest_heal_summary(
        &self,
        current_config_version: i64,
    ) -> Result<Option<(i64, String, usize, String)>>;
    /// Returns recent self-heal log entries.
    fn query_heal_log_entries(&self, limit: usize) -> Result<Vec<HealLogEntry>>;
    /// Persists a self-healed config snapshot and its detailed change log.
    fn persist_self_heal_snapshot(
        &self,
        yaml: &str,
        json_raw: &str,
        original_error: &str,
        changes: &[ConfigSelfHealChange],
    ) -> Result<(i64, String)>;
    /// Persists a normalized config snapshot without resource deletions.
    fn persist_raw_config(
        &self,
        normalized: OrchestratorConfig,
        yaml: &str,
        json_raw: &str,
        author: &str,
    ) -> Result<ConfigOverview>;
    /// Persists a normalized config snapshot and records resource deletions.
    fn persist_config_with_deletions(
        &self,
        normalized: OrchestratorConfig,
        yaml: &str,
        json_raw: &str,
        author: &str,
        deleted_resources: &[ResourceRemoval],
    ) -> Result<ConfigOverview>;
}

/// SQLite-backed implementation of the config repository.
pub struct SqliteConfigRepository {
    db_path: PathBuf,
}

impl SqliteConfigRepository {
    /// Creates a config repository that reads and writes the given SQLite database.
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn open_conn(&self) -> Result<rusqlite::Connection> {
        crate::db::open_conn(&self.db_path)
    }
}

fn serialize_config_snapshot(config: &OrchestratorConfig) -> Result<(String, String)> {
    let sanitized = sanitized_config_snapshot(config);
    let yaml = export_manifest_resources(&sanitized)
        .iter()
        .map(crate::resource::Resource::to_yaml)
        .collect::<Result<Vec<_>>>()?
        .join("---\n");
    let json_raw = serde_json::to_string(&sanitized)?;
    Ok((yaml, json_raw))
}

fn sanitized_config_snapshot(config: &OrchestratorConfig) -> OrchestratorConfig {
    let mut sanitized = config.clone();
    for project in sanitized.projects.values_mut() {
        for store in project.env_stores.values_mut() {
            if store.sensitive {
                for value in store.data.values_mut() {
                    *value = crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER.to_string();
                }
            }
        }
    }
    for resource in sanitized.resource_store.resources_mut().values_mut() {
        if resource.kind != "SecretStore" {
            continue;
        }
        if let Some(spec) = resource.spec.as_object_mut() {
            if let Some(data) = spec.get_mut("data").and_then(|value| value.as_object_mut()) {
                redact_secret_data_map(data);
            }
        }
    }
    sanitized
}

fn emit_decrypt_failed_audit(
    conn: &rusqlite::Connection,
    project: &str,
    name: &str,
    error: &anyhow::Error,
) {
    // Best-effort: if the audit table doesn't exist yet, skip silently
    let _ = crate::secret_key_audit::insert_key_audit_event(
        conn,
        &crate::secret_key_audit::KeyAuditEvent {
            event_kind: crate::secret_key_audit::KeyAuditEventKind::DecryptFailed,
            key_id: "unknown".to_string(),
            key_fingerprint: "unknown".to_string(),
            actor: "system:load_resources".to_string(),
            detail_json: serde_json::json!({
                "project": project,
                "name": name,
                "error": error.to_string(),
            })
            .to_string(),
            created_at: now_ts(),
        },
    );
}

pub(crate) fn persist_config_versioned(
    tx: &Transaction<'_>,
    yaml: &str,
    json_raw: &str,
    author: &str,
) -> Result<(i64, String)> {
    let current_version: i64 = tx.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM orchestrator_config_versions",
        [],
        |row| row.get(0),
    )?;
    let next_version = current_version + 1;
    let now = now_ts();
    tx.execute(
        "INSERT INTO orchestrator_config_versions (version, config_yaml, config_json, created_at, author)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![next_version, yaml, json_raw, now, author],
    )?;
    Ok((next_version, now))
}

pub(crate) fn persist_heal_log(
    tx: &Transaction<'_>,
    version: i64,
    original_error: &str,
    changes: &[ConfigSelfHealChange],
) -> Result<()> {
    let now = now_ts();
    for change in changes {
        tx.execute(
            "INSERT INTO config_heal_log (version, original_error, workflow_id, step_id, rule, detail, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                version,
                original_error,
                change.workflow_id,
                change.step_id,
                change.rule.as_label(),
                change.detail,
                now
            ],
        )?;
    }
    Ok(())
}

fn persist_resource(
    tx: &Transaction<'_>,
    cr: &crate::crd::types::CustomResource,
    author: &str,
    secret_encryption: &SecretEncryption,
) -> Result<()> {
    let project = cr
        .metadata
        .project
        .as_deref()
        .filter(|project| !project.trim().is_empty())
        .unwrap_or(crate::crd::store::SYSTEM_PROJECT);

    // RuntimePolicy is project-scoped but also has a system-level default in
    // _system that serves as fallback for projects without their own policy.
    if crate::crd::store::is_project_scoped(&cr.kind)
        && project == crate::crd::store::SYSTEM_PROJECT
        && cr.kind != "RuntimePolicy"
    {
        anyhow::bail!(
            "project-scoped resource {}/{} must have an explicit project, not _system",
            cr.kind,
            cr.metadata.name
        );
    }

    let spec_json = encrypt_resource_spec_json(
        secret_encryption,
        &cr.kind,
        project,
        &cr.metadata.name,
        &cr.spec,
    )?;
    let metadata_json = serde_json::to_string(&cr.metadata)?;
    let now = now_ts();

    tx.execute(
        "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(kind, project, name) DO UPDATE SET
           api_version=excluded.api_version,
           spec_json=excluded.spec_json,
           metadata_json=excluded.metadata_json,
           generation=generation+1,
           updated_at=excluded.updated_at",
        params![
            cr.kind,
            project,
            cr.metadata.name,
            cr.api_version,
            spec_json,
            metadata_json,
            cr.generation,
            cr.created_at,
            now
        ],
    )?;

    let next_version: i64 = tx.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM resource_versions WHERE kind=?1 AND project=?2 AND name=?3",
        params![cr.kind, project, cr.metadata.name],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO resource_versions (kind, project, name, spec_json, metadata_json, version, author, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            cr.kind,
            project,
            cr.metadata.name,
            spec_json,
            metadata_json,
            next_version,
            author,
            now
        ],
    )?;
    Ok(())
}

fn persist_all_resources(
    tx: &Transaction<'_>,
    store: &crate::crd::store::ResourceStore,
    crds: &HashMap<String, crate::crd::types::CustomResourceDefinition>,
    author: &str,
    secret_encryption: &SecretEncryption,
) -> Result<()> {
    for cr in store.resources().values() {
        persist_resource(tx, cr, author, secret_encryption)?;
    }
    let now = now_ts();
    for (kind_name, crd) in crds {
        let spec_json = serde_json::to_string(crd)?;
        tx.execute(
            "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at)
             VALUES ('CustomResourceDefinition', ?1, ?2, 'orchestrator.dev/v2', ?3, '{}', 1, ?4, ?5)
             ON CONFLICT(kind, project, name) DO UPDATE SET
               spec_json=excluded.spec_json, generation=generation+1, updated_at=excluded.updated_at",
            params![crate::crd::store::SYSTEM_PROJECT, kind_name, spec_json, now, now],
        )?;
    }
    Ok(())
}

fn delete_resource_row(
    tx: &Transaction<'_>,
    kind: &str,
    project: &str,
    name: &str,
    author: &str,
) -> Result<bool> {
    let deleted = tx.execute(
        "DELETE FROM resources WHERE kind=?1 AND project=?2 AND name=?3",
        params![kind, project, name],
    )? > 0;
    if deleted {
        let now = now_ts();
        tx.execute(
            "INSERT INTO resource_versions (kind, project, name, spec_json, metadata_json, version, author, created_at)
             VALUES (?1, ?2, ?3, '\"deleted\"', '{}', -1, ?4, ?5)",
            params![kind, project, name, author, now],
        )?;
    }
    Ok(deleted)
}

fn load_all_resources(
    db_path: &Path,
) -> Result<(
    crate::crd::store::ResourceStore,
    HashMap<String, crate::crd::types::CustomResourceDefinition>,
)> {
    let app_root = resolve_app_root_from_db_path(db_path)?;
    // Try loading via KeyRing for multi-key support; fall back to single-key
    let secret_encryption = match crate::secret_key_lifecycle::load_keyring(&app_root, db_path) {
        Ok(keyring) => {
            if keyring.has_active_key() {
                SecretEncryption::from_keyring(&keyring).ok()
            } else {
                load_existing_secret_key(&app_root)?.map(SecretEncryption::from_key)
            }
        }
        Err(_) => load_existing_secret_key(&app_root)?.map(SecretEncryption::from_key),
    };
    let conn = crate::db::open_conn(db_path)?;
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='resources'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !table_exists {
        return Ok((crate::crd::store::ResourceStore::default(), HashMap::new()));
    }

    let mut store = crate::crd::store::ResourceStore::default();
    let mut crds = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at
         FROM resources",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;

    for row in rows {
        let (
            kind,
            project,
            name,
            api_version,
            spec_json,
            metadata_json,
            generation,
            created_at,
            updated_at,
        ) = row?;
        if kind == "CustomResourceDefinition" {
            if let Ok(crd) =
                serde_json::from_str::<crate::crd::types::CustomResourceDefinition>(&spec_json)
            {
                crds.insert(name, crd);
            }
            continue;
        }

        let spec = match decrypt_resource_spec_json(
            secret_encryption.as_ref(),
            &kind,
            &project,
            &name,
            &spec_json,
        ) {
            Ok(v) => v,
            Err(e) => {
                // Write DecryptFailed audit event (best-effort)
                if kind == "SecretStore" {
                    emit_decrypt_failed_audit(&conn, &project, &name, &e);
                }
                return Err(e)
                    .with_context(|| format!("failed to load resource {kind}/{project}/{name}"));
            }
        };

        let metadata: crate::cli_types::ResourceMetadata = serde_json::from_str(&metadata_json)
            .unwrap_or_else(|_| crate::cli_types::ResourceMetadata {
                name: name.clone(),
                project: if project == crate::crd::store::SYSTEM_PROJECT {
                    None
                } else {
                    Some(project.clone())
                },
                labels: None,
                annotations: None,
            });

        store.put(crate::crd::types::CustomResource {
            kind,
            api_version,
            metadata,
            spec,
            generation: generation as u64,
            created_at,
            updated_at,
        });
    }

    Ok((store, crds))
}

fn query_max_resource_version(db_path: &Path) -> Result<i64> {
    let conn = crate::db::open_conn(db_path)?;
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

fn load_config_from_resources_table(
    db_path: &Path,
) -> Result<Option<(OrchestratorConfig, i64, String)>> {
    let (store, crds) = load_all_resources(db_path)?;
    if store.is_empty() {
        return Ok(None);
    }
    let mut config = OrchestratorConfig {
        resource_store: store,
        custom_resource_definitions: crds,
        ..Default::default()
    };
    crate::crd::writeback::reconcile_all_builtins(&mut config);
    for kind in [
        "Agent",
        "Workflow",
        "Workspace",
        "StepTemplate",
        "ExecutionProfile",
        "EnvStore",
        "SecretStore",
    ] {
        let resources: Vec<(Option<String>, String)> = config
            .resource_store
            .list_by_kind(kind)
            .iter()
            .map(|cr| (cr.metadata.project.clone(), cr.metadata.name.clone()))
            .collect();
        for (project, name) in resources {
            crate::crd::writeback::reconcile_single_resource(
                &mut config,
                kind,
                project.as_deref(),
                &name,
            );
        }
    }
    // Populate custom_resources from resource_store for non-builtin CRD kinds
    for crd_kind in config.custom_resource_definitions.keys() {
        if crate::crd::resolve::is_builtin_kind(crd_kind) {
            continue;
        }
        for cr in config.resource_store.list_by_kind(crd_kind) {
            let storage_key = format!("{}/{}", cr.kind, cr.metadata.name);
            config.custom_resources.insert(storage_key, cr.clone());
        }
    }
    Ok(Some((
        crate::config_load::normalize_config(config),
        query_max_resource_version(db_path)?,
        now_ts(),
    )))
}

impl ConfigRepository for SqliteConfigRepository {
    fn load_or_seed_config(&self) -> Result<(OrchestratorConfig, String, i64, String)> {
        if let Some((config, version, updated_at)) = self.load_config()? {
            let (yaml, _json_raw) = serialize_config_snapshot(&config)?;
            return Ok((config, yaml, version, updated_at));
        }

        let config = OrchestratorConfig::default();
        let (yaml, _json_raw) = serialize_config_snapshot(&config)?;
        Ok((config, yaml, 0, now_ts()))
    }

    fn load_config(&self) -> Result<Option<(OrchestratorConfig, i64, String)>> {
        load_config_from_resources_table(&self.db_path)
    }

    fn query_latest_heal_summary(
        &self,
        current_config_version: i64,
    ) -> Result<Option<(i64, String, usize, String)>> {
        let conn = self.open_conn()?;
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

    fn query_heal_log_entries(&self, limit: usize) -> Result<Vec<HealLogEntry>> {
        let conn = self.open_conn()?;
        let mut stmt = conn.prepare(
            "SELECT version, original_error, workflow_id, step_id, rule, detail, created_at
             FROM config_heal_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(HealLogEntry {
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

    fn persist_self_heal_snapshot(
        &self,
        yaml: &str,
        json_raw: &str,
        original_error: &str,
        changes: &[ConfigSelfHealChange],
    ) -> Result<(i64, String)> {
        let conn = self.open_conn()?;
        let tx = conn.unchecked_transaction()?;
        let (version, created_at) = persist_config_versioned(&tx, yaml, json_raw, "self-heal")?;
        persist_heal_log(&tx, version, original_error, changes)?;
        tx.commit()?;
        Ok((version, created_at))
    }

    fn persist_raw_config(
        &self,
        normalized: OrchestratorConfig,
        yaml: &str,
        json_raw: &str,
        author: &str,
    ) -> Result<ConfigOverview> {
        let app_root = resolve_app_root_from_db_path(&self.db_path)?;
        let secret_encryption =
            SecretEncryption::from_key(ensure_secret_key(&app_root, &self.db_path)?);
        let conn = self.open_conn()?;
        let tx = conn.unchecked_transaction()?;
        let (version, updated_at) = persist_config_versioned(&tx, yaml, json_raw, author)?;
        persist_all_resources(
            &tx,
            &normalized.resource_store,
            &normalized.custom_resource_definitions,
            author,
            &secret_encryption,
        )?;
        tx.commit()?;
        Ok(ConfigOverview {
            config: normalized,
            yaml: yaml.to_owned(),
            version,
            updated_at,
        })
    }

    fn persist_config_with_deletions(
        &self,
        normalized: OrchestratorConfig,
        yaml: &str,
        json_raw: &str,
        author: &str,
        deleted_resources: &[ResourceRemoval],
    ) -> Result<ConfigOverview> {
        let app_root = resolve_app_root_from_db_path(&self.db_path)?;
        let has_secret_stores = !normalized
            .resource_store
            .list_by_kind("SecretStore")
            .is_empty();
        let secret_encryption = match crate::secret_key_lifecycle::load_keyring(
            &app_root,
            &self.db_path,
        ) {
            Ok(keyring) => {
                if keyring.has_active_key() {
                    SecretEncryption::from_keyring(&keyring)?
                } else if has_secret_stores {
                    anyhow::bail!(
                            "SecretStore write blocked: no active encryption key (all keys revoked or retired)"
                        );
                } else {
                    SecretEncryption::from_key(ensure_secret_key(&app_root, &self.db_path)?)
                }
            }
            Err(_) => SecretEncryption::from_key(ensure_secret_key(&app_root, &self.db_path)?),
        };
        let conn = self.open_conn()?;
        let tx = conn.unchecked_transaction()?;
        crate::config_load::enforce_deletion_guards_for_removals(&tx, deleted_resources)?;
        for deletion in deleted_resources {
            let _ = delete_resource_row(
                &tx,
                &deletion.kind,
                &deletion.project_id,
                &deletion.name,
                author,
            )?;
        }
        let (version, updated_at) = persist_config_versioned(&tx, yaml, json_raw, author)?;
        persist_all_resources(
            &tx,
            &normalized.resource_store,
            &normalized.custom_resource_definitions,
            author,
            &secret_encryption,
        )?;
        tx.commit()?;
        Ok(ConfigOverview {
            config: normalized,
            yaml: yaml.to_owned(),
            version,
            updated_at,
        })
    }
}
