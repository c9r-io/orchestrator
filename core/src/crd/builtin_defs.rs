use crate::crd::scope::CrdScope;
use crate::crd::types::{CrdHooks, CrdVersion, CustomResourceDefinition};

const BUILTIN_GROUP: &str = "orchestrator.dev";

/// Returns the builtin CRD definitions for the orchestrator's core resource types.
pub fn builtin_crd_definitions() -> Vec<CustomResourceDefinition> {
    vec![
        agent_crd(),
        workflow_crd(),
        workspace_crd(),
        project_crd(),
        runtime_policy_crd(),
        step_template_crd(),
        env_store_crd(),
        secret_store_crd(),
        workflow_store_crd(),
        store_backend_provider_crd(),
    ]
}

fn agent_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "Agent".to_string(),
        plural: "agents".to_string(),
        short_names: vec![],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": { "type": "string" },
                "capabilities": { "type": "array", "items": { "type": "string" } },
                "metadata": { "type": "object" },
                "selection": { "type": "object" },
                "env": { "type": "array" },
                "prompt_delivery": { "type": "string" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Namespaced,
        builtin: true,
    }
}

fn workflow_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "Workflow".to_string(),
        plural: "workflows".to_string(),
        short_names: vec!["wf".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["steps", "loop_policy"],
            "properties": {
                "steps": { "type": "array" },
                "loop_policy": { "type": "object" },
                "finalize": { "type": "object" },
                "dynamic_steps": { "type": "array" },
                "adaptive": { "type": "object" },
                "safety": { "type": "object" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Namespaced,
        builtin: true,
    }
}

fn workspace_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "Workspace".to_string(),
        plural: "workspaces".to_string(),
        short_names: vec!["ws".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["root_path", "ticket_dir"],
            "properties": {
                "root_path": { "type": "string" },
                "qa_targets": { "type": "array", "items": { "type": "string" } },
                "ticket_dir": { "type": "string" },
                "self_referential": { "type": "boolean" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Namespaced,
        builtin: true,
    }
}

fn project_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "Project".to_string(),
        plural: "projects".to_string(),
        short_names: vec![],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "properties": {
                "description": { "type": "string" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Cluster,
        builtin: true,
    }
}

fn runtime_policy_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "RuntimePolicy".to_string(),
        plural: "runtimepolicies".to_string(),
        short_names: vec!["runtime-policy".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["runner", "resume"],
            "properties": {
                "runner": { "type": "object" },
                "resume": { "type": "object" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Singleton,
        builtin: true,
    }
}

fn step_template_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "StepTemplate".to_string(),
        plural: "steptemplates".to_string(),
        short_names: vec!["step-template".to_string(), "step_template".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["prompt"],
            "properties": {
                "prompt": { "type": "string" },
                "description": { "type": "string" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Cluster,
        builtin: true,
    }
}

fn env_store_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "EnvStore".to_string(),
        plural: "envstores".to_string(),
        short_names: vec!["env-store".to_string(), "env_store".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["data"],
            "properties": {
                "data": { "type": "object" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Cluster,
        builtin: true,
    }
}

fn secret_store_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "SecretStore".to_string(),
        plural: "secretstores".to_string(),
        short_names: vec!["secret-store".to_string(), "secret_store".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["data"],
            "properties": {
                "data": { "type": "object" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Cluster,
        builtin: true,
    }
}

fn workflow_store_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "WorkflowStore".to_string(),
        plural: "workflowstores".to_string(),
        short_names: vec!["wfs".to_string(), "workflow-store".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "properties": {
                "provider": { "type": "string" },
                "base_path": { "type": "string" },
                "schema": { "type": "object" },
                "retention": { "type": "object" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Namespaced,
        builtin: true,
    }
}

fn store_backend_provider_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "StoreBackendProvider".to_string(),
        plural: "storebackendproviders".to_string(),
        short_names: vec!["sbp".to_string(), "store-backend-provider".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "properties": {
                "builtin": { "type": "boolean" },
                "commands": { "type": "object" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Cluster,
        builtin: true,
    }
}

fn builtin_version(schema: serde_json::Value) -> CrdVersion {
    CrdVersion {
        name: "v2".to_string(),
        schema,
        served: true,
        cel_rules: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn returns_ten_definitions() {
        let defs = builtin_crd_definitions();
        assert_eq!(defs.len(), 10);
    }

    #[test]
    fn all_kinds_unique() {
        let defs = builtin_crd_definitions();
        let kinds: HashSet<&str> = defs.iter().map(|d| d.kind.as_str()).collect();
        assert_eq!(kinds.len(), 10);
    }

    #[test]
    fn all_plurals_unique() {
        let defs = builtin_crd_definitions();
        let plurals: HashSet<&str> = defs.iter().map(|d| d.plural.as_str()).collect();
        assert_eq!(plurals.len(), 10);
    }

    #[test]
    fn all_are_builtin() {
        let defs = builtin_crd_definitions();
        for def in &defs {
            assert!(def.builtin, "CRD {} should be builtin", def.kind);
        }
    }

    #[test]
    fn all_have_group() {
        let defs = builtin_crd_definitions();
        for def in &defs {
            assert_eq!(def.group, "orchestrator.dev");
        }
    }

    #[test]
    fn scopes_are_correct() {
        let defs = builtin_crd_definitions();
        let map: std::collections::HashMap<&str, CrdScope> =
            defs.iter().map(|d| (d.kind.as_str(), d.scope)).collect();

        assert_eq!(map["Agent"], CrdScope::Namespaced);
        assert_eq!(map["Workflow"], CrdScope::Namespaced);
        assert_eq!(map["Workspace"], CrdScope::Namespaced);
        assert_eq!(map["Project"], CrdScope::Cluster);
        assert_eq!(map["RuntimePolicy"], CrdScope::Singleton);
        assert_eq!(map["StepTemplate"], CrdScope::Cluster);
        assert_eq!(map["EnvStore"], CrdScope::Cluster);
        assert_eq!(map["SecretStore"], CrdScope::Cluster);
        assert_eq!(map["WorkflowStore"], CrdScope::Namespaced);
        assert_eq!(map["StoreBackendProvider"], CrdScope::Cluster);
    }

    #[test]
    fn each_has_at_least_one_served_version() {
        let defs = builtin_crd_definitions();
        for def in &defs {
            assert!(
                def.versions.iter().any(|v| v.served),
                "CRD {} has no served version",
                def.kind
            );
        }
    }

    #[test]
    fn schemas_are_objects() {
        let defs = builtin_crd_definitions();
        for def in &defs {
            for ver in &def.versions {
                assert_eq!(
                    ver.schema.get("type").and_then(|v| v.as_str()),
                    Some("object"),
                    "CRD {} version {} schema must be an object",
                    def.kind,
                    ver.name
                );
            }
        }
    }
}
