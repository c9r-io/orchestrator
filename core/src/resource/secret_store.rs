use crate::cli_types::{EnvStoreSpec, OrchestratorResource, ResourceKind, ResourceSpec};
use crate::config::{EnvStoreConfig, OrchestratorConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
pub struct SecretStoreResource {
    pub metadata: ResourceMetadata,
    pub spec: EnvStoreSpec,
}

impl Resource for SecretStoreResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::SecretStore
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = EnvStoreConfig {
            data: self.spec.data.clone(),
            sensitive: true,
        };
        super::apply_to_map(&mut config.env_stores, self.name(), incoming)
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::SecretStore,
            &self.metadata,
            ResourceSpec::EnvStore(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.env_stores.get(name).and_then(|store| {
            if store.sensitive {
                Some(Self {
                    metadata: super::metadata_with_name(name),
                    spec: EnvStoreSpec {
                        data: store.data.clone(),
                    },
                })
            } else {
                None // EnvStore, not SecretStore
            }
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        config
            .env_stores
            .get(name)
            .is_some_and(|s| s.sensitive)
            .then(|| config.env_stores.remove(name))
            .is_some()
    }
}

pub(super) fn build_secret_store(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::SecretStore {
        return Err(anyhow!("resource kind/spec mismatch for SecretStore"));
    }
    match spec {
        ResourceSpec::EnvStore(spec) => Ok(RegisteredResource::SecretStore(SecretStoreResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for SecretStore")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::test_fixtures::make_config;

    fn make_secret_store(name: &str) -> SecretStoreResource {
        SecretStoreResource {
            metadata: super::super::metadata_with_name(name),
            spec: EnvStoreSpec {
                data: [("API_KEY".to_string(), "sk-secret".to_string())].into(),
            },
        }
    }

    #[test]
    fn secret_store_apply_and_get() {
        let mut config = make_config();
        let store = make_secret_store("my-secrets");
        assert_eq!(store.apply(&mut config), ApplyResult::Created);

        let loaded = SecretStoreResource::get_from(&config, "my-secrets")
            .expect("secret store should be present");
        assert_eq!(loaded.spec.data.get("API_KEY").unwrap(), "sk-secret");
        assert_eq!(loaded.kind(), ResourceKind::SecretStore);

        // Underlying config should be marked sensitive
        assert!(config.env_stores.get("my-secrets").unwrap().sensitive);
    }

    #[test]
    fn secret_store_apply_unchanged() {
        let mut config = make_config();
        let store = make_secret_store("ss-unchanged");
        assert_eq!(store.apply(&mut config), ApplyResult::Created);
        assert_eq!(store.apply(&mut config), ApplyResult::Unchanged);
    }

    #[test]
    fn secret_store_delete() {
        let mut config = make_config();
        let store = make_secret_store("ss-del");
        store.apply(&mut config);
        assert!(SecretStoreResource::delete_from(&mut config, "ss-del"));
        assert!(SecretStoreResource::get_from(&config, "ss-del").is_none());
    }

    #[test]
    fn secret_store_validate_rejects_empty_name() {
        let store = make_secret_store("");
        assert!(store.validate().is_err());
    }

    #[test]
    fn secret_store_to_yaml() {
        let store = make_secret_store("yaml-ss");
        let yaml = store.to_yaml().expect("should serialize");
        assert!(yaml.contains("SecretStore"));
        assert!(yaml.contains("yaml-ss"));
    }

    #[test]
    fn secret_store_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(SecretStoreResource::get_from(&config, "no-such").is_none());
    }

    #[test]
    fn secret_store_get_from_skips_non_sensitive() {
        let mut config = make_config();
        config.env_stores.insert(
            "plain-env".to_string(),
            EnvStoreConfig {
                data: [("K".to_string(), "v".to_string())].into(),
                sensitive: false,
            },
        );
        assert!(SecretStoreResource::get_from(&config, "plain-env").is_none());
    }
}
