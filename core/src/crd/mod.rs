/// Builtin CRD definitions shipped with the orchestrator.
pub mod builtin_defs;
/// Lifecycle hook execution for CRD-backed resources.
pub mod hooks;
/// CRD plugin execution engine (interceptors, transformers, cron tasks).
pub mod plugins;
/// Helpers that project CRD resources into CLI-facing views.
pub mod projection;
/// Lookup helpers for CRDs and custom resources.
pub mod resolve;
/// JSON schema helpers used by CRD validation.
pub mod schema;
/// Scope model for custom resource definitions.
pub mod scope;
/// In-memory stores and persistence helpers for CRD data.
pub mod store;
/// Public CRD and custom-resource data types.
pub mod types;
/// Validation logic for CRD definitions and instances.
pub mod validate;
/// Writeback helpers that materialize config changes into resource manifests.
pub mod writeback;

use crate::cli_types::OrchestratorResource;
use crate::config::OrchestratorConfig;
use crate::resource::ApplyResult;
use anyhow::{Result, anyhow};
use orchestrator_config::plugin_policy::PluginPolicy;
use types::{CrdManifest, CustomResource, CustomResourceManifest};

/// Tri-state parse result for YAML manifests.
pub enum ParsedManifest {
    /// A builtin resource kind (Workspace, Agent, Workflow, etc.)
    Builtin(Box<OrchestratorResource>),
    /// A CustomResourceDefinition manifest
    Crd(CrdManifest),
    /// A custom resource instance (kind defined by a CRD)
    Custom(CustomResourceManifest),
}

/// Apply a CRD definition to the config.
pub fn apply_crd(
    config: &mut OrchestratorConfig,
    manifest: CrdManifest,
    plugin_policy: &PluginPolicy,
) -> Result<ApplyResult> {
    validate::validate_crd_definition(config, &manifest, plugin_policy)?;

    let crd = manifest.spec.into_crd();
    let key = crd.kind.clone();

    let result = match config.custom_resource_definitions.get(&key) {
        None => {
            config.custom_resource_definitions.insert(key, crd);
            ApplyResult::Created
        }
        Some(existing) => {
            if crate::resource::serializes_equal(existing, &crd) {
                ApplyResult::Unchanged
            } else {
                config.custom_resource_definitions.insert(key, crd);
                ApplyResult::Configured
            }
        }
    };

    Ok(result)
}

/// Apply a custom resource instance to the config.
pub fn apply_custom_resource(
    config: &mut OrchestratorConfig,
    manifest: CustomResourceManifest,
) -> Result<ApplyResult> {
    validate::validate_custom_resource(config, &manifest)?;

    let crd = resolve::find_crd_for_kind(config, &manifest.kind)?;

    // Determine action for hooks
    let storage_key = format!("{}/{}", manifest.kind, manifest.metadata.name);
    let is_update = config.custom_resources.contains_key(&storage_key);
    let action = if is_update { "update" } else { "create" };

    // Execute hook before applying
    hooks::run_hook_if_defined(
        &crd.hooks,
        &manifest.kind,
        &manifest.metadata.name,
        action,
        &manifest.spec,
    )?;

    let now = chrono::Utc::now().to_rfc3339();

    let result = match config.custom_resources.get(&storage_key) {
        None => {
            let cr = CustomResource {
                kind: manifest.kind,
                api_version: manifest.api_version,
                metadata: manifest.metadata,
                spec: manifest.spec,
                generation: 1,
                created_at: now.clone(),
                updated_at: now,
            };
            config.resource_store.put(cr.clone());
            config.custom_resources.insert(storage_key, cr);
            ApplyResult::Created
        }
        Some(existing) => {
            if existing.spec == manifest.spec
                && existing.api_version == manifest.api_version
                && existing.metadata == manifest.metadata
            {
                ApplyResult::Unchanged
            } else {
                let cr = CustomResource {
                    kind: manifest.kind,
                    api_version: manifest.api_version,
                    metadata: manifest.metadata,
                    spec: manifest.spec,
                    generation: existing.generation + 1,
                    created_at: existing.created_at.clone(),
                    updated_at: now,
                };
                config.resource_store.put(cr.clone());
                config.custom_resources.insert(storage_key, cr);
                ApplyResult::Configured
            }
        }
    };

    Ok(result)
}

/// Delete a custom resource instance from the config.
pub fn delete_custom_resource(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
) -> Result<bool> {
    let storage_key = format!("{}/{}", kind, name);

    // Look up the CR to get its spec for the hook
    let cr = match config.custom_resources.get(&storage_key) {
        Some(cr) => cr.clone(),
        None => return Ok(false),
    };

    // Find CRD for hooks (best-effort — CRD might have been deleted)
    if let Ok(crd) = resolve::find_crd_for_kind(config, kind) {
        hooks::run_hook_if_defined(&crd.hooks, kind, name, "delete", &cr.spec)?;
    }

    config.custom_resources.remove(&storage_key);
    config.resource_store.remove_first_by_kind_name(kind, name);
    Ok(true)
}

/// Delete a CRD. Fails if custom resources of this kind still exist.
pub fn delete_crd(config: &mut OrchestratorConfig, kind: &str) -> Result<bool> {
    // Check for existing CRs of this kind
    let has_instances = config
        .custom_resources
        .keys()
        .any(|key| key.starts_with(&format!("{}/", kind)));

    if has_instances {
        return Err(anyhow!(
            "cannot delete CRD '{}': custom resource instances still exist (delete them first)",
            kind
        ));
    }

    Ok(config.custom_resource_definitions.remove(kind).is_some())
}

/// Get the display kind string for a CRD-based resource.
/// Returns the kind in lowercase for consistent CLI output.
pub fn crd_kind_display(kind: &str) -> String {
    kind.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::ResourceMetadata;
    use crate::crd::types::{CelValidationRule, CrdHooks, CrdSpec, CrdVersion};
    use orchestrator_config::plugin_policy::{PluginPolicy, PluginPolicyMode};

    fn audit_policy() -> PluginPolicy {
        PluginPolicy {
            mode: PluginPolicyMode::Audit,
            ..Default::default()
        }
    }

    fn make_crd_manifest() -> CrdManifest {
        CrdManifest {
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: "foos.test.dev".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: CrdSpec {
                kind: "Foo".to_string(),
                plural: "foos".to_string(),
                short_names: vec!["f".to_string()],
                group: "test.dev".to_string(),
                versions: vec![CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["bar"],
                        "properties": {
                            "bar": {"type": "string"}
                        }
                    }),
                    served: true,
                    cel_rules: vec![],
                }],
                hooks: CrdHooks::default(),
                scope: crate::crd::scope::CrdScope::default(),
                builtin: false,
                plugins: vec![],
            },
        }
    }

    fn make_cr_manifest(name: &str) -> CustomResourceManifest {
        CustomResourceManifest {
            api_version: "test.dev/v1".to_string(),
            kind: "Foo".to_string(),
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({"bar": "hello"}),
        }
    }

    #[test]
    fn apply_crd_creates() {
        let mut config = OrchestratorConfig::default();
        let result = apply_crd(&mut config, make_crd_manifest(), &audit_policy())
            .expect("apply should succeed");
        assert_eq!(result, ApplyResult::Created);
        assert!(config.custom_resource_definitions.contains_key("Foo"));
    }

    #[test]
    fn apply_crd_unchanged() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("first apply");
        let result =
            apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("second apply");
        assert_eq!(result, ApplyResult::Unchanged);
    }

    #[test]
    fn apply_crd_configured() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("first apply");
        let mut manifest = make_crd_manifest();
        manifest.spec.short_names.push("fo".to_string());
        let result = apply_crd(&mut config, manifest, &audit_policy()).expect("second apply");
        assert_eq!(result, ApplyResult::Configured);
    }

    #[test]
    fn apply_custom_resource_creates() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("apply crd");
        let result =
            apply_custom_resource(&mut config, make_cr_manifest("my-foo")).expect("apply cr");
        assert_eq!(result, ApplyResult::Created);
        assert!(config.custom_resources.contains_key("Foo/my-foo"));
    }

    #[test]
    fn apply_custom_resource_unchanged() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("apply crd");
        apply_custom_resource(&mut config, make_cr_manifest("my-foo")).expect("first apply");
        let result =
            apply_custom_resource(&mut config, make_cr_manifest("my-foo")).expect("second apply");
        assert_eq!(result, ApplyResult::Unchanged);
    }

    #[test]
    fn apply_custom_resource_schema_validation_fails() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("apply crd");
        let manifest = CustomResourceManifest {
            api_version: "test.dev/v1".to_string(),
            kind: "Foo".to_string(),
            metadata: ResourceMetadata {
                name: "bad-foo".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({}), // missing required "bar"
        };
        assert!(apply_custom_resource(&mut config, manifest).is_err());
    }

    #[test]
    fn delete_custom_resource_ok() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("apply crd");
        apply_custom_resource(&mut config, make_cr_manifest("del-foo")).expect("apply cr");
        assert!(
            delete_custom_resource(&mut config, "Foo", "del-foo").expect("delete should succeed")
        );
        assert!(!config.custom_resources.contains_key("Foo/del-foo"));
    }

    #[test]
    fn delete_custom_resource_not_found() {
        let mut config = OrchestratorConfig::default();
        assert!(
            !delete_custom_resource(&mut config, "Foo", "missing").expect("delete returns false")
        );
    }

    #[test]
    fn delete_crd_ok() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("apply crd");
        assert!(delete_crd(&mut config, "Foo").expect("delete should succeed"));
        assert!(!config.custom_resource_definitions.contains_key("Foo"));
    }

    #[test]
    fn delete_crd_blocked_by_instances() {
        let mut config = OrchestratorConfig::default();
        apply_crd(&mut config, make_crd_manifest(), &audit_policy()).expect("apply crd");
        apply_custom_resource(&mut config, make_cr_manifest("blocker")).expect("apply cr");
        assert!(delete_crd(&mut config, "Foo").is_err());
    }

    #[test]
    fn apply_custom_resource_with_cel_validation() {
        let mut config = OrchestratorConfig::default();
        let mut crd_manifest = make_crd_manifest();
        crd_manifest.spec.versions[0]
            .cel_rules
            .push(CelValidationRule {
                rule: r#"size(self.bar) > 2"#.to_string(),
                message: "bar must be longer than 2 chars".to_string(),
            });
        apply_crd(&mut config, crd_manifest, &audit_policy()).expect("apply crd");

        // Valid: bar = "hello" (len > 2)
        let result =
            apply_custom_resource(&mut config, make_cr_manifest("cel-ok")).expect("should pass");
        assert_eq!(result, ApplyResult::Created);

        // Invalid: bar = "hi" (len <= 2)
        let bad_manifest = CustomResourceManifest {
            api_version: "test.dev/v1".to_string(),
            kind: "Foo".to_string(),
            metadata: ResourceMetadata {
                name: "cel-bad".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({"bar": "hi"}),
        };
        assert!(apply_custom_resource(&mut config, bad_manifest).is_err());
    }

    #[test]
    fn crd_kind_display_lowercases() {
        assert_eq!(crd_kind_display("PromptLibrary"), "promptlibrary");
    }
}
