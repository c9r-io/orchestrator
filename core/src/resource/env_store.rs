use crate::cli_types::{EnvStoreSpec, OrchestratorResource, ResourceKind, ResourceSpec};
use crate::config::{EnvStoreConfig, OrchestratorConfig};
use anyhow::{Result, anyhow};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for non-sensitive `EnvStore` resources.
pub struct EnvStoreResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload for the env store.
    pub spec: EnvStoreSpec,
}

impl Resource for EnvStoreResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::EnvStore
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let incoming = EnvStoreConfig {
            data: self.spec.data.clone(),
            sensitive: false,
        };
        let project = config.ensure_project(self.metadata.project.as_deref());
        Ok(super::helpers::apply_to_map(
            &mut project.env_stores,
            self.name(),
            incoming,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::EnvStore,
            &self.metadata,
            ResourceSpec::EnvStore(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .env_stores
            .get(name)
            .and_then(|store| {
                if store.sensitive {
                    None // SecretStore, not EnvStore
                } else {
                    Some(Self {
                        metadata: super::metadata_with_name(name),
                        spec: EnvStoreSpec {
                            data: store.data.clone(),
                        },
                    })
                }
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        config
            .project_mut(project_id)
            .map(|project| {
                matches!(project.env_stores.get(name), Some(store) if !store.sensitive)
                    && project.env_stores.remove(name).is_some()
            })
            .unwrap_or(false)
    }
}

/// Builds a typed `EnvStoreResource` from a generic manifest wrapper.
pub(super) fn build_env_store(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::EnvStore {
        return Err(anyhow!("resource kind/spec mismatch for EnvStore"));
    }
    match spec {
        ResourceSpec::EnvStore(spec) => Ok(RegisteredResource::EnvStore(EnvStoreResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for EnvStore")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::test_fixtures::make_config;

    fn make_env_store(name: &str) -> EnvStoreResource {
        EnvStoreResource {
            metadata: super::super::metadata_with_name(name),
            spec: EnvStoreSpec {
                data: [("KEY".to_string(), "value".to_string())].into(),
            },
        }
    }

    #[test]
    fn env_store_apply_and_get() {
        let mut config = make_config();
        let store = make_env_store("my-env");
        assert_eq!(
            store.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );

        let loaded =
            EnvStoreResource::get_from(&config, "my-env").expect("env store should be present");
        assert_eq!(loaded.spec.data.get("KEY").unwrap(), "value");
        assert_eq!(loaded.kind(), ResourceKind::EnvStore);
    }

    #[test]
    fn env_store_apply_unchanged() {
        let mut config = make_config();
        let store = make_env_store("es-unchanged");
        assert_eq!(
            store.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            store.apply(&mut config).expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn env_store_delete() {
        let mut config = make_config();
        let store = make_env_store("es-del");
        store.apply(&mut config).expect("apply");
        assert!(EnvStoreResource::delete_from(&mut config, "es-del"));
        assert!(EnvStoreResource::get_from(&config, "es-del").is_none());
    }

    #[test]
    fn env_store_validate_rejects_empty_name() {
        let store = make_env_store("");
        assert!(store.validate().is_err());
    }

    #[test]
    fn env_store_to_yaml() {
        let store = make_env_store("yaml-es");
        let yaml = store.to_yaml().expect("should serialize");
        assert!(yaml.contains("EnvStore"));
        assert!(yaml.contains("yaml-es"));
    }

    #[test]
    fn env_store_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(EnvStoreResource::get_from(&config, "no-such").is_none());
    }

    #[test]
    fn env_store_get_from_skips_sensitive() {
        let mut config = make_config();
        config.ensure_project(None).env_stores.insert(
            "secret-one".to_string(),
            EnvStoreConfig {
                data: [("S".to_string(), "v".to_string())].into(),
                sensitive: true,
            },
        );
        assert!(EnvStoreResource::get_from(&config, "secret-one").is_none());
    }
}
