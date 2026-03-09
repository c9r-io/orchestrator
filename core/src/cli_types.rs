/// K8s-style YAML types for declarative resource management via `apply` command.
/// Resources follow Kubernetes manifest conventions: apiVersion, kind, metadata, spec.
use crate::config::PromptDelivery;
use crate::metrics::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Expected API version for orchestrator resources
const EXPECTED_API_VERSION: &str = "orchestrator.dev/v2";

/// Kubernetes-style resource manifest for declarative configuration.
/// Top-level structure for YAML deserialization in the `apply` command.
///
/// Uses custom `Deserialize` to route `spec` deserialization based on the
/// `kind` field, avoiding ambiguity from `#[serde(untagged)]` on `ResourceSpec`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OrchestratorResource {
    /// API version of this resource (e.g., "orchestrator.dev/v2")
    #[serde(rename = "apiVersion")]
    pub api_version: String,

    /// Resource kind (Workspace, Agent, Workflow, Project, RuntimePolicy)
    pub kind: ResourceKind,

    /// Resource metadata (name, labels, annotations)
    pub metadata: ResourceMetadata,

    /// Resource-specific configuration based on kind
    pub spec: ResourceSpec,
}

impl<'de> Deserialize<'de> for OrchestratorResource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// Helper struct that captures `spec` as raw Value for kind-aware dispatch.
        #[derive(Deserialize)]
        struct RawResource {
            #[serde(rename = "apiVersion")]
            api_version: String,
            kind: ResourceKind,
            metadata: ResourceMetadata,
            spec: serde_yml::Value,
        }

        let raw = RawResource::deserialize(deserializer)?;
        let spec = match raw.kind {
            ResourceKind::Workspace => {
                let s: WorkspaceSpec =
                    serde_yml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Workspace(s)
            }
            ResourceKind::Agent => {
                let s: AgentSpec =
                    serde_yml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Agent(Box::new(s))
            }
            ResourceKind::Workflow => {
                let s: WorkflowSpec =
                    serde_yml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Workflow(s)
            }
            ResourceKind::Project => {
                let s: ProjectSpec =
                    serde_yml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Project(s)
            }
            ResourceKind::RuntimePolicy => {
                let s: RuntimePolicySpec =
                    serde_yml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::RuntimePolicy(s)
            }
            ResourceKind::StepTemplate => {
                let s: StepTemplateSpec =
                    serde_yml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::StepTemplate(s)
            }
            ResourceKind::EnvStore | ResourceKind::SecretStore => {
                let s: EnvStoreSpec =
                    serde_yml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::EnvStore(s)
            }
        };
        Ok(OrchestratorResource {
            api_version: raw.api_version,
            kind: raw.kind,
            metadata: raw.metadata,
            spec,
        })
    }
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
    RuntimePolicy,
    StepTemplate,
    EnvStore,
    SecretStore,
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

    /// Runtime policy resource spec
    RuntimePolicy(RuntimePolicySpec),

    /// Step template resource spec
    StepTemplate(StepTemplateSpec),

    /// Env store / Secret store resource spec (both share the same data shape).
    /// The `ResourceKind` field on `OrchestratorResource` distinguishes them.
    EnvStore(EnvStoreSpec),
}

/// Project resource specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProjectSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Runtime policy specification containing runner + resume + observability behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePolicySpec {
    pub runner: RunnerSpec,
    pub resume: ResumeSpec,
    #[serde(default)]
    pub observability: Option<serde_json::Value>,
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
    #[serde(default = "default_allowed_shells")]
    pub allowed_shells: Vec<String>,
    #[serde(default = "default_allowed_shell_args")]
    pub allowed_shell_args: Vec<String>,
    #[serde(default = "default_env_allowlist")]
    pub env_allowlist: Vec<String>,
    #[serde(default = "default_redaction_patterns")]
    pub redaction_patterns: Vec<String>,
}

fn default_shell_arg() -> String {
    "-lc".to_string()
}

fn default_runner_policy() -> String {
    "allowlist".to_string()
}

fn default_runner_executor() -> String {
    "shell".to_string()
}

fn default_allowed_shells() -> Vec<String> {
    vec![
        "/bin/bash".to_string(),
        "/bin/zsh".to_string(),
        "/bin/sh".to_string(),
    ]
}

fn default_allowed_shell_args() -> Vec<String> {
    vec!["-lc".to_string(), "-c".to_string()]
}

fn default_env_allowlist() -> Vec<String> {
    vec![
        "PATH".to_string(),
        "HOME".to_string(),
        "USER".to_string(),
        "LANG".to_string(),
        "TERM".to_string(),
    ]
}

fn default_redaction_patterns() -> Vec<String> {
    vec![
        "token".to_string(),
        "password".to_string(),
        "secret".to_string(),
        "api_key".to_string(),
        "authorization".to_string(),
    ]
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

/// Step template resource specification.
/// Defines a reusable prompt template for workflow steps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StepTemplateSpec {
    /// The prompt template text (supports {variable} placeholders)
    pub prompt: String,

    /// Optional description of what this template does
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// EnvStore resource specification.
/// Declares reusable environment variable sets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvStoreSpec {
    pub data: HashMap<String, String>,
}

/// A single entry in an Agent's env configuration.
/// Exactly one of the three forms must be used:
/// - `name` + `value`: direct env var
/// - `fromRef`: import all keys from a named store
/// - `name` + `refValue`: import a single key from a named store
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEnvEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "fromRef")]
    pub from_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "refValue")]
    pub ref_value: Option<AgentEnvRefValue>,
}

/// Reference to a specific key within an EnvStore or SecretStore.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEnvRefValue {
    pub name: String,
    pub key: String,
}

/// Agent resource specification.
/// Defines an agent with a command and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSpec {
    /// Command to execute (must contain {prompt} placeholder)
    pub command: String,

    /// Agent capabilities (e.g., plan, implement, qa_testing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,

    /// Agent metadata (cost, description)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AgentMetadataSpec>,

    /// Agent selection strategy and weights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<AgentSelectionSpec>,

    /// Environment variables to inject into the agent process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<AgentEnvEntry>>,

    /// How the rendered prompt is delivered to the agent process.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "promptDelivery"
    )]
    pub prompt_delivery: Option<PromptDelivery>,
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

/// Workflow resource specification.
/// Defines a workflow pipeline with steps, loop policy, and finalization rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
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

    /// Optional adaptive planner configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<crate::dynamic_orchestration::AdaptivePlannerConfig>,

    /// Safety configuration for self-bootstrap scenarios
    #[serde(default)]
    pub safety: SafetySpec,

    /// Default max parallelism for item-scoped segments (1 = sequential)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// WP04: Invariant constraints
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invariants: Vec<crate::config::InvariantConfig>,
    /// WP02: Maximum total spawned tasks
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_spawned_tasks: Option<usize>,
    /// WP02: Maximum spawn depth
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_spawn_depth: Option<usize>,
    /// WP02: Cooldown between spawns in seconds
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_cooldown_seconds: Option<u64>,
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

    /// Reference to a StepTemplate resource name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,

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

    /// Maximum parallel items for item-scoped steps (per-step override)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,

    /// Per-step timeout in seconds (overrides global safety.step_timeout_secs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,

    /// Declarative step behavior (on_failure, captures, post_actions, etc.)
    #[serde(default)]
    pub behavior: crate::config::StepBehavior,

    /// WP03: Configuration for item_select builtin step
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_select_config: Option<crate::config::ItemSelectConfig>,

    /// WP01: Store inputs — read values from workflow stores before step execution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_inputs: Vec<crate::config::StoreInputConfig>,

    /// WP01: Store outputs — write pipeline vars to workflow stores after step execution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_outputs: Vec<crate::config::StoreOutputConfig>,
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

    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_true")]
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
            serde_yml::from_str(yaml).expect("Failed to parse workspace YAML");

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
            serde_yml::from_str(yaml).expect("Failed to parse YAML");

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
            serde_yml::from_str(yaml).expect("Failed to parse workflow YAML");

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
            assert!(
                matches!(&resource.spec, ResourceSpec::Workflow(_)),
                "Expected Workflow spec"
            );
        }
    }

    #[test]
    fn self_test_step_type_validates_correctly() {
        let result = crate::config::validate_step_type("self_test");
        assert!(result.is_ok());
        assert_eq!(
            result.expect("self_test should be a valid step type"),
            "self_test"
        );
    }

    #[test]
    fn parse_env_store_yaml() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: shared-config
spec:
  data:
    DATABASE_URL: "postgres://localhost/mydb"
    LOG_LEVEL: "debug"
"#;
        let resource: OrchestratorResource =
            serde_yml::from_str(yaml).expect("Failed to parse EnvStore YAML");
        resource.validate_version().expect("version ok");
        assert_eq!(resource.kind, ResourceKind::EnvStore);
        assert_eq!(resource.metadata.name, "shared-config");
        if let ResourceSpec::EnvStore(spec) = &resource.spec {
            assert_eq!(
                spec.data.get("DATABASE_URL").unwrap(),
                "postgres://localhost/mydb"
            );
            assert_eq!(spec.data.get("LOG_LEVEL").unwrap(), "debug");
        } else {
            panic!("Expected EnvStore spec");
        }
    }

    #[test]
    fn parse_secret_store_yaml() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: api-keys
spec:
  data:
    OPENAI_API_KEY: "sk-test123"
"#;
        let resource: OrchestratorResource =
            serde_yml::from_str(yaml).expect("Failed to parse SecretStore YAML");
        resource.validate_version().expect("version ok");
        assert_eq!(resource.kind, ResourceKind::SecretStore);
        if let ResourceSpec::EnvStore(spec) = &resource.spec {
            assert_eq!(spec.data.get("OPENAI_API_KEY").unwrap(), "sk-test123");
        } else {
            panic!("Expected EnvStore spec (SecretStore uses same spec shape)");
        }
    }

    #[test]
    fn parse_agent_with_env_yaml() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: coder
spec:
  command: claude -p "{prompt}"
  env:
    - name: LOG_LEVEL
      value: "debug"
    - fromRef: shared-config
    - name: MY_API_KEY
      refValue:
        name: api-keys
        key: OPENAI_API_KEY
"#;
        let resource: OrchestratorResource =
            serde_yml::from_str(yaml).expect("Failed to parse Agent with env YAML");
        resource.validate_version().expect("version ok");
        assert_eq!(resource.kind, ResourceKind::Agent);
        if let ResourceSpec::Agent(spec) = &resource.spec {
            let env = spec.env.as_ref().expect("env should be present");
            assert_eq!(env.len(), 3);

            // Direct value
            assert_eq!(env[0].name.as_deref(), Some("LOG_LEVEL"));
            assert_eq!(env[0].value.as_deref(), Some("debug"));

            // fromRef
            assert_eq!(env[1].from_ref.as_deref(), Some("shared-config"));

            // refValue
            assert_eq!(env[2].name.as_deref(), Some("MY_API_KEY"));
            let rv = env[2]
                .ref_value
                .as_ref()
                .expect("refValue should be present");
            assert_eq!(rv.name, "api-keys");
            assert_eq!(rv.key, "OPENAI_API_KEY");
        } else {
            panic!("Expected Agent spec");
        }
    }

    #[test]
    fn parse_agent_without_env_yaml() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: basic
spec:
  command: echo "{prompt}"
"#;
        let resource: OrchestratorResource =
            serde_yml::from_str(yaml).expect("Failed to parse Agent YAML");
        if let ResourceSpec::Agent(spec) = &resource.spec {
            assert!(spec.env.is_none());
        } else {
            panic!("Expected Agent spec");
        }
    }
}
