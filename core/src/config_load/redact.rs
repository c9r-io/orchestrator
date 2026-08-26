//! The egress boundary for `OrchestratorConfig`.
//!
//! The in-memory config holds **decrypted** SecretStore values — the load path runs
//! `decrypt_resource_spec_json` over every resource — so anything that renders a whole
//! config to a caller is a place secrets can leave the daemon. Before FR-175 the only
//! egress that redacted was the one that wrote a persisted snapshot, and it redacted by
//! calling a private helper that two modules had each copied. `manifest export` and
//! `debug --component config` serialized `read_active_config(state).config` directly and
//! emitted cleartext; the first of those is reachable by the **read-only** role.
//!
//! [`RedactedConfig`] makes that a type rather than a habit. It has one constructor, the
//! constructor redacts, and the export helpers in `crate::resource::export` accept
//! nothing else — so a future export path cannot be written that forgets.
//!
//! The residue is named rather than implied. Types close the *manifest* family and only
//! that family: a caller can still reach `active.config` and hand it to
//! `serde_yaml::to_string`, which serializes whatever it is given. `service::system::
//! debug_info` is such a caller and passes a `RedactedConfig` by rule; a new one could
//! pass the raw config and nothing would stop it. What holds that half is the tests in
//! `service::resource::tests` and the sentinel sweep in
//! `scripts/qa/test-secret-egress-redaction.sh`, not the compiler. See DD-194.

use crate::config::OrchestratorConfig;
use crate::secret_store_crypto::redact_secret_data_map;
use serde::Serialize;

/// A config whose SecretStore values have been replaced with
/// [`ENCRYPTED_PLACEHOLDER`](crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER).
///
/// Serializes as the config it wraps (`#[serde(transparent)]`), so a caller that needs the
/// whole document — `debug --component config` — gets the same shape it had before, minus
/// the values.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct RedactedConfig(OrchestratorConfig);

impl RedactedConfig {
    /// The only way to build one, which is the point.
    ///
    /// Two stores of SecretStore values exist and both must be covered, because different
    /// egress paths read different ones. `export_manifest_resources` reads the typed
    /// `projects[].secret_stores` map and never looks at `resource_store`; serializing the
    /// whole config reads both, since `crd::writeback` mirrors every SecretStore into
    /// `resource_store` as a CR. Redacting only the typed map would leave
    /// `debug --component config` leaking through the mirror, and redacting only the
    /// mirror would leave `manifest export` leaking through the typed map. Neither half is
    /// redundant and each is covered by its own test.
    ///
    /// `custom_resources` is deliberately not walked: it holds instances of **non**-builtin
    /// CRD kinds only. The loader populates it under an `is_builtin_kind` guard
    /// (`persistence::repository::config`), and `apply` routes a `kind: SecretStore`
    /// document to `ParsedManifest::Builtin`, never to `Custom`. A SecretStore cannot
    /// arrive there.
    pub fn new(config: &OrchestratorConfig) -> Self {
        let mut redacted = config.clone();
        for project in redacted.projects.values_mut() {
            for store in project.secret_stores.values_mut() {
                for value in store.data.values_mut() {
                    *value = crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER.to_string();
                }
            }
        }
        for resource in redacted.resource_store.resources_mut().values_mut() {
            if resource.kind != "SecretStore" {
                continue;
            }
            if let Some(spec) = resource.spec.as_object_mut()
                && let Some(data) = spec.get_mut("data").and_then(|value| value.as_object_mut())
            {
                redact_secret_data_map(data);
            }
        }
        Self(redacted)
    }

    /// The wrapped config, for readers that need to walk it rather than serialize it.
    ///
    /// This is not an escape hatch: what comes back is the redacted copy, and the original
    /// is not reachable from here.
    pub fn as_config(&self) -> &OrchestratorConfig {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SecretStoreConfig;
    use crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER;

    const SENTINEL: &str = "sk-fr175-redact-unit-sentinel";

    fn config_with_typed_store() -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        let mut data = std::collections::HashMap::new();
        data.insert("OPENAI_API_KEY".to_string(), SENTINEL.to_string());
        config
            .ensure_project(Some(crate::config::DEFAULT_PROJECT_ID))
            .secret_stores
            .insert("api-keys".to_string(), SecretStoreConfig { data });
        config
    }

    // The mirror is produced by the real mechanism rather than hand-built, so that a
    // writeback which stops mirroring SecretStore takes the premise test below down with
    // it instead of leaving these cases quietly asserting over one store.
    //
    // `sync_config_snapshot_to_store`, not `reconcile_all_builtins`: the latter covers
    // only Project and RuntimePolicy. Using it here left the mirror empty and the premise
    // test caught it, which is the whole reason the premise test exists.
    fn config_with_mirrored_store() -> OrchestratorConfig {
        let mut config = config_with_typed_store();
        crate::crd::writeback::sync_config_snapshot_to_store(&mut config);
        config
    }

    // The before-run. Without it the two tests below prove only that the sentinel is
    // absent from the output, which a config that never carried it also satisfies.
    #[test]
    fn the_unredacted_config_carries_the_secret_in_both_stores() {
        let config = config_with_mirrored_store();
        let raw = serde_json::to_string(&config).expect("serialize");
        assert!(
            raw.matches(SENTINEL).count() >= 2,
            "premise failed: the fixture must carry the secret in the typed map and in \
             resource_store, so that redacting one and not the other is visible; got {} \
             occurrence(s)",
            raw.matches(SENTINEL).count()
        );
    }

    #[test]
    fn redaction_covers_the_typed_secret_store_map() {
        let redacted = RedactedConfig::new(&config_with_typed_store());
        let value = redacted
            .as_config()
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .and_then(|project| project.secret_stores.get("api-keys"))
            .and_then(|store| store.data.get("OPENAI_API_KEY"))
            .expect("the store must survive redaction");
        // Both halves: absence alone would pass on a redactor that deleted the key.
        assert_eq!(value, ENCRYPTED_PLACEHOLDER);
        assert_ne!(value, SENTINEL);
    }

    #[test]
    fn redaction_covers_the_resource_store_mirror() {
        let redacted = RedactedConfig::new(&config_with_mirrored_store());
        let mirrored = redacted
            .as_config()
            .resource_store
            .list_by_kind("SecretStore")
            .into_iter()
            .next()
            .expect("the mirrored CR must survive redaction");
        let value = mirrored
            .spec
            .get("data")
            .and_then(|data| data.get("OPENAI_API_KEY"))
            .and_then(|value| value.as_str())
            .expect("the mirrored key must survive redaction");
        assert_eq!(value, ENCRYPTED_PLACEHOLDER);
        assert_ne!(value, SENTINEL);
    }

    // The whole-document assertion the two above cannot make: nothing anywhere in the
    // serialized config still carries the value, whichever field it might have reached.
    #[test]
    fn no_serialization_of_a_redacted_config_carries_the_secret() {
        let redacted = RedactedConfig::new(&config_with_mirrored_store());
        let json = serde_json::to_string(&redacted).expect("serialize json");
        let yaml = serde_yaml::to_string(&redacted).expect("serialize yaml");
        for (format, rendered) in [("json", &json), ("yaml", &yaml)] {
            assert!(
                !rendered.contains(SENTINEL),
                "{format} rendering of a redacted config still carries the secret"
            );
            assert!(
                rendered.contains(ENCRYPTED_PLACEHOLDER),
                "{format} rendering of a redacted config dropped the store instead of \
                 redacting it; the placeholder must be present, not merely the value absent"
            );
        }
    }

    // `#[serde(transparent)]` is load-bearing: `debug --component config` serializes a
    // RedactedConfig where it used to serialize an OrchestratorConfig, and a wrapper that
    // added a level of nesting would silently change that command's output shape.
    #[test]
    fn serializing_the_wrapper_matches_serializing_the_inner_config() {
        let redacted = RedactedConfig::new(&config_with_mirrored_store());
        assert_eq!(
            serde_yaml::to_string(&redacted).expect("wrapper"),
            serde_yaml::to_string(redacted.as_config()).expect("inner"),
        );
    }
}
