use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec};
use crate::config::OrchestratorConfig;

use super::{
    AgentResource, DefaultsResource, EnvStoreResource, ProjectResource, RegisteredResource,
    Resource, RuntimePolicyResource, SecretStoreResource, StepTemplateResource, WorkflowResource,
    WorkspaceResource, API_VERSION,
};

pub fn export_manifest_resources(config: &OrchestratorConfig) -> Vec<RegisteredResource> {
    let mut resources = Vec::new();
    if let Some(runtime_policy) = RuntimePolicyResource::get_from(config, "runtime") {
        resources.push(RegisteredResource::RuntimePolicy(runtime_policy));
    }
    if let Some(defaults) = DefaultsResource::get_from(config, "defaults") {
        resources.push(RegisteredResource::Defaults(defaults));
    }
    for name in config.projects.keys() {
        if name.is_empty() {
            continue;
        }
        if let Some(project) = ProjectResource::get_from(config, name) {
            resources.push(RegisteredResource::Project(project));
        }
    }
    for name in config.workspaces.keys() {
        if let Some(workspace) = WorkspaceResource::get_from(config, name) {
            resources.push(RegisteredResource::Workspace(workspace));
        }
    }
    for name in config.agents.keys() {
        if let Some(agent) = AgentResource::get_from(config, name) {
            resources.push(RegisteredResource::Agent(Box::new(agent)));
        }
    }
    for name in config.workflows.keys() {
        if let Some(workflow) = WorkflowResource::get_from(config, name) {
            resources.push(RegisteredResource::Workflow(workflow));
        }
    }
    for name in config.step_templates.keys() {
        if let Some(step_template) = StepTemplateResource::get_from(config, name) {
            resources.push(RegisteredResource::StepTemplate(step_template));
        }
    }
    for name in config.env_stores.keys() {
        if let Some(env_store) = EnvStoreResource::get_from(config, name) {
            resources.push(RegisteredResource::EnvStore(env_store));
        }
        if let Some(secret_store) = SecretStoreResource::get_from(config, name) {
            resources.push(RegisteredResource::SecretStore(secret_store));
        }
    }
    resources
}

/// Export CRD definitions and custom resource instances as YAML-serializable values.
pub fn export_crd_documents(config: &OrchestratorConfig) -> Vec<serde_yaml::Value> {
    let mut docs = Vec::new();

    // Export CRD definitions first (sorted by kind for deterministic output)
    let mut crd_keys: Vec<_> = config.custom_resource_definitions.keys().collect();
    crd_keys.sort();
    for key in crd_keys {
        if let Some(crd) = config.custom_resource_definitions.get(key) {
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
                },
            };
            // Wrap with `kind: CustomResourceDefinition`
            let mut value = serde_yaml::to_value(&manifest).unwrap_or_default();
            if let serde_yaml::Value::Mapping(ref mut map) = value {
                map.insert(
                    serde_yaml::Value::String("kind".to_string()),
                    serde_yaml::Value::String("CustomResourceDefinition".to_string()),
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
            if let Ok(value) = serde_yaml::to_value(&manifest) {
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
            RegisteredResource::Defaults(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Defaults,
                metadata: item.metadata,
                spec: ResourceSpec::Defaults(item.spec),
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
        ws.apply(&mut config);
        let ag = dispatch_resource(agent_manifest("exp-ag", "cmd")).expect("dispatch export agent");
        ag.apply(&mut config);
        let wf = dispatch_resource(workflow_manifest("exp-wf")).expect("dispatch export workflow");
        wf.apply(&mut config);
        let pr =
            dispatch_resource(project_manifest("exp-pr", "d")).expect("dispatch export project");
        pr.apply(&mut config);

        // Add EnvStore and SecretStore
        config.env_stores.insert(
            "shared-config".to_string(),
            crate::config::EnvStoreConfig {
                data: [("K".to_string(), "V".to_string())].into(),
                sensitive: false,
            },
        );
        config.env_stores.insert(
            "api-keys".to_string(),
            crate::config::EnvStoreConfig {
                data: [("SECRET".to_string(), "val".to_string())].into(),
                sensitive: true,
            },
        );

        let resources = export_manifest_resources(&config);
        let kinds: Vec<ResourceKind> = resources.iter().map(|r| r.kind()).collect();
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::Defaults));
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
        ws.apply(&mut config);

        let docs = export_manifest_documents(&config);
        assert!(!docs.is_empty());
        for doc in &docs {
            assert_eq!(doc.api_version, "orchestrator.dev/v2");
        }
        let doc_kinds: Vec<ResourceKind> = docs.iter().map(|d| d.kind).collect();
        assert!(doc_kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(doc_kinds.contains(&ResourceKind::Defaults));
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
        rr.apply(&mut config);

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
