/// K8s-style YAML types for declarative resource management via `apply` command.
/// Resources follow Kubernetes manifest conventions: apiVersion, kind, metadata, spec.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Expected API version for orchestrator resources
const EXPECTED_API_VERSION: &str = "orchestrator.dev/v1";

/// Kubernetes-style resource manifest for declarative configuration.
/// Top-level structure for YAML deserialization in the `apply` command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorResource {
    /// API version of this resource (e.g., "orchestrator.dev/v1")
    #[serde(rename = "apiVersion")]
    pub api_version: String,

    /// Resource kind (Workspace, Agent, Workflow)
    pub kind: ResourceKind,

    /// Resource metadata (name, labels, annotations)
    pub metadata: ResourceMetadata,

    /// Resource-specific configuration based on kind
    pub spec: ResourceSpec,
}

impl OrchestratorResource {
    /// Validates that the apiVersion matches the expected version.
    /// Returns an error if the version is invalid.
    pub fn validate_version(&self) -> Result<(), String> {
        if self.api_version != EXPECTED_API_VERSION {
            return Err(format!(
                "Invalid apiVersion: '{}'. Expected '{}'",
                self.api_version, EXPECTED_API_VERSION
            ));
        }
        Ok(())
    }
}

/// Kubernetes-style resource kind enum.
/// Defines the four resource types supported by the orchestrator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum ResourceKind {
    Workspace,
    Agent,
    Workflow,
}

/// Kubernetes-style resource metadata.
/// Identifies and describes the resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceMetadata {
    /// Name of the resource (e.g., "default", "qa-agent")
    pub name: String,

    /// Optional labels for categorization and selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Optional annotations for arbitrary metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
}

/// Resource specification (configuration) - kind-specific.
/// Each variant holds the configuration for its resource type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ResourceSpec {
    /// Workspace resource spec
    Workspace(WorkspaceSpec),

    /// Agent resource spec
    Agent(AgentSpec),

    /// Workflow resource spec
    Workflow(WorkflowSpec),
}

/// Workspace resource specification.
/// Defines a workspace configuration with root path and QA targets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSpec {
    /// Root path of the workspace
    pub root_path: String,

    /// QA target paths or patterns
    #[serde(default)]
    pub qa_targets: Vec<String>,

    /// Directory for ticket files
    pub ticket_dir: String,
}

/// Agent resource specification.
/// Defines an agent with command templates for workflow phases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSpec {
    /// Templates for each workflow phase
    pub templates: AgentTemplatesSpec,

    /// Agent capabilities (e.g., qa, fix, retest)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,

    /// Agent metadata (cost, description)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AgentMetadataSpec>,
}

/// Agent metadata specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentMetadataSpec {
    /// Agent cost tier (0-255)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<u8>,

    /// Agent description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Agent command templates for different workflow phases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTemplatesSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_once: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qa: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retest: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_guard: Option<String>,
}

/// Workflow resource specification.
/// Defines a workflow pipeline with steps, loop policy, and finalization rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowSpec {
    /// Workflow execution steps
    #[serde(default)]
    pub steps: Vec<WorkflowStepSpec>,

    /// Loop policy (once or infinite)
    #[serde(default, rename = "loop")]
    pub loop_policy: WorkflowLoopSpec,

    /// Finalization rules for determining final step status
    #[serde(default)]
    pub finalize: WorkflowFinalizeSpec,
}

/// Workflow step specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowStepSpec {
    pub id: String,

    #[serde(rename = "type")]
    pub step_type: String,

    pub enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<WorkflowPrehookSpec>,
}

/// Workflow prehook specification for conditional execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowPrehookSpec {
    pub when: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Workflow loop policy specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowLoopSpec {
    #[serde(default)]
    pub mode: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cycles: Option<u32>,
}

impl Default for WorkflowLoopSpec {
    fn default() -> Self {
        Self {
            mode: "once".to_string(),
            max_cycles: None,
        }
    }
}

/// Workflow finalization rules specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkflowFinalizeSpec {
    #[serde(default)]
    pub rules: Vec<WorkflowFinalizeRuleSpec>,
}

/// Individual finalization rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowFinalizeRuleSpec {
    pub id: String,

    pub when: String,

    pub status: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_workspace_yaml() {
        let yaml = r#"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: default
  labels:
    env: test
spec:
  root_path: /home/user/project
  qa_targets:
    - docs/qa/
    - tests/
  ticket_dir: docs/ticket/
"#;

        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse workspace YAML");

        resource
            .validate_version()
            .expect("Version validation failed");
        assert_eq!(resource.api_version, "orchestrator.dev/v1");
        assert_eq!(resource.kind, ResourceKind::Workspace);
        assert_eq!(resource.metadata.name, "default");

        if let ResourceSpec::Workspace(spec) = &resource.spec {
            assert_eq!(spec.root_path, "/home/user/project");
            assert_eq!(spec.qa_targets, vec!["docs/qa/", "tests/"]);
            assert_eq!(spec.ticket_dir, "docs/ticket/");
        } else {
            panic!("Expected WorkspaceSpec");
        }
    }

    #[test]
    fn parse_agent_yaml() {
        let yaml = r#"
apiVersion: orchestrator.dev/v1
kind: Agent
metadata:
  name: default-agent
  labels:
    role: default
spec:
  templates:
    qa: "npm test"
    fix: "npm run lint --fix"
    retest: "npm test"
"#;

        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse agent YAML");

        resource
            .validate_version()
            .expect("Version validation failed");
        assert_eq!(resource.kind, ResourceKind::Agent);
        assert_eq!(resource.metadata.name, "default-agent");

        if let ResourceSpec::Agent(spec) = &resource.spec {
            assert_eq!(spec.templates.qa, Some("npm test".to_string()));
            assert_eq!(spec.templates.fix, Some("npm run lint --fix".to_string()));
        } else {
            panic!("Expected AgentSpec");
        }
    }

    #[test]
    fn parse_workflow_yaml() {
        let yaml = r#"
apiVersion: orchestrator.dev/v1
kind: Workflow
metadata:
  name: default-workflow
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
  loop:
    mode: once
  finalize:
    rules: []
"#;

        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse workflow YAML");

        resource
            .validate_version()
            .expect("Version validation failed");
        assert_eq!(resource.kind, ResourceKind::Workflow);

        if let ResourceSpec::Workflow(spec) = &resource.spec {
            assert_eq!(spec.steps.len(), 1);
            assert_eq!(spec.steps[0].id, "qa");
            assert_eq!(spec.loop_policy.mode, "once");
        } else {
            panic!("Expected WorkflowSpec");
        }
    }

    #[test]
    fn invalid_apiversion() {
        let yaml = r#"
apiVersion: wrong.version/v2
kind: Workspace
metadata:
  name: invalid
spec:
  root_path: /tmp
  qa_targets: []
  ticket_dir: /tmp/tickets
"#;

        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse YAML");

        let result = resource.validate_version();
        assert!(result.is_err());

        if let Err(msg) = result {
            assert!(msg.contains("wrong.version/v2"));
            assert!(msg.contains("orchestrator.dev/v1"));
        }
    }

    #[test]
    fn resource_with_annotations() {
        let yaml = r#"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: test-ws
  annotations:
    description: "Test workspace"
    owner: "devteam"
spec:
  root_path: /test
  qa_targets: []
  ticket_dir: /test/tickets
"#;

        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse YAML with annotations");

        assert!(resource.metadata.annotations.is_some());
        let annotations = resource.metadata.annotations.unwrap();
        assert_eq!(annotations.get("owner"), Some(&"devteam".to_string()));
    }
}
