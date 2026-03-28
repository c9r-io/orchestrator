use crate::cli_types::AgentEnvEntry;
use crate::config::{EnvStoreConfig, SecretStoreConfig};
use anyhow::{Result, anyhow};
use std::collections::HashMap;

/// Look up store data by name, checking env_stores first then secret_stores.
fn lookup_store_data<'a>(
    name: &str,
    env_stores: &'a HashMap<String, EnvStoreConfig>,
    secret_stores: &'a HashMap<String, SecretStoreConfig>,
) -> Option<&'a HashMap<String, String>> {
    env_stores
        .get(name)
        .map(|s| &s.data)
        .or_else(|| secret_stores.get(name).map(|s| &s.data))
}

/// Resolves an agent's env entries against the available env and secret stores.
/// Returns a flat HashMap of env var name→value ready for injection.
/// Later entries override earlier ones.
pub fn resolve_agent_env(
    env_entries: &[AgentEnvEntry],
    env_stores: &HashMap<String, EnvStoreConfig>,
    secret_stores: &HashMap<String, SecretStoreConfig>,
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
                let data = lookup_store_data(store_name, env_stores, secret_stores)
                    .ok_or_else(|| {
                        anyhow!(
                            "agent env fromRef '{}' references unknown store",
                            store_name
                        )
                    })?;
                for (k, v) in data {
                    resolved.insert(k.clone(), v.clone());
                }
            }
            // Form 3: single key from a store — name + refValue
            (Some(name), None, None, Some(ref_value)) => {
                let data =
                    lookup_store_data(&ref_value.name, env_stores, secret_stores).ok_or_else(
                        || {
                            anyhow!(
                                "agent env refValue.name '{}' references unknown store",
                                ref_value.name
                            )
                        },
                    )?;
                let value = data.get(&ref_value.key).ok_or_else(|| {
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
    secret_stores: &HashMap<String, SecretStoreConfig>,
) -> Vec<String> {
    secret_stores
        .values()
        .flat_map(|s| s.data.values().cloned())
        .collect()
}

/// Collect env var values from sensitive stores for redaction.
pub fn collect_sensitive_values(
    env_entries: &[AgentEnvEntry],
    secret_stores: &HashMap<String, SecretStoreConfig>,
) -> Vec<String> {
    let mut sensitive = Vec::new();
    for entry in env_entries {
        if let Some(ref store_name) = entry.from_ref {
            if let Some(store) = secret_stores.get(store_name.as_str()) {
                sensitive.extend(store.data.values().cloned());
            }
        }
        if let Some(ref rv) = entry.ref_value {
            if let Some(store) = secret_stores.get(&rv.name) {
                if let Some(v) = store.data.get(&rv.key) {
                    sensitive.push(v.clone());
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

    fn make_env_stores() -> HashMap<String, EnvStoreConfig> {
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
            },
        );
        stores
    }

    fn make_secret_stores() -> HashMap<String, SecretStoreConfig> {
        let mut stores = HashMap::new();
        stores.insert(
            "api-keys".to_string(),
            SecretStoreConfig {
                data: [("OPENAI_API_KEY".to_string(), "sk-test123".to_string())].into(),
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
        let result = resolve_agent_env(&entries, &HashMap::new(), &HashMap::new()).unwrap();
        assert_eq!(result.get("MY_VAR").unwrap(), "my_value");
    }

    #[test]
    fn resolve_from_ref() {
        let env_stores = make_env_stores();
        let entries = vec![AgentEnvEntry {
            name: None,
            value: None,
            from_ref: Some("shared-config".to_string()),
            ref_value: None,
        }];
        let result = resolve_agent_env(&entries, &env_stores, &HashMap::new()).unwrap();
        assert_eq!(result.get("DATABASE_URL").unwrap(), "postgres://localhost");
        assert_eq!(result.get("LOG_LEVEL").unwrap(), "debug");
    }

    #[test]
    fn resolve_ref_value() {
        let secret_stores = make_secret_stores();
        let entries = vec![AgentEnvEntry {
            name: Some("MY_API_KEY".to_string()),
            value: None,
            from_ref: None,
            ref_value: Some(AgentEnvRefValue {
                name: "api-keys".to_string(),
                key: "OPENAI_API_KEY".to_string(),
            }),
        }];
        let result = resolve_agent_env(&entries, &HashMap::new(), &secret_stores).unwrap();
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
        let err = resolve_agent_env(&entries, &HashMap::new(), &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("unknown store"));
    }

    #[test]
    fn resolve_missing_key_errors() {
        let secret_stores = make_secret_stores();
        let entries = vec![AgentEnvEntry {
            name: Some("X".to_string()),
            value: None,
            from_ref: None,
            ref_value: Some(AgentEnvRefValue {
                name: "api-keys".to_string(),
                key: "NO_SUCH_KEY".to_string(),
            }),
        }];
        let err = resolve_agent_env(&entries, &HashMap::new(), &secret_stores).unwrap_err();
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
        let err = resolve_agent_env(&entries, &HashMap::new(), &HashMap::new()).unwrap_err();
        assert!(err.to_string().contains("invalid agent env entry"));
    }

    #[test]
    fn resolve_later_entries_override_earlier() {
        let env_stores = make_env_stores();
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
        let result = resolve_agent_env(&entries, &env_stores, &HashMap::new()).unwrap();
        assert_eq!(result.get("LOG_LEVEL").unwrap(), "info");
    }

    #[test]
    fn collect_sensitive_values_from_secret_store() {
        let secret_stores = make_secret_stores();
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
        let sensitive = collect_sensitive_values(&entries, &secret_stores);
        assert!(sensitive.contains(&"sk-test123".to_string()));
    }

    #[test]
    fn collect_sensitive_values_skips_env_stores() {
        let secret_stores = make_secret_stores();
        let entries = vec![AgentEnvEntry {
            name: None,
            value: None,
            from_ref: Some("shared-config".to_string()),
            ref_value: None,
        }];
        let sensitive = collect_sensitive_values(&entries, &secret_stores);
        assert!(sensitive.is_empty());
    }

    #[test]
    fn test_collect_all_sensitive_store_values() {
        let secret_stores = make_secret_stores();
        let values = collect_all_sensitive_store_values(&secret_stores);
        assert!(values.contains(&"sk-test123".to_string()));
    }

    #[test]
    fn test_collect_all_sensitive_store_values_empty() {
        let stores = HashMap::new();
        let values = collect_all_sensitive_store_values(&stores);
        assert!(values.is_empty());
    }
}
