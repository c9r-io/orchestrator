use crate::config::OrchestratorConfig;
use crate::config_load::{
    ConfigSelfHealChange, ResourceRemoval, now_ts, serialize_config_snapshot,
};
use crate::dto::ConfigOverview;
use crate::secret_store_crypto::{
    SecretEncryption, decrypt_resource_spec_json, encrypt_resource_spec_json, ensure_secret_key,
    load_existing_secret_key, resolve_data_dir_from_db_path,
};
use anyhow::{Context, Result};
use orchestrator_persistence::config_store::{ConfigTx, HealLogRow, ResourceRow};
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

    fn store(&self) -> orchestrator_persistence::config_store::ConfigStore {
        orchestrator_persistence::config_store::ConfigStore::new(&self.db_path)
    }
}

// Takes the database path rather than the caller's connection. The caller is
// mid-way through stepping a `SELECT` over `resources` when this fires, and
// under WAL — `PRAGMA journal_mode = WAL`, set by the migration chain — a second
// connection may write while that read is in progress. Best-effort either way:
// during bootstrap the audit table may not exist yet, and losing the row is
// preferable to failing the load it is reporting on.
fn emit_decrypt_failed_audit(db_path: &Path, project: &str, name: &str, error: &anyhow::Error) {
    let Ok(session) = crate::secret_store_session::SecretStoreSession::open(db_path) else {
        return;
    };
    session.record_audit_event_best_effort(&crate::secret_store_session::decrypt_failed_event(
        project,
        name,
        "system:load_resources",
        error,
    ));
}

pub(crate) fn persist_config_versioned(
    tx: &ConfigTx<'_>,
    yaml: &str,
    json_raw: &str,
    author: &str,
) -> Result<(i64, String)> {
    tx.insert_config_version(yaml, json_raw, author)
}

pub(crate) fn persist_heal_log(
    tx: &ConfigTx<'_>,
    version: i64,
    original_error: &str,
    changes: &[ConfigSelfHealChange],
) -> Result<()> {
    let rows: Vec<HealLogRow> = changes
        .iter()
        .map(|change| HealLogRow {
            workflow_id: change.workflow_id.clone(),
            step_id: change.step_id.clone(),
            rule: change.rule.as_label().to_string(),
            detail: change.detail.clone(),
        })
        .collect();
    tx.insert_heal_log(version, original_error, &rows)
}

fn persist_resource(
    tx: &ConfigTx<'_>,
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

    tx.upsert_resource(
        &ResourceRow {
            kind: cr.kind.clone(),
            project: project.to_string(),
            name: cr.metadata.name.clone(),
            api_version: cr.api_version.clone(),
            spec_json,
            metadata_json,
            generation: cr.generation,
            created_at: cr.created_at.clone(),
        },
        author,
    )
}

fn persist_all_resources(
    tx: &ConfigTx<'_>,
    store: &crate::crd::store::ResourceStore,
    crds: &HashMap<String, crate::crd::types::CustomResourceDefinition>,
    author: &str,
    secret_encryption: &SecretEncryption,
) -> Result<()> {
    for cr in store.resources().values() {
        persist_resource(tx, cr, author, secret_encryption)?;
    }
    for (kind_name, crd) in crds {
        let spec_json = serde_json::to_string(crd)?;
        tx.upsert_crd(crate::crd::store::SYSTEM_PROJECT, kind_name, &spec_json)?;
    }
    Ok(())
}

fn delete_resource_row(
    tx: &ConfigTx<'_>,
    kind: &str,
    project: &str,
    name: &str,
    author: &str,
) -> Result<bool> {
    tx.delete_resource(kind, project, name, author)
}

fn load_all_resources(
    db_path: &Path,
) -> Result<(
    crate::crd::store::ResourceStore,
    HashMap<String, crate::crd::types::CustomResourceDefinition>,
)> {
    let data_dir = resolve_data_dir_from_db_path(db_path)?;
    // Try loading via KeyRing for multi-key support; fall back to single-key
    let secret_encryption = match crate::secret_key_lifecycle::load_keyring(&data_dir, db_path) {
        Ok(keyring) => {
            if keyring.has_active_key() {
                SecretEncryption::from_keyring(&keyring).ok()
            } else {
                load_existing_secret_key(&data_dir)?.map(SecretEncryption::from_key)
            }
        }
        Err(_) => load_existing_secret_key(&data_dir)?.map(SecretEncryption::from_key),
    };
    let Some(rows) =
        orchestrator_persistence::config_store::ConfigStore::new(db_path).all_resource_rows()?
    else {
        return Ok((crate::crd::store::ResourceStore::default(), HashMap::new()));
    };

    let mut store = crate::crd::store::ResourceStore::default();
    let mut crds = HashMap::new();
    for row in rows {
        let orchestrator_persistence::config_store::StoredResourceRow {
            kind,
            project,
            name,
            api_version,
            spec_json,
            metadata_json,
            generation,
            created_at,
            updated_at,
        } = row;
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
                    emit_decrypt_failed_audit(db_path, &project, &name, &e);
                    let inner = e.to_string();
                    if inner.contains("secret key is unavailable")
                        || inner.contains("no decryption key")
                    {
                        return Err(e).with_context(|| format!(
                            "SecretStore write blocked: cannot load {project}/{name} — no active encryption key (run `orchestrator secret key list` to check key state)"
                        ));
                    }
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
    orchestrator_persistence::config_store::ConfigStore::new(db_path).max_resource_version()
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
        "SourceTaskTemplate",
        "SourceTaskBinding",
        "ExecutionProfile",
        "EnvStore",
        "SecretStore",
        "Trigger",
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
        self.store().latest_heal_summary(current_config_version)
    }

    fn query_heal_log_entries(&self, limit: usize) -> Result<Vec<HealLogEntry>> {
        Ok(self
            .store()
            .heal_log_entries(limit)?
            .into_iter()
            .map(|row| HealLogEntry {
                version: row.version,
                original_error: row.original_error,
                workflow_id: row.workflow_id,
                step_id: row.step_id,
                rule: row.rule,
                detail: row.detail,
                created_at: row.created_at,
            })
            .collect())
    }

    fn persist_self_heal_snapshot(
        &self,
        yaml: &str,
        json_raw: &str,
        original_error: &str,
        changes: &[ConfigSelfHealChange],
    ) -> Result<(i64, String)> {
        self.store().write(|tx| {
            let (version, created_at) = persist_config_versioned(tx, yaml, json_raw, "self-heal")?;
            persist_heal_log(tx, version, original_error, changes)?;
            Ok((version, created_at))
        })
    }

    fn persist_raw_config(
        &self,
        normalized: OrchestratorConfig,
        yaml: &str,
        json_raw: &str,
        author: &str,
    ) -> Result<ConfigOverview> {
        let data_dir = resolve_data_dir_from_db_path(&self.db_path)?;
        let secret_encryption =
            SecretEncryption::from_key(ensure_secret_key(&data_dir, &self.db_path)?);
        let (version, updated_at) = self.store().write(|tx| {
            let versioned = persist_config_versioned(tx, yaml, json_raw, author)?;
            persist_all_resources(
                tx,
                &normalized.resource_store,
                &normalized.custom_resource_definitions,
                author,
                &secret_encryption,
            )?;
            Ok(versioned)
        })?;
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
        let data_dir = resolve_data_dir_from_db_path(&self.db_path)?;
        let has_secret_stores = !normalized
            .resource_store
            .list_by_kind("SecretStore")
            .is_empty();
        let secret_encryption = match crate::secret_key_lifecycle::load_keyring(
            &data_dir,
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
                    SecretEncryption::from_key(ensure_secret_key(&data_dir, &self.db_path)?)
                }
            }
            Err(_) => SecretEncryption::from_key(ensure_secret_key(&data_dir, &self.db_path)?),
        };
        let (version, updated_at) = self.store().write(|tx| {
            // The guard runs inside this transaction, which is the property that
            // matters — a count taken outside it could be stale by the time the
            // delete lands. `deletion_guards()` is how the transaction hands out
            // that port without handing out the connection it is implemented for.
            crate::config_load::enforce_deletion_guards_for_removals(
                tx.deletion_guards(),
                deleted_resources,
            )?;
            for deletion in deleted_resources {
                let _ = delete_resource_row(
                    tx,
                    &deletion.kind,
                    &deletion.project_id,
                    &deletion.name,
                    author,
                )?;
            }
            let versioned = persist_config_versioned(tx, yaml, json_raw, author)?;
            persist_all_resources(
                tx,
                &normalized.resource_store,
                &normalized.custom_resource_definitions,
                author,
                &secret_encryption,
            )?;
            Ok(versioned)
        })?;
        Ok(ConfigOverview {
            config: normalized,
            yaml: yaml.to_owned(),
            version,
            updated_at,
        })
    }
}
