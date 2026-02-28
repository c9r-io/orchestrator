/// K8s-style YAML types for declarative resource management via `apply` command.
/// Resources follow Kubernetes manifest conventions: apiVersion, kind, metadata, spec.
use crate::metrics::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Expected API version for orchestrator resources
const EXPECTED_API_VERSION: &str = "orchestrator.dev/v2";

/// Kubernetes-style resource manifest for declarative configuration.
/// Top-level structure for YAML deserialization in the `apply` command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrchestratorResource {
    /// API version of this resource (e.g., "orchestrator.dev/v2")
    #[serde(rename = "apiVersion")]
    pub api_version: String,

    /// Resource kind (Workspace, Agent, Workflow, Project, Defaults, RuntimePolicy)
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
/// Defines all resource types supported by the orchestrator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum ResourceKind {
    Workspace,
    Agent,
    Workflow,
    Project,
    Defaults,
    RuntimePolicy,
}

/// Kubernetes-style resource metadata.
/// Identifies and describes the resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceMetadata {
    /// Name of the resource (e.g., "default", "qa-agent")
    pub name: String,

    /// Optional project namespace identifier for scoped resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,

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
    Agent(Box<AgentSpec>),

    /// Workflow resource spec
    Workflow(WorkflowSpec),

    /// Project resource spec
    Project(ProjectSpec),

    /// Defaults resource spec
    Defaults(DefaultsSpec),

    /// Runtime policy resource spec
    RuntimePolicy(RuntimePolicySpec),
}

/// Project resource specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Defaults resource specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefaultsSpec {
    pub project: String,
    pub workspace: String,
    pub workflow: String,
}

/// Runtime policy specification containing runner + resume behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePolicySpec {
    pub runner: RunnerSpec,
    pub resume: ResumeSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerSpec {
    pub shell: String,
    #[serde(default = "default_shell_arg")]
    pub shell_arg: String,
    #[serde(default = "default_runner_policy")]
    pub policy: String,
    #[serde(default = "default_runner_executor")]
    pub executor: String,
    #[serde(default)]
    pub allowed_shells: Vec<String>,
    #[serde(default)]
    pub allowed_shell_args: Vec<String>,
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    #[serde(default)]
    pub redaction_patterns: Vec<String>,
}

fn default_shell_arg() -> String {
    "-lc".to_string()
}

fn default_runner_policy() -> String {
    "legacy".to_string()
}

fn default_runner_executor() -> String {
    "shell".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeSpec {
    pub auto: bool,
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

    /// When true, the workspace points to the orchestrator's own source tree
    #[serde(default)]
    pub self_referential: bool,
}

/// Agent resource specification.
/// Defines an agent with command templates for workflow phases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSpec {
    /// Templates for each workflow phase
    pub templates: AgentTemplatesSpec,

    /// Agent capabilities (e.g., qa, fix, retest)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,

    /// Agent metadata (cost, description)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AgentMetadataSpec>,

    /// Agent selection strategy and weights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<AgentSelectionSpec>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSelectionSpec {
    #[serde(default)]
    pub strategy: SelectionStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<SelectionWeights>,
}

/// Agent command templates for different workflow phases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTemplatesSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_once: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qa: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retest: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_guard: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_scan: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implement: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ops: Option<String>,

    /// Extra templates for custom/SDLC step types (qa_doc_gen, qa_testing, ticket_fix, etc.)
    #[serde(
        flatten,
        default,
        skip_serializing_if = "std::collections::HashMap::is_empty"
    )]
    pub extra: std::collections::HashMap<String, String>,
}

/// Workflow resource specification.
/// Defines a workflow pipeline with steps, loop policy, and finalization rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

    /// Dynamic runtime steps pool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_steps: Vec<DynamicStepSpec>,

    /// Safety configuration for self-bootstrap scenarios
    #[serde(default)]
    pub safety: SafetySpec,
}

/// Safety configuration specification for YAML
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SafetySpec {
    #[serde(default = "default_max_consecutive_failures")]
    pub max_consecutive_failures: u32,
    #[serde(default)]
    pub auto_rollback: bool,
    #[serde(default)]
    pub checkpoint_strategy: String,
    /// Per-step timeout in seconds (default: 1800 = 30 min)
    #[serde(default)]
    pub step_timeout_secs: Option<u64>,
    /// Snapshot the release binary at cycle start for rollback
    #[serde(default)]
    pub binary_snapshot: bool,
}

fn default_max_consecutive_failures() -> u32 {
    3
}

/// Workflow step specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStepSpec {
    pub id: String,

    #[serde(rename = "type")]
    pub step_type: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_true")]
    pub repeatable: bool,

    #[serde(default)]
    pub is_guard: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_preference: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<WorkflowPrehookSpec>,

    #[serde(default)]
    pub tty: bool,

    /// Build command for builtin build/test/lint steps
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Execution scope: "task" (once per cycle) or "item" (per QA file)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Workflow prehook specification for conditional execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowPrehookSpec {
    #[serde(default = "default_hook_engine")]
    pub engine: String,
    pub when: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<serde_json::Value>,

    #[serde(default)]
    pub extended: bool,
}

fn default_hook_engine() -> String {
    "cel".to_string()
}

/// Workflow loop policy specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowLoopSpec {
    #[serde(default)]
    pub mode: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cycles: Option<u32>,

    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub stop_when_no_unresolved: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_template: Option<String>,
}

impl Default for WorkflowLoopSpec {
    fn default() -> Self {
        Self {
            mode: "once".to_string(),
            max_cycles: None,
            enabled: true,
            stop_when_no_unresolved: true,
            agent_template: None,
        }
    }
}

/// Workflow finalization rules specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkflowFinalizeSpec {
    #[serde(default)]
    pub rules: Vec<WorkflowFinalizeRuleSpec>,
}

/// Individual finalization rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowFinalizeRuleSpec {
    pub id: String,

    #[serde(default = "default_hook_engine")]
    pub engine: String,

    pub when: String,

    pub status: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Dynamic step configuration carried by workflow manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DynamicStepSpec {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub step_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runs: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_workspace_yaml_v2() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
  project: default
spec:
  root_path: /home/user/project
  qa_targets:
    - docs/qa/
  ticket_dir: docs/ticket/
"#;

        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse workspace YAML");

        resource
            .validate_version()
            .expect("Version validation failed");
        assert_eq!(resource.api_version, "orchestrator.dev/v2");
        assert_eq!(resource.kind, ResourceKind::Workspace);
        assert_eq!(resource.metadata.project.as_deref(), Some("default"));
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
            assert!(msg.contains("orchestrator.dev/v2"));
        }
    }

    #[test]
    fn parse_workflow_yaml_with_self_test_step() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: test-workflow
spec:
  steps:
    - id: implement
      type: implement
      required_capability: implement
      enabled: true
      repeatable: false
    - id: self_test
      type: self_test
      enabled: true
      repeatable: false
    - id: qa_testing
      type: qa_testing
      required_capability: qa_testing
      enabled: true
      repeatable: true
  loop:
    mode: once
  safety:
    checkpoint_strategy: git_tag
"#;

        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse workflow YAML");

        resource
            .validate_version()
            .expect("Version validation failed");
        assert_eq!(resource.api_version, "orchestrator.dev/v2");
        assert_eq!(resource.kind, ResourceKind::Workflow);

        if let ResourceSpec::Workflow(workflow_spec) = &resource.spec {
            let step_ids: Vec<&str> = workflow_spec.steps.iter().map(|s| s.id.as_str()).collect();
            assert!(
                step_ids.contains(&"implement"),
                "should have implement step"
            );
            assert!(
                step_ids.contains(&"self_test"),
                "should have self_test step"
            );
            assert!(
                step_ids.contains(&"qa_testing"),
                "should have qa_testing step"
            );

            let self_test_step = workflow_spec
                .steps
                .iter()
                .find(|s| s.id == "self_test")
                .expect("self_test step should exist");
            assert_eq!(self_test_step.step_type.as_str(), "self_test");
        } else {
            panic!("Expected Workflow spec");
        }
    }

    #[test]
    fn self_test_step_type_validates_correctly() {
        let result = crate::config::validate_step_type("self_test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "self_test");
    }
}
