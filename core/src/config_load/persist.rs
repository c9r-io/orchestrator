use crate::config::OrchestratorConfig;
use crate::config_load::ConfigSelfHealChange;
use crate::db::open_conn;
use crate::dto::ConfigOverview;
use crate::resource::export_manifest_resources;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use std::path::Path;

use super::{
    build_active_config, build_active_config_for_project, enforce_deletion_guards_for_removals,
    normalize_config, now_ts, validate_agent_env_store_refs, ResourceRemoval,
};

pub(crate) fn serialize_config_snapshot(config: &OrchestratorConfig) -> Result<(String, String)> {
    let yaml = export_manifest_resources(config)
        .iter()
        .map(crate::resource::Resource::to_yaml)
        .collect::<Result<Vec<_>>>()?
        .join("---\n");
    let json_raw = serde_json::to_string(config).context("failed to serialize config json")?;
    Ok((yaml, json_raw))
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

    // Version history is kept in orchestrator_config_versions.
    // Live resource state is persisted via persist_all_resources.
    tx.execute(
        "INSERT INTO orchestrator_config_versions (version, config_yaml, config_json, created_at, author) VALUES (?1, ?2, ?3, ?4, ?5)",
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct HealLogEntry {
    pub version: i64,
    pub original_error: String,
    pub workflow_id: String,
    pub step_id: String,
    pub rule: String,
    pub detail: String,
    pub created_at: String,
}

/// Query the latest heal summary for a given config version.
/// Returns `Some((version, original_error, changes_count, created_at))` if the
/// current active config version matches the most recent self-heal version.
pub fn query_latest_heal_summary(
    db_path: &Path,
    current_config_version: i64,
) -> Result<Option<(i64, String, usize, String)>> {
    let conn = open_conn(db_path)?;
    let row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT version, original_error, created_at FROM config_heal_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
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
        |r| r.get(0),
    )?;

    Ok(Some((version, original_error, count, created_at)))
}

/// Query heal log entries grouped by version, most recent first.
pub fn query_heal_log_entries(db_path: &Path, limit: usize) -> Result<Vec<HealLogEntry>> {
    let conn = open_conn(db_path)?;
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

pub fn load_or_seed_config(db_path: &Path) -> Result<(OrchestratorConfig, String, i64, String)> {
    if let Some((config, version, updated_at)) = load_config_from_resources_table(db_path)? {
        let (yaml, _json_raw) = serialize_config_snapshot(&config)?;
        return Ok((config, yaml, version, updated_at));
    }

    anyhow::bail!(
        "[CONFIG_NOT_INITIALIZED] orchestrator manifest is not initialized in sqlite\n  category: validation\n  suggested_fix: run 'orchestrator apply -f <manifest.yaml>' first"
    )
}

/// Unified config loader backed only by the per-resource `resources` table.
pub fn load_config(db_path: &Path) -> Result<Option<(OrchestratorConfig, i64, String)>> {
    load_config_from_resources_table(db_path)
}

/// Persist a single resource to the `resources` table with version tracking.
pub(crate) fn persist_resource(
    tx: &Transaction<'_>,
    cr: &crate::crd::types::CustomResource,
    author: &str,
) -> Result<()> {
    let project = cr
        .metadata
        .project
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or(crate::crd::store::SYSTEM_PROJECT);

    // Project-scoped resources must not be stored under _system
    if crate::crd::store::is_project_scoped(&cr.kind)
        && project == crate::crd::store::SYSTEM_PROJECT
    {
        anyhow::bail!(
            "project-scoped resource {}/{} must have an explicit project, not _system",
            cr.kind,
            cr.metadata.name
        );
    }
    let spec_json = serde_json::to_string(&cr.spec)?;
    let metadata_json = serde_json::to_string(&cr.metadata)?;
    let now = now_ts();

    tx.execute(
        "INSERT INTO resources (kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(kind, project, name) DO UPDATE SET
           api_version=excluded.api_version, spec_json=excluded.spec_json,
           metadata_json=excluded.metadata_json, generation=generation+1, updated_at=excluded.updated_at",
        params![cr.kind, project, cr.metadata.name, cr.api_version, spec_json, metadata_json, cr.generation, cr.created_at, now],
    )?;

    let next_version: i64 = tx.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM resource_versions WHERE kind=?1 AND project=?2 AND name=?3",
        params![cr.kind, project, cr.metadata.name],
        |row| row.get(0),
    )?;
    tx.execute(
        "INSERT INTO resource_versions (kind, project, name, spec_json, metadata_json, version, author, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![cr.kind, project, cr.metadata.name, spec_json, metadata_json, next_version, author, now],
    )?;

    Ok(())
}

/// Persist all resources from a store + CRDs in a single transaction.
pub(crate) fn persist_all_resources(
    tx: &Transaction<'_>,
    store: &crate::crd::store::ResourceStore,
    crds: &std::collections::HashMap<String, crate::crd::types::CustomResourceDefinition>,
    author: &str,
) -> Result<()> {
    for cr in store.resources().values() {
        persist_resource(tx, cr, author)?;
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

/// Delete a single resource from the `resources` table with version tracking.
pub fn delete_resource_row(
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

/// Load all resources from the `resources` table into a ResourceStore + CRD map.
pub fn load_all_resources(
    db_path: &Path,
) -> Result<(
    crate::crd::store::ResourceStore,
    std::collections::HashMap<String, crate::crd::types::CustomResourceDefinition>,
)> {
    let conn = open_conn(db_path)?;
    // Check if resources table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='resources'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !table_exists {
        return Ok((
            crate::crd::store::ResourceStore::default(),
            std::collections::HashMap::new(),
        ));
    }

    let mut store = crate::crd::store::ResourceStore::default();
    let mut crds = std::collections::HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT kind, project, name, api_version, spec_json, metadata_json, generation, created_at, updated_at FROM resources",
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

        let spec: serde_json::Value = serde_json::from_str(&spec_json).unwrap_or_default();
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

        let cr = crate::crd::types::CustomResource {
            kind,
            api_version,
            metadata,
            spec,
            generation: generation as u64,
            created_at,
            updated_at,
        };
        store.put(cr);
    }

    Ok((store, crds))
}

/// Query the maximum resource version (for compatibility with config version tracking).
pub fn query_max_resource_version(db_path: &Path) -> Result<i64> {
    let conn = open_conn(db_path)?;
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

/// Load config from the per-resource `resources` table (v10+).
/// Returns None if the table is empty or doesn't exist.
pub fn load_config_from_resources_table(
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
    let project_kinds = [
        "Agent",
        "Workflow",
        "Workspace",
        "StepTemplate",
        "EnvStore",
        "SecretStore",
    ];
    for kind in &project_kinds {
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
    let version = query_max_resource_version(db_path)?;
    let now = now_ts();
    Ok(Some((normalize_config(config), version, now)))
}

pub fn persist_raw_config(
    db_path: &Path,
    config: OrchestratorConfig,
    author: &str,
) -> Result<ConfigOverview> {
    let normalized = normalize_config(config);
    validate_agent_env_store_refs(&normalized)?;
    let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;
    let conn = open_conn(db_path)?;
    let tx = conn.unchecked_transaction()?;
    let (next_version, now) = persist_config_versioned(&tx, &yaml, &json_raw, author)?;
    let _ = persist_all_resources(
        &tx,
        &normalized.resource_store,
        &normalized.custom_resource_definitions,
        author,
    );
    tx.commit()?;

    Ok(ConfigOverview {
        config: normalized,
        yaml,
        version: next_version,
        updated_at: now,
    })
}

pub fn persist_config_and_reload(
    state: &crate::state::InnerState,
    config: OrchestratorConfig,
    _yaml: String,
    author: &str,
    target_project: Option<&str>,
    deleted_resources: &[ResourceRemoval],
) -> Result<ConfigOverview> {
    let candidate = match target_project {
        Some(project) => build_active_config_for_project(&state.app_root, config.clone(), project)?,
        None => build_active_config(&state.app_root, config.clone())?,
    };
    let normalized = candidate.config.clone();
    let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;

    let conn = open_conn(&state.db_path)?;
    let tx = conn.unchecked_transaction()?;
    enforce_deletion_guards_for_removals(&tx, deleted_resources)?;
    for deletion in deleted_resources {
        let _ = delete_resource_row(
            &tx,
            &deletion.kind,
            &deletion.project_id,
            &deletion.name,
            author,
        )?;
    }
    let (next_version, now) = persist_config_versioned(&tx, &yaml, &json_raw, author)?;
    // Also write per-resource rows (dual-write for v10+ compatibility)
    let _ = persist_all_resources(
        &tx,
        &normalized.resource_store,
        &normalized.custom_resource_definitions,
        author,
    );
    tx.commit()?;

    {
        let mut active = crate::state::write_active_config(state)?;
        *active = candidate;
    }
    if let Ok(mut error) = state.active_config_error.write() {
        *error = None;
    }
    if let Ok(mut notice) = state.active_config_notice.write() {
        *notice = None;
    }

    Ok(ConfigOverview {
        config: normalized,
        yaml,
        version: next_version,
        updated_at: now,
    })
}

/// Persist config after a delete operation. Unlike `persist_config_and_reload`,
/// `build_active_config` failure is non-fatal — the deletion is persisted even
/// if another project's validation fails. `enforce_deletion_guards` still runs
/// to protect workspace/workflow references with active tasks.
pub fn persist_config_for_delete(
    state: &crate::state::InnerState,
    config: OrchestratorConfig,
    author: &str,
    deleted_resources: &[ResourceRemoval],
) -> Result<ConfigOverview> {
    let normalized = normalize_config(config);
    let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;

    let conn = open_conn(&state.db_path)?;
    let tx = conn.unchecked_transaction()?;
    enforce_deletion_guards_for_removals(&tx, deleted_resources)?;
    for deletion in deleted_resources {
        let _ = delete_resource_row(
            &tx,
            &deletion.kind,
            &deletion.project_id,
            &deletion.name,
            author,
        )?;
    }
    let (next_version, now) = persist_config_versioned(&tx, &yaml, &json_raw, author)?;
    let _ = persist_all_resources(
        &tx,
        &normalized.resource_store,
        &normalized.custom_resource_definitions,
        author,
    );
    tx.commit()?;

    // Best-effort rebuild of active config; if validation fails, still persist
    match build_active_config(&state.app_root, normalized.clone()) {
        Ok(candidate) => {
            let mut active = crate::state::write_active_config(state)?;
            *active = candidate;
            if let Ok(mut err) = state.active_config_error.write() {
                *err = None;
            }
        }
        Err(_) => {
            // Config is persisted but in-memory state may be stale.
            // Next successful apply will fix it.
        }
    }

    Ok(ConfigOverview {
        config: normalized,
        yaml,
        version: next_version,
        updated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::tests::make_test_db;
    use crate::config_load::{ConfigSelfHealChange, ConfigSelfHealRule};
    use std::collections::HashMap;

    fn seed_heal_log(db_path: &Path, version: i64) {
        let conn = open_conn(db_path).expect("open test db");
        let tx = conn
            .unchecked_transaction()
            .expect("begin unchecked transaction");
        let changes = vec![
            ConfigSelfHealChange {
                workflow_id: "basic".to_string(),
                step_id: "self_test".to_string(),
                rule: ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep,
                detail:
                    "removed deprecated required_capability 'self_test' from builtin 'self_test'"
                        .to_string(),
            },
            ConfigSelfHealChange {
                workflow_id: "basic".to_string(),
                step_id: "init".to_string(),
                rule: ConfigSelfHealRule::NormalizeStepExecutionMode,
                detail: "normalized behavior.execution from Agent to Builtin".to_string(),
            },
        ];
        persist_heal_log(&tx, version, "builtin/capability conflict", &changes)
            .expect("persist heal log");
        tx.commit().expect("commit heal log transaction");
    }

    #[test]
    fn persist_heal_log_roundtrip() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 3);

        let entries = query_heal_log_entries(&db_path, 10).expect("query heal log entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, 3);
        // DESC order: most recent entry first
        assert_eq!(entries[0].rule, "NormalizeStepExecutionMode");
        assert_eq!(entries[1].rule, "DropRequiredCapabilityFromBuiltinStep");
    }

    #[test]
    fn query_heal_log_entries_respects_limit() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 3);

        let entries = query_heal_log_entries(&db_path, 1).expect("query limited heal log");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn query_heal_log_entries_returns_empty_when_no_records() {
        let (_temp_dir, db_path) = make_test_db();
        let entries = query_heal_log_entries(&db_path, 10).expect("query empty heal log");
        assert!(entries.is_empty());
    }

    #[test]
    fn query_latest_heal_summary_returns_none_when_empty() {
        let (_temp_dir, db_path) = make_test_db();
        let result = query_latest_heal_summary(&db_path, 1).expect("query empty heal summary");
        assert!(result.is_none());
    }

    #[test]
    fn query_latest_heal_summary_returns_summary_for_matching_version() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 5);

        let result = query_latest_heal_summary(&db_path, 5).expect("query matching heal summary");
        assert!(result.is_some());
        let (version, original_error, count, _created_at) =
            result.expect("matching heal summary should exist");
        assert_eq!(version, 5);
        assert_eq!(original_error, "builtin/capability conflict");
        assert_eq!(count, 2);
    }

    #[test]
    fn query_latest_heal_summary_returns_none_for_non_matching_version() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 5);

        let result =
            query_latest_heal_summary(&db_path, 6).expect("query non-matching heal summary");
        assert!(
            result.is_none(),
            "should not match when config version is newer"
        );
    }

    #[test]
    fn persist_heal_log_stores_original_error_per_entry() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 2);

        let entries = query_heal_log_entries(&db_path, 10).expect("query heal log entries");
        for entry in &entries {
            assert_eq!(entry.original_error, "builtin/capability conflict");
        }
    }

    #[test]
    fn load_config_from_resources_table_preserves_same_named_project_resources() {
        let (_temp_dir, db_path) = make_test_db();
        let mut config = crate::config_load::tests::make_config_with_default_project();

        for project_id in ["alpha", "beta"] {
            config.projects.insert(
                project_id.to_string(),
                crate::config::ProjectConfig {
                    description: Some(format!("{project_id} project")),
                    workspaces: HashMap::from([(
                        "shared-ws".to_string(),
                        crate::config::WorkspaceConfig {
                            root_path: ".".to_string(),
                            qa_targets: vec!["docs/qa".to_string()],
                            ticket_dir: "docs/ticket".to_string(),
                            self_referential: false,
                        },
                    )]),
                    agents: HashMap::from([(
                        "shared-agent".to_string(),
                        crate::config::AgentConfig {
                            capabilities: vec!["implement".to_string()],
                            command: "echo hi".to_string(),
                            ..Default::default()
                        },
                    )]),
                    workflows: HashMap::from([(
                        "shared-wf".to_string(),
                        crate::config_load::tests::make_workflow(vec![
                            crate::config_load::tests::make_command_step(
                                "implement",
                                "echo shared",
                            ),
                        ]),
                    )]),
                    step_templates: HashMap::new(),
                    env_stores: HashMap::new(),
                },
            );
        }

        persist_raw_config(&db_path, config, "test-seed").expect("persist config");
        let loaded = load_config(&db_path)
            .expect("load config")
            .expect("config should exist")
            .0;

        let alpha = loaded.projects.get("alpha").expect("alpha project");
        let beta = loaded.projects.get("beta").expect("beta project");
        assert!(alpha.workspaces.contains_key("shared-ws"));
        assert!(beta.workspaces.contains_key("shared-ws"));
        assert!(alpha.agents.contains_key("shared-agent"));
        assert!(beta.agents.contains_key("shared-agent"));
        assert!(alpha.workflows.contains_key("shared-wf"));
        assert!(beta.workflows.contains_key("shared-wf"));
    }
}
