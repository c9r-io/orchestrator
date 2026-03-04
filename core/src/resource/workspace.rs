use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, WorkspaceSpec};
use crate::config::{OrchestratorConfig, WorkspaceConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
pub struct WorkspaceResource {
    pub metadata: ResourceMetadata,
    pub spec: WorkspaceSpec,
}

impl Resource for WorkspaceResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Workspace
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        if self.spec.root_path.trim().is_empty() {
            return Err(anyhow!("workspace.spec.root_path cannot be empty"));
        }
        if self.spec.ticket_dir.trim().is_empty() {
            return Err(anyhow!("workspace.spec.ticket_dir cannot be empty"));
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        use crate::crd::projection::CrdProjectable;
        let incoming = workspace_spec_to_config(&self.spec);
        let spec_value = incoming.to_cr_spec();
        super::apply_to_store(config, "Workspace", self.name(), &self.metadata, spec_value)
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Workspace,
            &self.metadata,
            ResourceSpec::Workspace(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.workspaces.get(name).map(|workspace| Self {
            metadata: super::metadata_from_store(config, "Workspace", name),
            spec: workspace_config_to_spec(workspace),
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        super::delete_from_store(config, "Workspace", name)
    }
}

pub(super) fn build_workspace(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Workspace {
        return Err(anyhow!("resource kind/spec mismatch for Workspace"));
    }
    match spec {
        ResourceSpec::Workspace(spec) => Ok(RegisteredResource::Workspace(WorkspaceResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Workspace")),
    }
}

pub(crate) fn workspace_spec_to_config(spec: &WorkspaceSpec) -> WorkspaceConfig {
    WorkspaceConfig {
        root_path: spec.root_path.clone(),
        qa_targets: spec.qa_targets.clone(),
        ticket_dir: spec.ticket_dir.clone(),
        self_referential: spec.self_referential,
    }
}

pub(crate) fn workspace_config_to_spec(config: &WorkspaceConfig) -> WorkspaceSpec {
    WorkspaceSpec {
        root_path: config.root_path.clone(),
        qa_targets: config.qa_targets.clone(),
        ticket_dir: config.ticket_dir.clone(),
        self_referential: config.self_referential,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ResourceMetadata, ResourceSpec};
    use crate::config_load::read_active_config;
    use crate::resource::{dispatch_resource, API_VERSION};
    use crate::test_utils::TestState;

    use super::super::test_fixtures::{make_config, workspace_manifest};

    #[test]
    fn workspace_resource_apply() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource =
            dispatch_resource(workspace_manifest("ws-roundtrip", "workspace/ws-roundtrip"))
                .expect("workspace dispatch should succeed");
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);

        let loaded = WorkspaceResource::get_from(&config, "ws-roundtrip")
            .expect("workspace should be present in config");
        assert_eq!(loaded.spec.root_path, "workspace/ws-roundtrip");
        assert_eq!(loaded.kind(), ResourceKind::Workspace);
    }

    #[test]
    fn workspace_validate_rejects_empty_root_path() {
        let ws = WorkspaceResource {
            metadata: super::super::metadata_with_name("ws-no-root"),
            spec: WorkspaceSpec {
                root_path: "  ".to_string(),
                qa_targets: vec![],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        };
        let err = ws.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("root_path"));
    }

    #[test]
    fn workspace_validate_rejects_empty_ticket_dir() {
        let ws = WorkspaceResource {
            metadata: super::super::metadata_with_name("ws-no-ticket"),
            spec: WorkspaceSpec {
                root_path: "/some/path".to_string(),
                qa_targets: vec![],
                ticket_dir: "  ".to_string(),
                self_referential: false,
            },
        };
        let err = ws.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("ticket_dir"));
    }

    #[test]
    fn workspace_get_from_without_stored_metadata() {
        let mut config = make_config();
        // Insert workspace directly without resource_meta
        config.workspaces.insert(
            "bare-ws".to_string(),
            WorkspaceConfig {
                root_path: "/bare".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        let loaded = WorkspaceResource::get_from(&config, "bare-ws")
            .expect("bare workspace should be returned");
        assert_eq!(loaded.metadata.name, "bare-ws");
        assert!(loaded.metadata.labels.is_none());
    }

    #[test]
    fn workspace_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(WorkspaceResource::get_from(&config, "nonexistent-ws").is_none());
    }

    #[test]
    fn workspace_delete_cleans_up_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "meta-ws".to_string(),
                project: None,
                labels: Some([("k".to_string(), "v".to_string())].into()),
                annotations: None,
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/meta".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            }),
        };
        let rr = dispatch_resource(resource).expect("dispatch workspace resource");
        rr.apply(&mut config);
        assert!(config.resource_store.get("Workspace", "meta-ws").is_some());

        WorkspaceResource::delete_from(&mut config, "meta-ws");
        assert!(config.resource_store.get("Workspace", "meta-ws").is_none());
    }

    #[test]
    fn workspace_to_yaml_includes_all_fields() {
        let workspace = WorkspaceResource {
            metadata: ResourceMetadata {
                name: "full-workspace".to_string(),
                project: None,
                labels: Some([("env".to_string(), "test".to_string())].into()),
                annotations: Some([("desc".to_string(), "test workspace".to_string())].into()),
            },
            spec: WorkspaceSpec {
                root_path: "/path/to/workspace".to_string(),
                qa_targets: vec!["docs/qa".to_string(), "tests".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        };
        let yaml = workspace.to_yaml().expect("should serialize");
        assert!(yaml.contains("full-workspace"));
        assert!(yaml.contains("/path/to/workspace"));
        assert!(yaml.contains("docs/qa"));
        assert!(yaml.contains("tickets"));
    }

    #[test]
    fn workspace_spec_config_roundtrip() {
        let spec = WorkspaceSpec {
            root_path: "/my/project".to_string(),
            qa_targets: vec!["src".to_string(), "tests".to_string()],
            ticket_dir: "docs/tickets".to_string(),
            self_referential: true,
        };
        let config = workspace_spec_to_config(&spec);
        assert_eq!(config.root_path, "/my/project");
        assert_eq!(config.qa_targets, vec!["src", "tests"]);
        assert_eq!(config.ticket_dir, "docs/tickets");
        assert!(config.self_referential);

        let back = workspace_config_to_spec(&config);
        assert_eq!(back.root_path, "/my/project");
        assert_eq!(back.qa_targets, vec!["src", "tests"]);
        assert!(back.self_referential);
    }

    #[test]
    fn workspace_apply_stores_resource_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "store-meta-ws".to_string(),
                project: None,
                labels: Some([("env".to_string(), "staging".to_string())].into()),
                annotations: Some([("note".to_string(), "test".to_string())].into()),
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/store-meta".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            }),
        };
        let rr = dispatch_resource(resource).expect("dispatch workspace resource");
        rr.apply(&mut config);

        let cr = config
            .resource_store
            .get("Workspace", "store-meta-ws")
            .expect("stored workspace CR should exist");
        assert_eq!(
            cr.metadata
                .labels
                .as_ref()
                .expect("labels should exist")
                .get("env")
                .expect("env label should exist"),
            "staging"
        );
        assert_eq!(
            cr.metadata
                .annotations
                .as_ref()
                .expect("annotations should exist")
                .get("note")
                .expect("note annotation should exist"),
            "test"
        );
    }
}
