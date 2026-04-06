use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, WorkspaceSpec};
use crate::config::{OrchestratorConfig, WorkspaceConfig};
use anyhow::{Result, anyhow};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for `Workspace` resources.
pub struct WorkspaceResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload for the workspace.
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

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let mut metadata = self.metadata.clone();
        metadata.project = Some(
            config
                .effective_project_id(metadata.project.as_deref())
                .to_string(),
        );
        Ok(super::apply_to_store(
            config,
            "Workspace",
            self.name(),
            &metadata,
            serde_json::to_value(&self.spec)?,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Workspace,
            &self.metadata,
            ResourceSpec::Workspace(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .workspaces
            .get(name)
            .map(|workspace| Self {
                metadata: super::metadata_from_store(config, "Workspace", name, project_id),
                spec: workspace_config_to_spec(workspace),
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        super::helpers::delete_from_store_project(config, "Workspace", name, project_id)
    }
}

/// Builds a typed `WorkspaceResource` from a generic manifest wrapper.
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

/// Converts a workspace manifest spec into runtime config.
///
/// Relative `root_path` values are resolved against the current working
/// directory so that the stored config always contains an absolute path.
pub(crate) fn workspace_spec_to_config(spec: &WorkspaceSpec) -> WorkspaceConfig {
    use crate::config::HealthPolicyConfig;
    let root_path = {
        let p = std::path::Path::new(&spec.root_path);
        if p.is_absolute() {
            spec.root_path.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(p)
                .to_string_lossy()
                .to_string()
        }
    };
    WorkspaceConfig {
        root_path,
        qa_targets: spec.qa_targets.clone(),
        ticket_dir: spec.ticket_dir.clone(),
        self_referential: spec.self_referential,
        health_policy: spec
            .health_policy
            .as_ref()
            .map(|hp| HealthPolicyConfig {
                disease_duration_hours: hp
                    .disease_duration_hours
                    .unwrap_or_else(|| HealthPolicyConfig::default().disease_duration_hours),
                disease_threshold: hp
                    .disease_threshold
                    .unwrap_or_else(|| HealthPolicyConfig::default().disease_threshold),
                capability_success_threshold: hp
                    .capability_success_threshold
                    .unwrap_or_else(|| HealthPolicyConfig::default().capability_success_threshold),
            })
            .unwrap_or_default(),
        artifacts_dir: spec.artifacts_dir.clone(),
    }
}

/// Converts runtime workspace config into its manifest spec representation.
pub(crate) fn workspace_config_to_spec(config: &WorkspaceConfig) -> WorkspaceSpec {
    use crate::cli_types::HealthPolicySpec;
    WorkspaceSpec {
        root_path: config.root_path.clone(),
        qa_targets: config.qa_targets.clone(),
        ticket_dir: config.ticket_dir.clone(),
        self_referential: config.self_referential,
        health_policy: if config.health_policy.is_default() {
            None
        } else {
            Some(HealthPolicySpec {
                disease_duration_hours: Some(config.health_policy.disease_duration_hours),
                disease_threshold: Some(config.health_policy.disease_threshold),
                capability_success_threshold: Some(
                    config.health_policy.capability_success_threshold,
                ),
            })
        },
        artifacts_dir: config.artifacts_dir.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ResourceMetadata, ResourceSpec};
    use crate::config_load::read_active_config;
    use crate::resource::{API_VERSION, dispatch_resource};
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
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );

        let loaded = WorkspaceResource::get_from(&config, "ws-roundtrip")
            .expect("workspace should be present in config");
        // root_path is absolutized at apply-time against CWD
        assert!(
            std::path::Path::new(&loaded.spec.root_path).is_absolute(),
            "root_path should be absolute after apply: {}",
            loaded.spec.root_path
        );
        assert!(
            loaded.spec.root_path.ends_with("workspace/ws-roundtrip"),
            "root_path should end with original relative path: {}",
            loaded.spec.root_path
        );
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
                health_policy: None,
                artifacts_dir: None,
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
                health_policy: None,
                artifacts_dir: None,
            },
        };
        let err = ws.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("ticket_dir"));
    }

    #[test]
    fn workspace_get_from_without_stored_metadata() {
        let mut config = make_config();
        // Insert workspace directly without resource_meta
        config.ensure_project(None).workspaces.insert(
            "bare-ws".to_string(),
            WorkspaceConfig {
                root_path: "/bare".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: Default::default(),
                artifacts_dir: None,
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
                health_policy: None,
                artifacts_dir: None,
            }),
        };
        let rr = dispatch_resource(resource).expect("dispatch workspace resource");
        rr.apply(&mut config).expect("apply");
        assert!(
            config
                .resource_store
                .get_namespaced("Workspace", crate::config::DEFAULT_PROJECT_ID, "meta-ws")
                .is_some()
        );

        WorkspaceResource::delete_from(&mut config, "meta-ws");
        assert!(
            config
                .resource_store
                .get_namespaced("Workspace", crate::config::DEFAULT_PROJECT_ID, "meta-ws")
                .is_none()
        );
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
                health_policy: None,
                artifacts_dir: None,
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
            health_policy: None,
            artifacts_dir: None,
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
                health_policy: None,
                artifacts_dir: None,
            }),
        };
        let rr = dispatch_resource(resource).expect("dispatch workspace resource");
        rr.apply(&mut config).expect("apply");

        let cr = config
            .resource_store
            .get_namespaced(
                "Workspace",
                crate::config::DEFAULT_PROJECT_ID,
                "store-meta-ws",
            )
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
