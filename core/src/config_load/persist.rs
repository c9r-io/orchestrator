use crate::config::OrchestratorConfig;
#[cfg(test)]
use crate::db::open_conn;
use crate::dto::ConfigOverview;
use crate::persistence::repository::{ConfigRepository, HealLogEntry, SqliteConfigRepository};
use crate::resource::export_manifest_resources;
use crate::secret_store_crypto::redact_secret_data_map;
use anyhow::{Context, Result};
#[cfg(test)]
use rusqlite::params;
use std::path::Path;

#[cfg(test)]
use super::now_ts;
use super::{
    ResourceRemoval, build_active_config, build_active_config_for_project, normalize_config,
    validate_agent_command_rules, validate_agent_env_store_refs,
};

pub(crate) fn serialize_config_snapshot(config: &OrchestratorConfig) -> Result<(String, String)> {
    let sanitized = sanitized_config_snapshot(config);
    let yaml = export_manifest_resources(&sanitized)
        .iter()
        .map(crate::resource::Resource::to_yaml)
        .collect::<Result<Vec<_>>>()?
        .join("---\n");
    let json_raw =
        serde_json::to_string(&sanitized).context("failed to serialize redacted config json")?;
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

/// Query the latest heal summary for a given config version.
/// Returns `Some((version, original_error, changes_count, created_at))` if the
/// current active config version matches the most recent self-heal version.
pub fn query_latest_heal_summary(
    db_path: &Path,
    current_config_version: i64,
) -> Result<Option<(i64, String, usize, String)>> {
    SqliteConfigRepository::new(db_path).query_latest_heal_summary(current_config_version)
}

/// Query heal log entries grouped by version, most recent first.
pub fn query_heal_log_entries(db_path: &Path, limit: usize) -> Result<Vec<HealLogEntry>> {
    SqliteConfigRepository::new(db_path).query_heal_log_entries(limit)
}

/// Loads the latest config snapshot or seeds the initial snapshot when absent.
pub fn load_or_seed_config(db_path: &Path) -> Result<(OrchestratorConfig, String, i64, String)> {
    SqliteConfigRepository::new(db_path).load_or_seed_config()
}

/// Unified config loader backed only by the per-resource `resources` table.
pub fn load_config(db_path: &Path) -> Result<Option<(OrchestratorConfig, i64, String)>> {
    SqliteConfigRepository::new(db_path).load_config()
}

/// Persists a validated config snapshot and returns its overview metadata.
pub fn persist_raw_config(
    db_path: &Path,
    config: OrchestratorConfig,
    author: &str,
) -> Result<ConfigOverview> {
    let normalized = normalize_config(config);
    validate_agent_env_store_refs(&normalized)?;
    validate_agent_command_rules(&normalized)?;
    let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;
    SqliteConfigRepository::new(db_path).persist_raw_config(normalized, &yaml, &json_raw, author)
}

/// Persists a config update, rebuilds active state, and refreshes the runtime snapshot.
pub fn persist_config_and_reload(
    state: &crate::state::InnerState,
    config: OrchestratorConfig,
    _yaml: String,
    author: &str,
    target_project: Option<&str>,
    deleted_resources: &[ResourceRemoval],
) -> Result<ConfigOverview> {
    let candidate = match target_project {
        Some(project) => build_active_config_for_project(&state.data_dir, config.clone(), project)?,
        None => build_active_config(&state.data_dir, config.clone())?,
    };
    let normalized = candidate.config.clone();
    let (yaml, json_raw) = serialize_config_snapshot(&normalized)?;
    let overview = SqliteConfigRepository::new(&state.db_path).persist_config_with_deletions(
        normalized.clone(),
        &yaml,
        &json_raw,
        author,
        deleted_resources,
    )?;

    crate::state::set_config_runtime_snapshot(
        state,
        crate::state::ConfigRuntimeSnapshot::new(candidate, None, None),
    );

    Ok(overview)
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
    let overview = SqliteConfigRepository::new(&state.db_path).persist_config_with_deletions(
        normalized.clone(),
        &yaml,
        &json_raw,
        author,
        deleted_resources,
    )?;

    // Best-effort rebuild of active config; if validation fails, still persist
    match build_active_config(&state.data_dir, normalized.clone()) {
        Ok(candidate) => {
            crate::state::set_config_runtime_snapshot(
                state,
                crate::state::ConfigRuntimeSnapshot::new(candidate, None, None),
            );
        }
        Err(_) => {
            // Config is persisted but in-memory state may be stale.
            // Next successful apply will fix it.
        }
    }

    Ok(overview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::tests::make_test_db;
    use crate::config_load::{ConfigSelfHealChange, ConfigSelfHealRule};
    use std::collections::HashMap;

    fn seed_heal_log(db_path: &Path, version: i64) {
        let repo = SqliteConfigRepository::new(db_path);
        let config = OrchestratorConfig::default();
        let (yaml, json_raw) = serialize_config_snapshot(&config).expect("serialize config");
        let conn = open_conn(db_path).expect("open test db");
        for seed_version in 1..version {
            conn.execute(
                "INSERT INTO orchestrator_config_versions (version, config_yaml, config_json, created_at, author)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![seed_version, yaml, json_raw, now_ts(), "test-seed"],
            )
            .expect("seed config version");
        }
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
        let (persisted_version, _) = repo
            .persist_self_heal_snapshot(&yaml, &json_raw, "builtin/capability conflict", &changes)
            .expect("persist heal log");
        assert_eq!(persisted_version, version, "seeded version should match");
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
    fn load_or_seed_config_returns_blank_default_when_resources_are_empty() {
        let (_temp_dir, db_path) = make_test_db();

        let existing = load_config(&db_path).expect("load blank config");
        assert!(
            existing.is_none(),
            "blank sqlite should have no persisted resources"
        );

        let (config, yaml, version, _updated_at) =
            load_or_seed_config(&db_path).expect("load blank default config");

        assert!(config.projects.is_empty());
        assert!(config.custom_resource_definitions.is_empty());
        assert!(config.custom_resources.is_empty());
        assert!(config.resource_store.is_empty());
        assert_eq!(version, 0);
        assert!(
            yaml.contains("kind: RuntimePolicy"),
            "blank default config should still export synthesized runtime policy"
        );

        let still_blank = load_config(&db_path).expect("reload blank config");
        assert!(
            still_blank.is_none(),
            "load_or_seed_config must not persist synthetic resources for a blank db"
        );
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
                            health_policy: Default::default(),
                        },
                    )]),
                    agents: HashMap::from([(
                        "shared-agent".to_string(),
                        crate::config::AgentConfig {
                            enabled: true,
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
                    execution_profiles: HashMap::new(),
                    triggers: HashMap::new(),
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

    #[test]
    fn persist_raw_config_encrypts_secret_store_resources_and_redacts_snapshots() {
        let (_temp_dir, db_path) = make_test_db();
        let mut config = crate::config_load::tests::make_config_with_default_project();
        config
            .ensure_project(Some(crate::config::DEFAULT_PROJECT_ID))
            .env_stores
            .insert(
                "api-keys".to_string(),
                crate::config::EnvStoreConfig {
                    data: [("OPENAI_API_KEY".to_string(), "sk-secret-123".to_string())].into(),
                    sensitive: true,
                },
            );
        crate::crd::writeback::reconcile_all_builtins(&mut config);

        persist_raw_config(&db_path, config, "test-seed").expect("persist config");

        let conn = open_conn(&db_path).expect("open sqlite");
        let spec_json: String = conn
            .query_row(
                "SELECT spec_json FROM resources WHERE kind = 'SecretStore' AND name = 'api-keys'",
                [],
                |row| row.get(0),
            )
            .expect("query encrypted secret store resource");
        assert!(!spec_json.contains("sk-secret-123"));
        assert!(spec_json.contains("\"_encrypted\":true"));

        let version_spec_json: String = conn
            .query_row(
                "SELECT spec_json FROM resource_versions WHERE kind = 'SecretStore' AND name = 'api-keys' ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("query encrypted secret store version");
        assert!(!version_spec_json.contains("sk-secret-123"));
        assert!(version_spec_json.contains("\"_encrypted\":true"));

        let snapshot: (String, String) = conn
            .query_row(
                "SELECT config_yaml, config_json FROM orchestrator_config_versions ORDER BY version DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query config snapshot");
        assert!(!snapshot.0.contains("sk-secret-123"));
        assert!(!snapshot.1.contains("sk-secret-123"));
        assert!(
            snapshot
                .0
                .contains(crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER)
        );
        assert!(
            snapshot
                .1
                .contains(crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER)
        );

        let loaded = load_config(&db_path)
            .expect("load config")
            .expect("config should exist")
            .0;
        let loaded_value = loaded
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .and_then(|project| project.env_stores.get("api-keys"))
            .and_then(|store| store.data.get("OPENAI_API_KEY"))
            .cloned()
            .expect("loaded decrypted secret value");
        assert_eq!(loaded_value, "sk-secret-123");
    }
}
