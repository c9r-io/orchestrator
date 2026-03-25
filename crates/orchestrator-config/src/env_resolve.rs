use crate::cli_types::AgentEnvEntry;
use crate::config::EnvStoreConfig;
use anyhow::{Result, anyhow};
use std::collections::HashMap;

/// Resolves an agent's env entries against the available env stores.
/// Returns a flat HashMap of env var name→value ready for injection.
/// Later entries override earlier ones.
pub fn resolve_agent_env(
    env_entries: &[AgentEnvEntry],
    env_stores: &HashMap<String, EnvStoreConfig>,
) -> Result<HashMap<String, String>> {
    let mut resolved = HashMap::new();

    for entry in env_entries {
        match (
            entry.name.as_deref(),
            entry.value.as_deref(),
            entry.from_ref.as_deref(),
            entry.ref_value.as_ref(),
        ) {
            // Form 1: direct value — name + value
            (Some(name), Some(value), None, None) => {
                resolved.insert(name.to_string(), value.to_string());
            }
            // Form 2: import all keys from a store — fromRef
            (None, None, Some(store_name), None) => {
                let store = env_stores.get(store_name).ok_or_else(|| {
                    anyhow!(
                        "agent env fromRef '{}' references unknown store",
                        store_name
                    )
                })?;
                for (k, v) in &store.data {
                    resolved.insert(k.clone(), v.clone());
                }
            }
            // Form 3: single key from a store — name + refValue
            (Some(name), None, None, Some(ref_value)) => {
                let store = env_stores.get(&ref_value.name).ok_or_else(|| {
                    anyhow!(
                        "agent env refValue.name '{}' references unknown store",
                        ref_value.name
                    )
                })?;
                let value = store.data.get(&ref_value.key).ok_or_else(|| {
                    anyhow!(
                        "agent env refValue key '{}' not found in store '{}'",
                        ref_value.key,
                        ref_value.name
                    )
                })?;
                resolved.insert(name.to_string(), value.clone());
            }
            _ => {
                return Err(anyhow!(
                    "invalid agent env entry: must have exactly one of (name+value), (fromRef), or (name+refValue)"
                ));
            }
        }
    }

    Ok(resolved)
}

/// Collect all sensitive values from all SecretStore configs.
/// Use this when per-agent context is unavailable (e.g., log streaming).
pub fn collect_all_sensitive_store_values(
    env_stores: &HashMap<String, EnvStoreConfig>,
) -> Vec<String> {
    env_stores
        .values()
        .filter(|s| s.sensitive)
        .flat_map(|s| s.data.values().cloned())
        .collect()
}

/// Collect env var values from sensitive stores for redaction.
pub fn collect_sensitive_values(
    env_entries: &[AgentEnvEntry],
    env_stores: &HashMap<String, EnvStoreConfig>,
) -> Vec<String> {
    let mut sensitive = Vec::new();
    for entry in env_entries {
        if let Some(ref store_name) = entry.from_ref {
            if let Some(store) = env_stores.get(store_name.as_str()) {
                if store.sensitive {
                    sensitive.extend(store.data.values().cloned());
                }
            }
        }
        if let Some(ref rv) = entry.ref_value {
            if let Some(store) = env_stores.get(&rv.name) {
                if store.sensitive {
                    if let Some(v) = store.data.get(&rv.key) {
                        sensitive.push(v.clone());
                    }
                }
            }
        }
    }
    sensitive
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{AgentEnvEntry, AgentEnvRefValue};

    fn make_stores() -> HashMap<String, EnvStoreConfig> {
        let mut stores = HashMap::new();
        stores.insert(
            "shared-config".to_string(),
            EnvStoreConfig {
                data: [
                    (
                        "DATABASE_URL".to_string(),
                        "postgres://localhost".to_string(),
                    ),
                    ("LOG_LEVEL".to_string(), "debug".to_string()),
                ]
                .into(),
                sensitive: false,
            },
        );
        stores.insert(
            "api-keys".to_string(),
            EnvStoreConfig {
                data: [("OPENAI_API_KEY".to_string(), "sk-test123".to_string())].into(),
                sensitive: true,
            },
        );
        stores
    }

    #[test]
    fn resolve_direct_value() {
        let entries = vec![AgentEnvEntry {
            name: Some("MY_VAR".to_string()),
            value: Some("my_value".to_string()),
            from_ref: None,
            ref_value: None,
        }];
        let result = resolve_agent_env(&entries, &HashMap::new()).unwrap();
        assert_eq!(result.get("MY_VAR").unwrap(), "my_value");
    }

    #[test]
    fn resolve_from_ref() {
        let stores = make_stores();
        let entries = vec![AgentEnvEntry {
            name: None,
            value: None,
            from_ref: Some("shared-config".to_string()),
            ref_value: None,
        }];
        let result = resolve_agent_env(&entries, &stores).unwrap();
        assert_eq!(result.get("DATABASE_URL").unwrap(), "postgres://localhost");
        assert_eq!(result.get("LOG_LEVEL").unwrap(), "debug");
    }

    #[test]
    fn resolve_ref_value() {
        let stores = make_stores();
        let entries = vec![AgentEnvEntry {
            name: Some("MY_API_KEY".to_string()),
            value: None,
            from_ref: None,
            ref_value: Some(AgentEnvRefValue {
                name: "api-keys".to_string(),
                key: "OPENAI_API_KEY".to_string(),
            }),
        }];
        let result = resolve_agent_env(&entries, &stores).unwrap();
        assert_eq!(result.get("MY_API_KEY").unwrap(), "sk-test123");
    }

    #[test]
    fn resolve_missing_store_errors() {
        let entries = vec![AgentEnvEntry {
            name: None,
            value: None,
            from_ref: Some("nonexistent".to_string()),
            ref_value: None,
        }];
        let err = resolve_agent_env(&entries, &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("unknown store"));
    }

    #[test]
    fn resolve_missing_key_errors() {
        let stores = make_stores();
        let entries = vec![AgentEnvEntry {
            name: Some("X".to_string()),
            value: None,
            from_ref: None,
            ref_value: Some(AgentEnvRefValue {
                name: "api-keys".to_string(),
                key: "NO_SUCH_KEY".to_string(),
            }),
        }];
        let err = resolve_agent_env(&entries, &stores).unwrap_err();
        assert!(err.to_string().contains("not found in store"));
    }

    #[test]
    fn resolve_invalid_entry_errors() {
        let entries = vec![AgentEnvEntry {
            name: Some("X".to_string()),
            value: None,
            from_ref: None,
            ref_value: None,
        }];
        let err = resolve_agent_env(&entries, &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("invalid agent env entry"));
    }

    #[test]
    fn resolve_later_entries_override_earlier() {
        let stores = make_stores();
        let entries = vec![
            AgentEnvEntry {
                name: None,
                value: None,
                from_ref: Some("shared-config".to_string()),
                ref_value: None,
            },
            AgentEnvEntry {
                name: Some("LOG_LEVEL".to_string()),
                value: Some("info".to_string()),
                from_ref: None,
                ref_value: None,
            },
        ];
        let result = resolve_agent_env(&entries, &stores).unwrap();
        assert_eq!(result.get("LOG_LEVEL").unwrap(), "info");
    }

    #[test]
    fn collect_sensitive_values_from_secret_store() {
        let stores = make_stores();
        let entries = vec![
            AgentEnvEntry {
                name: None,
                value: None,
                from_ref: Some("api-keys".to_string()),
                ref_value: None,
            },
            AgentEnvEntry {
                name: Some("X".to_string()),
                value: None,
                from_ref: None,
                ref_value: Some(AgentEnvRefValue {
                    name: "api-keys".to_string(),
                    key: "OPENAI_API_KEY".to_string(),
                }),
            },
        ];
        let sensitive = collect_sensitive_values(&entries, &stores);
        assert!(sensitive.contains(&"sk-test123".to_string()));
    }

    #[test]
    fn collect_sensitive_values_skips_non_sensitive() {
        let stores = make_stores();
        let entries = vec![AgentEnvEntry {
            name: None,
            value: None,
            from_ref: Some("shared-config".to_string()),
            ref_value: None,
        }];
        let sensitive = collect_sensitive_values(&entries, &stores);
        assert!(sensitive.is_empty());
    }

    #[test]
    fn test_collect_all_sensitive_store_values() {
        let stores = make_stores();
        let values = collect_all_sensitive_store_values(&stores);
        assert!(values.contains(&"sk-test123".to_string()));
        // non-sensitive store values should not be included
        assert!(!values.contains(&"postgres://localhost".to_string()));
        assert!(!values.contains(&"debug".to_string()));
    }

    #[test]
    fn test_collect_all_sensitive_store_values_empty() {
        let stores = HashMap::new();
        let values = collect_all_sensitive_store_values(&stores);
        assert!(values.is_empty());
    }
}
