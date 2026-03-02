use crate::cli_types::{OrchestratorResource, ResourceKind};
use crate::config::OrchestratorConfig;
use anyhow::{anyhow, Result};
use serde::Deserialize;

use super::{
    AgentResource, DefaultsResource, ProjectResource, Resource, RuntimePolicyResource,
    WorkflowResource, WorkspaceResource,
};

pub fn parse_resources_from_yaml(content: &str) -> Result<Vec<OrchestratorResource>> {
    let mut resources = Vec::new();
    for document in serde_yaml::Deserializer::from_str(content) {
        let value = serde_yaml::Value::deserialize(document)?;
        if value.is_null() {
            continue;
        }
        let resource = serde_yaml::from_value::<OrchestratorResource>(value)?;
        resources.push(resource);
    }
    Ok(resources)
}

pub fn delete_resource_by_kind(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
) -> Result<bool> {
    match kind {
        "ws" | "workspace" => Ok(WorkspaceResource::delete_from(config, name)),
        "agent" => Ok(AgentResource::delete_from(config, name)),
        "wf" | "workflow" => Ok(WorkflowResource::delete_from(config, name)),
        "project" => Ok(ProjectResource::delete_from(config, name)),
        "defaults" => Ok(DefaultsResource::delete_from(config, name)),
        "runtimepolicy" | "runtime-policy" => Ok(RuntimePolicyResource::delete_from(config, name)),
        _ => Err(anyhow!(
            "unknown resource type: {} (supported: workspace, agent, workflow, project, defaults, runtimepolicy)",
            kind
        )),
    }
}

pub fn kind_as_str(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Workspace => "workspace",
        ResourceKind::Agent => "agent",
        ResourceKind::Workflow => "workflow",
        ResourceKind::Project => "project",
        ResourceKind::Defaults => "defaults",
        ResourceKind::RuntimePolicy => "runtimepolicy",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::dispatch_resource;

    use super::super::test_fixtures::{
        agent_manifest, make_config, project_manifest, workflow_manifest, workspace_manifest,
    };

    // ── kind_as_str tests ───────────────────────────────────────────

    #[test]
    fn kind_as_str_all_variants() {
        assert_eq!(kind_as_str(ResourceKind::Workspace), "workspace");
        assert_eq!(kind_as_str(ResourceKind::Agent), "agent");
        assert_eq!(kind_as_str(ResourceKind::Workflow), "workflow");
        assert_eq!(kind_as_str(ResourceKind::Project), "project");
        assert_eq!(kind_as_str(ResourceKind::Defaults), "defaults");
        assert_eq!(kind_as_str(ResourceKind::RuntimePolicy), "runtimepolicy");
    }

    // ── parse_resources_from_yaml tests ─────────────────────────────

    #[test]
    fn parse_resources_from_yaml_single_document() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: test-proj
spec:
  description: A project
"#;
        let resources = parse_resources_from_yaml(yaml).expect("should parse");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].kind, ResourceKind::Project);
        assert_eq!(resources[0].metadata.name, "test-proj");
    }

    #[test]
    fn parse_resources_from_yaml_multi_document() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: proj-1
spec:
  description: first
---
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: proj-2
spec:
  description: second
"#;
        let resources = parse_resources_from_yaml(yaml).expect("should parse");
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].metadata.name, "proj-1");
        assert_eq!(resources[1].metadata.name, "proj-2");
    }

    #[test]
    fn parse_resources_from_yaml_skips_null_documents() {
        let yaml = "---\n---\napiVersion: orchestrator.dev/v2\nkind: Project\nmetadata:\n  name: p\nspec:\n  description: d\n";
        let resources = parse_resources_from_yaml(yaml).expect("should parse");
        assert_eq!(resources.len(), 1);
    }

    // ── delete_resource_by_kind tests ────────────────────────────────

    #[test]
    fn delete_resource_by_kind_workspace() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("del-ws", "workspace/del"))
            .expect("dispatch should succeed");
        ws.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "workspace", "del-ws")
            .expect("delete workspace resource"));
        assert!(!config.workspaces.contains_key("del-ws"));
    }

    #[test]
    fn delete_resource_by_kind_ws_alias() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("del-ws2", "workspace/del2"))
            .expect("dispatch should succeed");
        ws.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "ws", "del-ws2").expect("delete ws alias"));
    }

    #[test]
    fn delete_resource_by_kind_agent() {
        let mut config = make_config();
        let agent = dispatch_resource(agent_manifest("del-agent", "cargo test"))
            .expect("dispatch should succeed");
        agent.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "agent", "del-agent")
            .expect("delete agent resource"));
        assert!(!config.agents.contains_key("del-agent"));
    }

    #[test]
    fn delete_resource_by_kind_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("del-wf")).expect("dispatch should succeed");
        wf.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "workflow", "del-wf")
            .expect("delete workflow resource"));
    }

    #[test]
    fn delete_resource_by_kind_wf_alias() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("del-wf2")).expect("dispatch should succeed");
        wf.apply(&mut config);
        assert!(
            delete_resource_by_kind(&mut config, "wf", "del-wf2").expect("delete workflow alias")
        );
    }

    #[test]
    fn delete_resource_by_kind_project() {
        let mut config = make_config();
        let proj = dispatch_resource(project_manifest("del-proj", "desc"))
            .expect("dispatch should succeed");
        proj.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "project", "del-proj")
            .expect("delete project resource"));
    }

    #[test]
    fn delete_resource_by_kind_defaults() {
        let mut config = make_config();
        assert!(
            !delete_resource_by_kind(&mut config, "defaults", "defaults")
                .expect("delete defaults should return false")
        );
    }

    #[test]
    fn delete_resource_by_kind_runtime_policy() {
        let mut config = make_config();
        assert!(
            !delete_resource_by_kind(&mut config, "runtimepolicy", "runtime")
                .expect("delete runtimepolicy should return false")
        );
    }

    #[test]
    fn delete_resource_by_kind_runtime_policy_alias() {
        let mut config = make_config();
        assert!(
            !delete_resource_by_kind(&mut config, "runtime-policy", "runtime")
                .expect("delete runtime-policy alias should return false")
        );
    }

    #[test]
    fn delete_resource_by_kind_rejects_unknown() {
        let mut config = make_config();
        let err =
            delete_resource_by_kind(&mut config, "foobar", "x").expect_err("operation should fail");
        assert!(err.to_string().contains("unknown resource type"));
    }
}
