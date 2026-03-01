use crate::cli_types::{DefaultsSpec, OrchestratorResource, ResourceKind, ResourceSpec};
use crate::config::{ConfigDefaults, OrchestratorConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
pub struct DefaultsResource {
    pub metadata: ResourceMetadata,
    pub spec: DefaultsSpec,
}

impl Resource for DefaultsResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Defaults
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = ConfigDefaults {
            project: self.spec.project.clone(),
            workspace: self.spec.workspace.clone(),
            workflow: self.spec.workflow.clone(),
        };
        if super::serializes_equal(&config.defaults, &incoming) {
            ApplyResult::Unchanged
        } else {
            config.defaults = incoming;
            ApplyResult::Configured
        }
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Defaults,
            &self.metadata,
            ResourceSpec::Defaults(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, _name: &str) -> Option<Self> {
        Some(Self {
            metadata: super::metadata_with_name("defaults"),
            spec: DefaultsSpec {
                project: config.defaults.project.clone(),
                workspace: config.defaults.workspace.clone(),
                workflow: config.defaults.workflow.clone(),
            },
        })
    }

    fn delete_from(_config: &mut OrchestratorConfig, _name: &str) -> bool {
        false
    }
}

pub(super) fn build_defaults(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Defaults {
        return Err(anyhow!("resource kind/spec mismatch for Defaults"));
    }
    match spec {
        ResourceSpec::Defaults(spec) => Ok(RegisteredResource::Defaults(DefaultsResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Defaults")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ProjectSpec, ResourceMetadata, ResourceSpec};
    use crate::resource::{dispatch_resource, API_VERSION};

    use super::super::test_fixtures::{defaults_manifest, make_config};

    #[test]
    fn defaults_resource_dispatch_and_kind() {
        let resource =
            dispatch_resource(defaults_manifest("p", "w", "f")).expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::Defaults);
        assert_eq!(resource.name(), "defaults");
    }

    #[test]
    fn defaults_resource_apply_unchanged_when_same() {
        let mut config = make_config();
        let r1 =
            dispatch_resource(defaults_manifest("p", "w", "f")).expect("dispatch should succeed");
        r1.apply(&mut config);
        // Apply same again -> unchanged
        assert_eq!(r1.apply(&mut config), ApplyResult::Unchanged);
    }

    #[test]
    fn defaults_resource_apply_configured_on_change() {
        let mut config = make_config();
        let r1 = dispatch_resource(defaults_manifest("p1", "w1", "f1"))
            .expect("dispatch should succeed");
        r1.apply(&mut config);

        let r2 = dispatch_resource(defaults_manifest("p2", "w2", "f2"))
            .expect("dispatch should succeed");
        assert_eq!(r2.apply(&mut config), ApplyResult::Configured);
    }

    #[test]
    fn defaults_resource_get_from_always_returns_some() {
        let config = make_config();
        let loaded = DefaultsResource::get_from(&config, "anything");
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().metadata.name, "defaults");
    }

    #[test]
    fn defaults_resource_delete_returns_false() {
        let mut config = make_config();
        assert!(!DefaultsResource::delete_from(&mut config, "defaults"));
    }

    #[test]
    fn defaults_resource_to_yaml() {
        let resource =
            dispatch_resource(defaults_manifest("proj", "", "")).expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: Defaults"));
        assert!(yaml.contains("proj"));
    }

    #[test]
    fn build_defaults_rejects_wrong_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Defaults,
            metadata: ResourceMetadata {
                name: "bad".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Project(ProjectSpec { description: None }),
        };
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }
}
