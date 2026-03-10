use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec};
use crate::config::OrchestratorConfig;

use super::{
    AgentResource, EnvStoreResource, ProjectResource, RegisteredResource, Resource,
    RuntimePolicyResource, SecretStoreResource, StepTemplateResource, WorkflowResource,
    WorkspaceResource, API_VERSION,
};

pub fn export_manifest_resources(config: &OrchestratorConfig) -> Vec<RegisteredResource> {
    let mut resources = Vec::new();
    if let Some(runtime_policy) = RuntimePolicyResource::get_from(config, "runtime") {
        resources.push(RegisteredResource::RuntimePolicy(runtime_policy));
    }
    for name in config.projects.keys() {
        if name.is_empty() {
            continue;
        }
        if let Some(project) = ProjectResource::get_from(config, name) {
            resources.push(RegisteredResource::Project(project));
        }
    }
    for (project_id, project) in &config.projects {
        for (name, workspace) in &project.workspaces {
            resources.push(RegisteredResource::Workspace(WorkspaceResource {
                metadata: crate::cli_types::ResourceMetadata {
                    name: name.clone(),
                    project: Some(project_id.clone()),
                    labels: None,
                    annotations: None,
                },
                spec: crate::cli_types::WorkspaceSpec {
                    root_path: workspace.root_path.clone(),
                    qa_targets: workspace.qa_targets.clone(),
                    ticket_dir: workspace.ticket_dir.clone(),
                    self_referential: workspace.self_referential,
                },
            }));
        }
        for (name, agent) in &project.agents {
            resources.push(RegisteredResource::Agent(Box::new(AgentResource {
                metadata: crate::cli_types::ResourceMetadata {
                    name: name.clone(),
                    project: Some(project_id.clone()),
                    labels: None,
                    annotations: None,
                },
                spec: super::agent::agent_config_to_spec(agent),
            })));
        }
        for (name, workflow) in &project.workflows {
            resources.push(RegisteredResource::Workflow(WorkflowResource {
                metadata: crate::cli_types::ResourceMetadata {
                    name: name.clone(),
                    project: Some(project_id.clone()),
                    labels: None,
                    annotations: None,
                },
                spec: super::workflow::workflow_config_to_spec(workflow),
            }));
        }
        for (name, template) in &project.step_templates {
            resources.push(RegisteredResource::StepTemplate(StepTemplateResource {
                metadata: crate::cli_types::ResourceMetadata {
                    name: name.clone(),
                    project: Some(project_id.clone()),
                    labels: None,
                    annotations: None,
                },
                spec: crate::cli_types::StepTemplateSpec {
                    prompt: template.prompt.clone(),
                    description: template.description.clone(),
                },
            }));
        }
        for (name, store) in &project.env_stores {
            let metadata = crate::cli_types::ResourceMetadata {
                name: name.clone(),
                project: Some(project_id.clone()),
                labels: None,
                annotations: None,
            };
            let spec = crate::cli_types::EnvStoreSpec {
                data: store.data.clone(),
            };
            if store.sensitive {
                resources.push(RegisteredResource::SecretStore(SecretStoreResource {
                    metadata,
                    spec,
                }));
            } else {
                resources.push(RegisteredResource::EnvStore(EnvStoreResource {
                    metadata,
                    spec,
                }));
            }
        }
    }
    resources
}

/// Export CRD definitions and custom resource instances as YAML-serializable values.
pub fn export_crd_documents(config: &OrchestratorConfig) -> Vec<serde_yml::Value> {
    let mut docs = Vec::new();

    // Export CRD definitions first (sorted by kind for deterministic output)
    let mut crd_keys: Vec<_> = config.custom_resource_definitions.keys().collect();
    crd_keys.sort();
    for key in crd_keys {
        if let Some(crd) = config.custom_resource_definitions.get(key) {
            // Skip builtin CRDs — they are auto-registered at init and would conflict on re-import
            if crd.builtin {
                continue;
            }
            let manifest = crate::crd::types::CrdManifest {
                api_version: "orchestrator.dev/v2".to_string(),
                metadata: crate::cli_types::ResourceMetadata {
                    name: format!("{}.{}", crd.plural, crd.group),
                    project: None,
                    labels: None,
                    annotations: None,
                },
                spec: crate::crd::types::CrdSpec {
                    kind: crd.kind.clone(),
                    plural: crd.plural.clone(),
                    short_names: crd.short_names.clone(),
                    group: crd.group.clone(),
                    versions: crd.versions.clone(),
                    hooks: crd.hooks.clone(),
                    scope: crd.scope,
                    builtin: crd.builtin,
                },
            };
            // Wrap with `kind: CustomResourceDefinition`
            let mut value = serde_yml::to_value(&manifest).unwrap_or_default();
            if let serde_yml::Value::Mapping(ref mut map) = value {
                map.insert(
                    serde_yml::Value::String("kind".to_string()),
                    serde_yml::Value::String("CustomResourceDefinition".to_string()),
                );
            }
            docs.push(value);
        }
    }

    // Export CR instances (sorted by storage key for deterministic output)
    let mut cr_keys: Vec<_> = config.custom_resources.keys().collect();
    cr_keys.sort();
    for key in cr_keys {
        if let Some(cr) = config.custom_resources.get(key) {
            let manifest = crate::crd::types::CustomResourceManifest {
                api_version: cr.api_version.clone(),
                kind: cr.kind.clone(),
                metadata: cr.metadata.clone(),
                spec: cr.spec.clone(),
            };
            if let Ok(value) = serde_yml::to_value(&manifest) {
                docs.push(value);
            }
        }
    }

    docs
}

pub fn export_manifest_documents(config: &OrchestratorConfig) -> Vec<OrchestratorResource> {
    export_manifest_resources(config)
        .into_iter()
        .map(|resource| match resource {
            RegisteredResource::Workspace(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Workspace,
                metadata: item.metadata,
                spec: ResourceSpec::Workspace(item.spec),
            },
            RegisteredResource::Agent(item) => {
                let item = *item;
                OrchestratorResource {
                    api_version: API_VERSION.to_string(),
                    kind: ResourceKind::Agent,
                    metadata: item.metadata,
                    spec: ResourceSpec::Agent(Box::new(item.spec)),
                }
            }
            RegisteredResource::Workflow(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Workflow,
                metadata: item.metadata,
                spec: ResourceSpec::Workflow(item.spec),
            },
            RegisteredResource::Project(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Project,
                metadata: item.metadata,
                spec: ResourceSpec::Project(item.spec),
            },
            RegisteredResource::RuntimePolicy(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::RuntimePolicy,
                metadata: item.metadata,
                spec: ResourceSpec::RuntimePolicy(item.spec),
            },
            RegisteredResource::StepTemplate(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::StepTemplate,
                metadata: item.metadata,
                spec: ResourceSpec::StepTemplate(item.spec),
            },
            RegisteredResource::ExecutionProfile(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::ExecutionProfile,
                metadata: item.metadata,
                spec: ResourceSpec::ExecutionProfile(item.spec),
            },
            RegisteredResource::EnvStore(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::EnvStore,
                metadata: item.metadata,
                spec: ResourceSpec::EnvStore(item.spec),
            },
            RegisteredResource::SecretStore(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::SecretStore,
                metadata: item.metadata,
                spec: ResourceSpec::EnvStore(item.spec),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ResourceMetadata, ResourceSpec, WorkspaceSpec};
    use crate::resource::{dispatch_resource, Resource, API_VERSION};

    use super::super::test_fixtures::{
        agent_manifest, make_config, project_manifest, workflow_manifest, workspace_manifest,
    };

    #[test]
    fn export_manifest_resources_includes_all_resource_types() {
        let mut config = make_config();
        // Add one of each
        let ws = dispatch_resource(workspace_manifest("exp-ws", "workspace/exp"))
            .expect("dispatch export workspace");
        ws.apply(&mut config).expect("apply");
        let ag = dispatch_resource(agent_manifest("exp-ag", "cmd")).expect("dispatch export agent");
        ag.apply(&mut config).expect("apply");
        let wf = dispatch_resource(workflow_manifest("exp-wf")).expect("dispatch export workflow");
        wf.apply(&mut config).expect("apply");
        let pr =
            dispatch_resource(project_manifest("exp-pr", "d")).expect("dispatch export project");
        pr.apply(&mut config).expect("apply");

        // Add EnvStore and SecretStore
        config
            .projects
            .get_mut("default")
            .unwrap()
            .env_stores
            .insert(
                "shared-config".to_string(),
                crate::config::EnvStoreConfig {
                    data: [("K".to_string(), "V".to_string())].into(),
                    sensitive: false,
                },
            );
        config
            .projects
            .get_mut("default")
            .unwrap()
            .env_stores
            .insert(
                "api-keys".to_string(),
                crate::config::EnvStoreConfig {
                    data: [("SECRET".to_string(), "val".to_string())].into(),
                    sensitive: true,
                },
            );

        let resources = export_manifest_resources(&config);
        let kinds: Vec<ResourceKind> = resources.iter().map(|r| r.kind()).collect();
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
        assert!(kinds.contains(&ResourceKind::EnvStore));
        assert!(kinds.contains(&ResourceKind::SecretStore));
    }

    #[test]
    fn export_manifest_documents_produces_orchestrator_resources() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("doc-ws", "workspace/doc"))
            .expect("dispatch doc workspace");
        ws.apply(&mut config).expect("apply");

        let docs = export_manifest_documents(&config);
        assert!(!docs.is_empty());
        for doc in &docs {
            assert_eq!(doc.api_version, "orchestrator.dev/v2");
        }
        let doc_kinds: Vec<ResourceKind> = docs.iter().map(|d| d.kind).collect();
        assert!(doc_kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(doc_kinds.contains(&ResourceKind::Workspace));
    }

    #[test]
    fn export_validate_roundtrip_all_kinds() {
        use crate::resource::parse::parse_resources_from_yaml;

        let config = make_config();
        let resources = export_manifest_resources(&config);
        let mut yaml_parts: Vec<String> = Vec::new();
        for r in &resources {
            let yaml = r.to_yaml().expect("serialize resource yaml");
            yaml_parts.push(yaml);
        }
        let combined = yaml_parts.join("---\n");
        let reparsed = parse_resources_from_yaml(&combined).expect("round-trip parse should work");
        for res in &reparsed {
            let dispatched = dispatch_resource(res.clone());
            assert!(
                dispatched.is_ok(),
                "dispatch failed for kind {:?}, spec variant: {:?}\nyaml:\n{}",
                res.kind,
                std::mem::discriminant(&res.spec),
                combined
            );
        }
    }

    #[test]
    fn export_crd_documents_empty_config() {
        let config = make_config();
        let docs = export_crd_documents(&config);
        assert!(docs.is_empty(), "no CRDs or CRs should produce empty docs");
    }

    #[test]
    fn export_crd_documents_skips_builtin_crds() {
        let mut config = make_config();
        config.custom_resource_definitions.insert(
            "builtincrd".to_string(),
            crate::crd::types::CustomResourceDefinition {
                kind: "BuiltinCrd".to_string(),
                plural: "builtincrds".to_string(),
                short_names: vec![],
                group: "core.orchestrator.dev".to_string(),
                versions: vec![crate::crd::types::CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({"type": "object"}),
                    served: true,
                    cel_rules: vec![],
                }],
                hooks: Default::default(),
                scope: Default::default(),
                builtin: true,
            },
        );

        let docs = export_crd_documents(&config);
        assert!(docs.is_empty(), "builtin CRDs should be skipped in export");
    }

    #[test]
    fn export_crd_documents_includes_non_builtin_crd() {
        let mut config = make_config();
        config.custom_resource_definitions.insert(
            "promptlibraries.extensions.orchestrator.dev".to_string(),
            crate::crd::types::CustomResourceDefinition {
                kind: "PromptLibrary".to_string(),
                plural: "promptlibraries".to_string(),
                short_names: vec!["pl".to_string()],
                group: "extensions.orchestrator.dev".to_string(),
                versions: vec![crate::crd::types::CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({"type": "object"}),
                    served: true,
                    cel_rules: vec![],
                }],
                hooks: Default::default(),
                scope: Default::default(),
                builtin: false,
            },
        );

        let docs = export_crd_documents(&config);
        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        assert_eq!(
            doc.get("kind").and_then(|v| v.as_str()),
            Some("CustomResourceDefinition")
        );
    }

    #[test]
    fn export_crd_documents_includes_custom_resource_instances() {
        let mut config = make_config();
        config.custom_resources.insert(
            "PromptLibrary/my-prompts".to_string(),
            crate::crd::types::CustomResource {
                api_version: "extensions.orchestrator.dev/v1".to_string(),
                kind: "PromptLibrary".to_string(),
                metadata: crate::cli_types::ResourceMetadata {
                    name: "my-prompts".to_string(),
                    project: None,
                    labels: None,
                    annotations: None,
                },
                spec: serde_json::json!({"templates": []}),
                generation: 1,
                created_at: "2025-01-01T00:00:00Z".to_string(),
                updated_at: "2025-01-01T00:00:00Z".to_string(),
            },
        );

        let docs = export_crd_documents(&config);
        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        assert_eq!(
            doc.get("kind").and_then(|v| v.as_str()),
            Some("PromptLibrary")
        );
    }

    #[test]
    fn export_manifest_documents_maps_all_kind_variants() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("map-ws", "workspace/map"))
            .expect("dispatch workspace");
        ws.apply(&mut config).expect("apply");
        let ag = dispatch_resource(agent_manifest("map-ag", "cmd")).expect("dispatch agent");
        ag.apply(&mut config).expect("apply");
        let wf = dispatch_resource(workflow_manifest("map-wf")).expect("dispatch workflow");
        wf.apply(&mut config).expect("apply");
        let pr = dispatch_resource(project_manifest("map-pr", "d")).expect("dispatch project");
        pr.apply(&mut config).expect("apply");

        config
            .projects
            .get_mut("default")
            .unwrap()
            .env_stores
            .insert(
                "test-config".to_string(),
                crate::config::EnvStoreConfig {
                    data: [("K".to_string(), "V".to_string())].into(),
                    sensitive: false,
                },
            );
        config
            .projects
            .get_mut("default")
            .unwrap()
            .env_stores
            .insert(
                "test-secrets".to_string(),
                crate::config::EnvStoreConfig {
                    data: [("S".to_string(), "V".to_string())].into(),
                    sensitive: true,
                },
            );

        let docs = export_manifest_documents(&config);
        let kinds: Vec<ResourceKind> = docs.iter().map(|d| d.kind).collect();
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::EnvStore));
        assert!(kinds.contains(&ResourceKind::SecretStore));
    }

    #[test]
    fn export_manifest_resources_skips_empty_project_name() {
        let mut config = make_config();
        config.projects.insert(
            String::new(),
            crate::config::ProjectConfig {
                description: Some("ghost".to_string()),
                workspaces: Default::default(),
                agents: Default::default(),
                workflows: Default::default(),
                step_templates: Default::default(),
                env_stores: Default::default(),
                execution_profiles: Default::default(),
            },
        );

        let resources = export_manifest_resources(&config);
        let project_names: Vec<&str> = resources
            .iter()
            .filter(|r| r.kind() == ResourceKind::Project)
            .map(|r| r.name())
            .collect();
        assert!(
            !project_names.contains(&""),
            "empty project name should be skipped"
        );
    }

    #[test]
    fn export_manifest_resources_preserves_labels_annotations() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "labeled-ws".to_string(),
                project: None,
                labels: Some([("env".to_string(), "prod".to_string())].into()),
                annotations: Some([("team".to_string(), "infra".to_string())].into()),
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/labeled".to_string(),
                qa_targets: vec![],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            }),
        };
        let rr = dispatch_resource(resource).expect("dispatch labeled workspace");
        rr.apply(&mut config).expect("apply");

        let exported = export_manifest_resources(&config);
        let ws = exported.iter().find(|r| r.name() == "labeled-ws");
        assert!(ws.is_some());
        // Verify via get_from
        let loaded = WorkspaceResource::get_from(&config, "labeled-ws")
            .expect("labeled workspace should exist");
        assert_eq!(
            loaded
                .metadata
                .labels
                .as_ref()
                .expect("labels should exist")
                .get("env")
                .expect("env label should exist"),
            "prod"
        );
        assert_eq!(
            loaded
                .metadata
                .annotations
                .as_ref()
                .expect("annotations should exist")
                .get("team")
                .expect("team annotation should exist"),
            "infra"
        );
    }
}
