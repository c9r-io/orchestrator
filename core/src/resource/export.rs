use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec};
use crate::config::OrchestratorConfig;

use super::{
    AgentResource, DefaultsResource, ProjectResource, RegisteredResource, Resource,
    RuntimePolicyResource, WorkflowResource, WorkspaceResource, API_VERSION,
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
    resources
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
        let ws = dispatch_resource(workspace_manifest("exp-ws", "workspace/exp")).unwrap();
        ws.apply(&mut config);
        let ag = dispatch_resource(agent_manifest("exp-ag", "cmd")).unwrap();
        ag.apply(&mut config);
        let wf = dispatch_resource(workflow_manifest("exp-wf")).unwrap();
        wf.apply(&mut config);
        let pr = dispatch_resource(project_manifest("exp-pr", "d")).unwrap();
        pr.apply(&mut config);

        let resources = export_manifest_resources(&config);
        let kinds: Vec<ResourceKind> = resources.iter().map(|r| r.kind()).collect();
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::Defaults));
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
    }

    #[test]
    fn export_manifest_documents_produces_orchestrator_resources() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("doc-ws", "workspace/doc")).unwrap();
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
            let yaml = r.to_yaml().unwrap();
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
        let rr = dispatch_resource(resource).unwrap();
        rr.apply(&mut config);

        let exported = export_manifest_resources(&config);
        let ws = exported.iter().find(|r| r.name() == "labeled-ws");
        assert!(ws.is_some());
        // Verify via get_from
        let loaded = WorkspaceResource::get_from(&config, "labeled-ws").unwrap();
        assert_eq!(
            loaded.metadata.labels.as_ref().unwrap().get("env").unwrap(),
            "prod"
        );
        assert_eq!(
            loaded
                .metadata
                .annotations
                .as_ref()
                .unwrap()
                .get("team")
                .unwrap(),
            "infra"
        );
    }
}
