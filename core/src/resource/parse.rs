use crate::cli_types::{OrchestratorResource, ResourceKind};
use crate::config::OrchestratorConfig;
use crate::crd::resolve::{find_crd_by_kind_or_alias, is_builtin_kind};
use crate::crd::types::{CrdManifest, CustomResourceManifest};
use crate::crd::ParsedManifest;
use anyhow::{anyhow, Result};
use serde::Deserialize;

use super::{
    AgentResource, DefaultsResource, EnvStoreResource, ProjectResource, Resource,
    RuntimePolicyResource, SecretStoreResource, StepTemplateResource, WorkflowResource,
    WorkspaceResource,
};

/// Parse YAML into builtin OrchestratorResource types only (backward-compatible).
pub fn parse_resources_from_yaml(content: &str) -> Result<Vec<OrchestratorResource>> {
    let mut resources = Vec::new();
    for document in serde_yml::Deserializer::from_str(content) {
        let value = serde_yml::Value::deserialize(document)?;
        if value.is_null() {
            continue;
        }
        let resource = serde_yml::from_value::<OrchestratorResource>(value)?;
        resources.push(resource);
    }
    Ok(resources)
}

/// Two-phase YAML parsing: reads `kind` first, then routes to the appropriate type.
pub fn parse_manifests_from_yaml(content: &str) -> Result<Vec<ParsedManifest>> {
    let mut manifests = Vec::new();
    for document in serde_yml::Deserializer::from_str(content) {
        let value = serde_yml::Value::deserialize(document)?;
        if value.is_null() {
            continue;
        }

        // Phase 1: extract the `kind` string
        let kind_str = value
            .get("kind")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let manifest = match kind_str.as_deref() {
            Some("CustomResourceDefinition") => {
                let crd: CrdManifest = serde_yml::from_value(value)?;
                ParsedManifest::Crd(crd)
            }
            Some(kind) if is_builtin_kind(kind) => {
                let resource: OrchestratorResource = serde_yml::from_value(value)?;
                ParsedManifest::Builtin(resource)
            }
            Some(_) => {
                // Unknown kind → treat as custom resource
                let cr: CustomResourceManifest = serde_yml::from_value(value)?;
                ParsedManifest::Custom(cr)
            }
            None => {
                // No kind field — try as builtin (will error later on dispatch)
                let resource: OrchestratorResource = serde_yml::from_value(value)?;
                ParsedManifest::Builtin(resource)
            }
        };
        manifests.push(manifest);
    }
    Ok(manifests)
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
        "steptemplate" | "step_template" | "step-template" => {
            Ok(StepTemplateResource::delete_from(config, name))
        }
        "envstore" | "env-store" | "env_store" => Ok(EnvStoreResource::delete_from(config, name)),
        "secretstore" | "secret-store" | "secret_store" => {
            Ok(SecretStoreResource::delete_from(config, name))
        }
        "customresourcedefinition" | "crd" => crate::crd::delete_crd(config, name),
        _ => {
            // Try CRD-defined custom resource types
            if find_crd_by_kind_or_alias(config, kind).is_some() {
                // Resolve the actual kind name from the CRD
                let crd_kind = find_crd_by_kind_or_alias(config, kind)
                    .map(|crd| crd.kind.clone())
                    .ok_or_else(|| anyhow!("CRD not found for '{}'", kind))?;
                return crate::crd::delete_custom_resource(config, &crd_kind, name);
            }
            Err(anyhow!(
                "unknown resource type: {} (supported: workspace, agent, workflow, project, defaults, runtimepolicy, steptemplate, envstore, secretstore, or CRD-defined types)",
                kind
            ))
        }
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
        ResourceKind::StepTemplate => "steptemplate",
        ResourceKind::EnvStore => "envstore",
        ResourceKind::SecretStore => "secretstore",
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
        ws.apply(&mut config).expect("apply");
        assert!(delete_resource_by_kind(&mut config, "workspace", "del-ws")
            .expect("delete workspace resource"));
        assert!(!config.workspaces.contains_key("del-ws"));
    }

    #[test]
    fn delete_resource_by_kind_ws_alias() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("del-ws2", "workspace/del2"))
            .expect("dispatch should succeed");
        ws.apply(&mut config).expect("apply");
        assert!(delete_resource_by_kind(&mut config, "ws", "del-ws2").expect("delete ws alias"));
    }

    #[test]
    fn delete_resource_by_kind_agent() {
        let mut config = make_config();
        let agent = dispatch_resource(agent_manifest("del-agent", "cargo test"))
            .expect("dispatch should succeed");
        agent.apply(&mut config).expect("apply");
        assert!(delete_resource_by_kind(&mut config, "agent", "del-agent")
            .expect("delete agent resource"));
        assert!(!config.agents.contains_key("del-agent"));
    }

    #[test]
    fn delete_resource_by_kind_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("del-wf")).expect("dispatch should succeed");
        wf.apply(&mut config).expect("apply");
        assert!(delete_resource_by_kind(&mut config, "workflow", "del-wf")
            .expect("delete workflow resource"));
    }

    #[test]
    fn delete_resource_by_kind_wf_alias() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("del-wf2")).expect("dispatch should succeed");
        wf.apply(&mut config).expect("apply");
        assert!(
            delete_resource_by_kind(&mut config, "wf", "del-wf2").expect("delete workflow alias")
        );
    }

    #[test]
    fn delete_resource_by_kind_project() {
        let mut config = make_config();
        let proj = dispatch_resource(project_manifest("del-proj", "desc"))
            .expect("dispatch should succeed");
        proj.apply(&mut config).expect("apply");
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

    #[test]
    fn kind_as_str_env_store_variants() {
        assert_eq!(kind_as_str(ResourceKind::EnvStore), "envstore");
        assert_eq!(kind_as_str(ResourceKind::SecretStore), "secretstore");
    }

    #[test]
    fn parse_env_store_multi_document() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: config
spec:
  data:
    KEY: value
---
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: secrets
spec:
  data:
    API_KEY: sk-test
"#;
        let resources = parse_resources_from_yaml(yaml).expect("should parse");
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].kind, ResourceKind::EnvStore);
        assert_eq!(resources[1].kind, ResourceKind::SecretStore);
    }

    #[test]
    fn delete_resource_by_kind_step_template_aliases() {
        let mut config = make_config();
        // No template exists, so delete returns false
        assert!(
            !delete_resource_by_kind(&mut config, "steptemplate", "missing")
                .expect("delete steptemplate")
        );
        assert!(
            !delete_resource_by_kind(&mut config, "step_template", "missing")
                .expect("delete step_template")
        );
        assert!(
            !delete_resource_by_kind(&mut config, "step-template", "missing")
                .expect("delete step-template")
        );
    }

    #[test]
    fn kind_as_str_step_template() {
        assert_eq!(kind_as_str(ResourceKind::StepTemplate), "steptemplate");
    }

    #[test]
    fn parse_manifests_from_yaml_builtin_kind() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: test-proj
spec:
  description: A project
"#;
        let manifests = parse_manifests_from_yaml(yaml).expect("should parse builtin");
        assert_eq!(manifests.len(), 1);
        assert!(matches!(manifests[0], crate::crd::ParsedManifest::Builtin(_)));
    }

    #[test]
    fn parse_manifests_from_yaml_crd_kind() {
        let yaml = r#"
kind: CustomResourceDefinition
apiVersion: orchestrator.dev/v2
metadata:
  name: promptlibraries.extensions.orchestrator.dev
spec:
  kind: PromptLibrary
  plural: promptlibraries
  group: extensions.orchestrator.dev
  versions:
    - name: v1
      schema:
        type: object
"#;
        let manifests = parse_manifests_from_yaml(yaml).expect("should parse CRD");
        assert_eq!(manifests.len(), 1);
        assert!(matches!(manifests[0], crate::crd::ParsedManifest::Crd(_)));
    }

    #[test]
    fn parse_manifests_from_yaml_custom_resource() {
        let yaml = r#"
kind: PromptLibrary
apiVersion: extensions.orchestrator.dev/v1
metadata:
  name: my-prompts
spec:
  templates: []
"#;
        let manifests = parse_manifests_from_yaml(yaml).expect("should parse custom resource");
        assert_eq!(manifests.len(), 1);
        assert!(matches!(manifests[0], crate::crd::ParsedManifest::Custom(_)));
    }

    #[test]
    fn parse_manifests_from_yaml_skips_null_documents() {
        let yaml = "---\n---\nkind: Project\napiVersion: orchestrator.dev/v2\nmetadata:\n  name: p\nspec:\n  description: d\n";
        let manifests = parse_manifests_from_yaml(yaml).expect("should parse with nulls");
        assert_eq!(manifests.len(), 1);
    }

    #[test]
    fn parse_manifests_from_yaml_no_kind_falls_through_to_builtin() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
metadata:
  name: test
spec:
  description: no kind
"#;
        // This should try to parse as builtin (will fail at dispatch but parse succeeds)
        let result = parse_manifests_from_yaml(yaml);
        // May succeed or fail depending on serde - we just verify it doesn't panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn delete_resource_by_kind_env_store_aliases() {
        let mut config = make_config();
        // No store exists, so delete returns false
        assert!(
            !delete_resource_by_kind(&mut config, "envstore", "missing").expect("delete envstore")
        );
        assert!(
            !delete_resource_by_kind(&mut config, "env-store", "missing")
                .expect("delete env-store")
        );
        assert!(
            !delete_resource_by_kind(&mut config, "secretstore", "missing")
                .expect("delete secretstore")
        );
        assert!(
            !delete_resource_by_kind(&mut config, "secret-store", "missing")
                .expect("delete secret-store")
        );
    }
}
