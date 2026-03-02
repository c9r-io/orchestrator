use crate::config::OrchestratorConfig;
use crate::db::open_conn;
use crate::dto::ConfigOverview;
use crate::resource::export_manifest_resources;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use std::path::Path;

use super::{
    build_active_config, enforce_deletion_guards, normalize_config, now_ts, read_active_config,
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

    tx.execute(
        "INSERT INTO orchestrator_config (id, config_yaml, config_json, version, updated_at) VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET config_yaml=excluded.config_yaml, config_json=excluded.config_json, version=excluded.version, updated_at=excluded.updated_at",
        params![yaml, json_raw, next_version, now],
    )?;
    tx.execute(
        "INSERT INTO orchestrator_config_versions (version, config_yaml, config_json, created_at, author) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![next_version, yaml, json_raw, now, author],
    )?;

    Ok((next_version, now))
}

pub fn load_or_seed_config(db_path: &Path) -> Result<(OrchestratorConfig, String, i64, String)> {
    let conn = open_conn(db_path)?;
    let row: Option<(String, String, i64, String)> = conn
        .query_row(
            "SELECT config_yaml, config_json, version, updated_at FROM orchestrator_config WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;

    if let Some((_yaml, json_raw, version, updated_at)) = row {
        let config = serde_json::from_str::<OrchestratorConfig>(&json_raw)
            .context("failed to parse config_json from sqlite")?;
        let config = normalize_config(config);
        let (yaml, _json_raw) = serialize_config_snapshot(&config)?;
        return Ok((config, yaml, version, updated_at));
    }

    anyhow::bail!(
        "[CONFIG_NOT_INITIALIZED] orchestrator manifest is not initialized in sqlite\n  category: validation\n  suggested_fix: run 'orchestrator apply -f <manifest.yaml>' first"
    )
}

pub fn load_raw_config_from_db(
    db_path: &Path,
) -> Result<Option<(OrchestratorConfig, i64, String)>> {
    let conn = open_conn(db_path)?;
    let row: Option<(String, String, i64, String)> = conn
        .query_row(
            "SELECT config_yaml, config_json, version, updated_at FROM orchestrator_config WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;

    let Some((_yaml, json_raw, version, updated_at)) = row else {
        return Ok(None);
    };

    let config = serde_json::from_str::<OrchestratorConfig>(&json_raw)
        .context("failed to parse config_json from sqlite")?;
    Ok(Some((normalize_config(config), version, updated_at)))
}

pub fn persist_raw_config(
    db_path: &Path,
    config: OrchestratorConfig,
    author: &str,
) -> Result<ConfigOverview> {
    let normalized = normalize_config(config);
    let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;
    let conn = open_conn(db_path)?;
    let tx = conn.unchecked_transaction()?;
    let (next_version, now) = persist_config_versioned(&tx, &yaml, &json_raw, author)?;
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
) -> Result<ConfigOverview> {
    let candidate = build_active_config(&state.app_root, config.clone())?;
    let normalized = candidate.config.clone();
    let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;

    let previous_config = {
        let active = read_active_config(state)?;
        active.config.clone()
    };

    let conn = open_conn(&state.db_path)?;
    let tx = conn.unchecked_transaction()?;
    enforce_deletion_guards(&tx, &previous_config, &normalized)?;
    let (next_version, now) = persist_config_versioned(&tx, &yaml, &json_raw, author)?;
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
