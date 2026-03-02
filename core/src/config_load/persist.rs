use crate::config::OrchestratorConfig;
use crate::config_load::ConfigSelfHealChange;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::tests::make_test_db;
    use crate::config_load::{ConfigSelfHealChange, ConfigSelfHealRule};

    fn seed_heal_log(db_path: &Path, version: i64) {
        let conn = open_conn(db_path).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        let changes = vec![
            ConfigSelfHealChange {
                workflow_id: "basic".to_string(),
                step_id: "self_test".to_string(),
                rule: ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep,
                detail: "removed legacy required_capability 'self_test' from builtin 'self_test'"
                    .to_string(),
            },
            ConfigSelfHealChange {
                workflow_id: "basic".to_string(),
                step_id: "init".to_string(),
                rule: ConfigSelfHealRule::NormalizeStepExecutionMode,
                detail: "normalized behavior.execution from Agent to Builtin".to_string(),
            },
        ];
        persist_heal_log(&tx, version, "builtin/capability conflict", &changes).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn persist_heal_log_roundtrip() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 3);

        let entries = query_heal_log_entries(&db_path, 10).unwrap();
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

        let entries = query_heal_log_entries(&db_path, 1).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn query_heal_log_entries_returns_empty_when_no_records() {
        let (_temp_dir, db_path) = make_test_db();
        let entries = query_heal_log_entries(&db_path, 10).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn query_latest_heal_summary_returns_none_when_empty() {
        let (_temp_dir, db_path) = make_test_db();
        let result = query_latest_heal_summary(&db_path, 1).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn query_latest_heal_summary_returns_summary_for_matching_version() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 5);

        let result = query_latest_heal_summary(&db_path, 5).unwrap();
        assert!(result.is_some());
        let (version, original_error, count, _created_at) = result.unwrap();
        assert_eq!(version, 5);
        assert_eq!(original_error, "builtin/capability conflict");
        assert_eq!(count, 2);
    }

    #[test]
    fn query_latest_heal_summary_returns_none_for_non_matching_version() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 5);

        let result = query_latest_heal_summary(&db_path, 6).unwrap();
        assert!(
            result.is_none(),
            "should not match when config version is newer"
        );
    }

    #[test]
    fn persist_heal_log_stores_original_error_per_entry() {
        let (_temp_dir, db_path) = make_test_db();
        seed_heal_log(&db_path, 2);

        let entries = query_heal_log_entries(&db_path, 10).unwrap();
        for entry in &entries {
            assert_eq!(entry.original_error, "builtin/capability conflict");
        }
    }
}
