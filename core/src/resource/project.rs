use crate::cli_types::{OrchestratorResource, ProjectSpec, ResourceKind, ResourceSpec};
use crate::config::{OrchestratorConfig, ProjectConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
pub struct ProjectResource {
    pub metadata: ResourceMetadata,
    pub spec: ProjectSpec,
}

impl Resource for ProjectResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Project
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = ProjectConfig {
            description: self.spec.description.clone(),
            workspaces: std::collections::HashMap::new(),
            agents: std::collections::HashMap::new(),
            workflows: std::collections::HashMap::new(),
        };
        match config.projects.get(self.name()) {
            None => {
                config.projects.insert(self.name().to_string(), incoming);
                ApplyResult::Created
            }
            Some(existing) => {
                if existing.description == incoming.description {
                    ApplyResult::Unchanged
                } else {
                    let mut next = existing.clone();
                    next.description = incoming.description;
                    config.projects.insert(self.name().to_string(), next);
                    ApplyResult::Configured
                }
            }
        }
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Project,
            &self.metadata,
            ResourceSpec::Project(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.projects.get(name).map(|project| Self {
            metadata: super::metadata_with_name(name),
            spec: ProjectSpec {
                description: project.description.clone(),
            },
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        config.projects.remove(name).is_some()
    }
}

pub(super) fn build_project(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Project {
        return Err(anyhow!("resource kind/spec mismatch for Project"));
    }
    match spec {
        ResourceSpec::Project(spec) => Ok(RegisteredResource::Project(ProjectResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Project")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ResourceMetadata, ResourceSpec};
    use crate::resource::{dispatch_resource, API_VERSION};

    use super::super::test_fixtures::{make_config, project_manifest};

    #[test]
    fn project_resource_dispatch_and_kind() {
        let resource = dispatch_resource(project_manifest("my-proj", "A test project"))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::Project);
        assert_eq!(resource.name(), "my-proj");
    }

    #[test]
    fn project_resource_validate_accepts_valid() {
        let resource = dispatch_resource(project_manifest("valid-proj", "desc"))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn project_resource_validate_rejects_empty_name() {
        let resource =
            dispatch_resource(project_manifest("", "desc")).expect("dispatch should succeed");
        assert!(resource.validate().is_err());
    }

    #[test]
    fn project_resource_apply_created_then_unchanged() {
        let mut config = make_config();
        let resource =
            dispatch_resource(project_manifest("proj-a", "desc")).expect("dispatch should succeed");
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);
        assert_eq!(resource.apply(&mut config), ApplyResult::Unchanged);
    }

    #[test]
    fn project_resource_apply_configured_on_change() {
        let mut config = make_config();
        let r1 =
            dispatch_resource(project_manifest("proj-b", "v1")).expect("dispatch should succeed");
        assert_eq!(r1.apply(&mut config), ApplyResult::Created);

        let r2 =
            dispatch_resource(project_manifest("proj-b", "v2")).expect("dispatch should succeed");
        assert_eq!(r2.apply(&mut config), ApplyResult::Configured);
    }

    #[test]
    fn project_resource_get_from_and_delete_from() {
        let mut config = make_config();
        let resource = dispatch_resource(project_manifest("proj-del", "desc"))
            .expect("dispatch should succeed");
        resource.apply(&mut config);

        let loaded = ProjectResource::get_from(&config, "proj-del");
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().spec.description, Some("desc".to_string()));

        assert!(ProjectResource::delete_from(&mut config, "proj-del"));
        assert!(ProjectResource::get_from(&config, "proj-del").is_none());
    }

    #[test]
    fn project_resource_delete_returns_false_when_missing() {
        let mut config = make_config();
        assert!(!ProjectResource::delete_from(&mut config, "nonexistent"));
    }

    #[test]
    fn project_resource_to_yaml() {
        let resource = dispatch_resource(project_manifest("yaml-proj", "desc"))
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: Project"));
        assert!(yaml.contains("yaml-proj"));
    }

    #[test]
    fn build_project_rejects_wrong_kind() {
        use crate::cli_types::DefaultsSpec;
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Project,
            metadata: ResourceMetadata {
                name: "bad".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Defaults(DefaultsSpec {
                project: String::new(),
                workspace: String::new(),
                workflow: String::new(),
            }),
        };
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }
}
