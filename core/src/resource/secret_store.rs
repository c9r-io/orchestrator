use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, SecretStoreSpec};
use crate::config::{OrchestratorConfig, SecretStoreConfig};
use anyhow::{Result, anyhow};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for sensitive `SecretStore` resources.
pub struct SecretStoreResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload for the secret store.
    pub spec: SecretStoreSpec,
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
        // Reads render secret values as `ENCRYPTED_PLACEHOLDER`, and a described
        // resource stays an apply-compatible manifest by design
        // (`get_resource_supports_named_queries_describe_and_selector_helpers`
        // asserts that). Those two facts together make describe -> edit -> apply
        // a way to overwrite real secrets with the literal placeholder, silently
        // and irreversibly. Applying a redacted manifest is therefore rejected
        // rather than treated as an edit: the manifest does not carry the value
        // it appears to carry, so no interpretation of it is safe.
        //
        // Deliberately not implemented as "placeholder means keep the stored
        // value": that would make a manifest's meaning depend on prior state,
        // and these manifests are declarative.
        if let Some(key) = self
            .spec
            .data
            .iter()
            .find(|(_, value)| value.as_str() == crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER)
            .map(|(key, _)| key)
        {
            return Err(anyhow!(
                "[secret_value_placeholder_rejected] SecretStore '{}' key '{}' carries the redaction placeholder '{}' rather than a value. \
                 Reads redact secret values, so a manifest obtained from `get` or `describe` cannot be applied back. \
                 Supply the real value for every key: apply replaces the whole store, so a key omitted here is deleted, not preserved.",
                self.name(),
                key,
                crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER
            ));
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let incoming = SecretStoreConfig {
            data: self.spec.data.clone(),
        };
        let project = config.ensure_project(self.metadata.project.as_deref());
        Ok(super::helpers::apply_to_map(
            &mut project.secret_stores,
            self.name(),
            incoming,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::SecretStore,
            &self.metadata,
            ResourceSpec::SecretStore(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .secret_stores
            .get(name)
            .map(|store| Self {
                metadata: super::metadata_with_name(name),
                spec: SecretStoreSpec {
                    data: store.data.clone(),
                },
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        config
            .project_mut(project_id)
            .map(|project| project.secret_stores.remove(name).is_some())
            .unwrap_or(false)
    }
}

/// Builds a typed `SecretStoreResource` from a generic manifest wrapper.
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
        ResourceSpec::SecretStore(spec) => {
            Ok(RegisteredResource::SecretStore(SecretStoreResource {
                metadata,
                spec,
            }))
        }
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
            spec: SecretStoreSpec {
                data: [("API_KEY".to_string(), "sk-secret".to_string())].into(),
            },
        }
    }

    #[test]
    fn secret_store_apply_and_get() {
        let mut config = make_config();
        let store = make_secret_store("my-secrets");
        assert_eq!(
            store.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );

        let loaded = SecretStoreResource::get_from(&config, "my-secrets")
            .expect("secret store should be present");
        assert_eq!(loaded.spec.data.get("API_KEY").unwrap(), "sk-secret");
        assert_eq!(loaded.kind(), ResourceKind::SecretStore);

        // Underlying config should be in secret_stores
        assert!(
            config
                .default_project()
                .unwrap()
                .secret_stores
                .contains_key("my-secrets")
        );
    }

    #[test]
    fn secret_store_apply_unchanged() {
        let mut config = make_config();
        let store = make_secret_store("ss-unchanged");
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
    fn secret_store_delete() {
        let mut config = make_config();
        let store = make_secret_store("ss-del");
        store.apply(&mut config).expect("apply");
        assert!(SecretStoreResource::delete_from(&mut config, "ss-del"));
        assert!(SecretStoreResource::get_from(&config, "ss-del").is_none());
    }

    #[test]
    fn secret_store_validate_rejects_empty_name() {
        let store = make_secret_store("");
        assert!(store.validate().is_err());
    }

    #[test]
    fn secret_store_validate_rejects_the_redaction_placeholder() {
        let store = SecretStoreResource {
            metadata: super::super::metadata_with_name("round-trip"),
            spec: SecretStoreSpec {
                data: [(
                    "API_KEY".to_string(),
                    crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER.to_string(),
                )]
                .into(),
            },
        };
        let error = store
            .validate()
            .expect_err("a redacted manifest must not apply back over real values");
        let message = error.to_string();
        assert!(message.contains("secret_value_placeholder_rejected"));
        // The diagnostic has to name the offending key: a store with twenty
        // keys and one placeholder is the case where "somewhere in this
        // manifest" costs the operator the most.
        assert!(message.contains("API_KEY"));
    }

    #[test]
    fn secret_store_validate_accepts_a_value_merely_containing_the_placeholder() {
        // The check is equality, not `contains`. A real secret that happens to
        // embed the placeholder text is still a real secret, and rejecting it
        // would make a legitimate value unstorable — the over-reaching half of
        // the matcher, which costs nothing until someone writes that value.
        let store = SecretStoreResource {
            metadata: super::super::metadata_with_name("substring"),
            spec: SecretStoreSpec {
                data: [(
                    "API_KEY".to_string(),
                    format!(
                        "prefix-{}-suffix",
                        crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER
                    ),
                )]
                .into(),
            },
        };
        store
            .validate()
            .expect("a value containing the placeholder is not a redacted value");
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
    fn secret_store_and_env_store_same_name_coexist() {
        use crate::cli_types::EnvStoreSpec;
        use crate::resource::env_store::EnvStoreResource;

        let mut config = make_config();

        // Apply EnvStore with name "shared"
        let env = EnvStoreResource {
            metadata: super::super::metadata_with_name("shared"),
            spec: EnvStoreSpec {
                data: [("ENV_KEY".to_string(), "env_val".to_string())].into(),
            },
        };
        env.apply(&mut config).expect("apply env");

        // Apply SecretStore with same name "shared"
        let secret = make_secret_store("shared");
        secret.apply(&mut config).expect("apply secret");

        // Both should coexist
        assert!(EnvStoreResource::get_from(&config, "shared").is_some());
        assert!(SecretStoreResource::get_from(&config, "shared").is_some());
    }
}
