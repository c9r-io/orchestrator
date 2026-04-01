/// K8s-style YAML types for declarative resource management via `apply` command.
/// Resources follow Kubernetes manifest conventions: apiVersion, kind, metadata, spec.
use crate::config::PromptDelivery;
use crate::selection::{SelectionStrategy, SelectionWeights};
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
            spec: serde_yaml::Value,
        }

        let raw = RawResource::deserialize(deserializer)?;
        let spec = match raw.kind {
            ResourceKind::Workspace => {
                let s: WorkspaceSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Workspace(s)
            }
            ResourceKind::Agent => {
                let s: AgentSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Agent(Box::new(s))
            }
            ResourceKind::Workflow => {
                let s: WorkflowSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Workflow(s)
            }
            ResourceKind::Project => {
                let s: ProjectSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Project(s)
            }
            ResourceKind::RuntimePolicy => {
                let s: RuntimePolicySpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::RuntimePolicy(s)
            }
            ResourceKind::StepTemplate => {
                let s: StepTemplateSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::StepTemplate(s)
            }
            ResourceKind::ExecutionProfile => {
                let s: ExecutionProfileSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::ExecutionProfile(s)
            }
            ResourceKind::EnvStore => {
                let s: EnvStoreSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::EnvStore(s)
            }
            ResourceKind::SecretStore => {
                let s: SecretStoreSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::SecretStore(s)
            }
            ResourceKind::Trigger => {
                let s: TriggerSpec =
                    serde_yaml::from_value(raw.spec).map_err(serde::de::Error::custom)?;
                ResourceSpec::Trigger(s)
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
    /// Workspace manifest.
    Workspace,
    /// Agent manifest.
    Agent,
    /// Workflow manifest.
    Workflow,
    /// Project manifest.
    Project,
    /// Runtime-policy manifest.
    RuntimePolicy,
    /// Step-template manifest.
    StepTemplate,
    /// Execution-profile manifest.
    ExecutionProfile,
    /// Environment-store manifest.
    EnvStore,
    /// Secret-store manifest.
    SecretStore,
    /// Trigger manifest.
    Trigger,
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

    /// Execution profile resource spec
    ExecutionProfile(ExecutionProfileSpec),

    /// Non-sensitive env store resource spec.
    EnvStore(EnvStoreSpec),

    /// Sensitive secret store resource spec.
    SecretStore(SecretStoreSpec),

    /// Trigger resource spec.
    Trigger(TriggerSpec),
}

/// Project resource specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProjectSpec {
    /// Optional human-readable project description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Runtime policy specification containing runner + resume + observability behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimePolicySpec {
    /// Runner policy applied to spawned commands.
    pub runner: RunnerSpec,
    /// Resume behavior used by long-running tasks.
    pub resume: ResumeSpec,
    /// Optional untyped observability settings forwarded to runtime config.
    #[serde(default)]
    pub observability: Option<serde_json::Value>,
}

/// Runner-policy manifest payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerSpec {
    /// Shell binary used to execute command templates.
    pub shell: String,
    /// Shell flag used to pass a command string.
    #[serde(default = "default_shell_arg")]
    pub shell_arg: String,
    /// Runner policy mode such as `allowlist`.
    #[serde(default = "default_runner_policy")]
    pub policy: String,
    /// Executor implementation kind.
    #[serde(default = "default_runner_executor")]
    pub executor: String,
    /// Allowed shell binaries when allowlist enforcement is enabled.
    #[serde(default = "default_allowed_shells")]
    pub allowed_shells: Vec<String>,
    /// Allowed shell arguments when allowlist enforcement is enabled.
    #[serde(default = "default_allowed_shell_args")]
    pub allowed_shell_args: Vec<String>,
    /// Environment variables propagated into child processes.
    #[serde(default = "default_env_allowlist")]
    pub env_allowlist: Vec<String>,
    /// Case-insensitive substrings used to redact secrets from logs.
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

/// Resume-policy manifest payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeSpec {
    /// Enables automatic resume behavior after restart or interruption.
    pub auto: bool,
}

/// Health policy specification for agent/workspace YAML manifests.
/// All fields are optional; missing fields inherit from the next level
/// (agent → workspace → global defaults).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HealthPolicySpec {
    /// Hours to keep an agent in "diseased" state. 0 disables disease.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disease_duration_hours: Option<u64>,

    /// Consecutive infrastructure failures before marking diseased.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disease_threshold: Option<u32>,

    /// Minimum per-capability success rate while diseased.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_success_threshold: Option<f64>,
}

/// Workspace resource specification.
/// Defines a workspace configuration with root path and QA targets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

    /// Default health policy for agents in this workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_policy: Option<HealthPolicySpec>,
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

/// Execution profile resource specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProfileSpec {
    /// Execution backend mode such as `host` or sandboxed execution.
    #[serde(default = "default_execution_profile_mode")]
    pub mode: String,
    /// Filesystem isolation mode.
    #[serde(default = "default_execution_fs_mode")]
    pub fs_mode: String,
    /// Additional writable paths allowed by the profile.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable_paths: Vec<String>,
    /// Network isolation mode.
    #[serde(default = "default_execution_network_mode")]
    pub network_mode: String,
    /// Explicit outbound network allowlist entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_allowlist: Vec<String>,
    /// Optional memory limit in MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u64>,
    /// Optional CPU time limit in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu_seconds: Option<u64>,
    /// Optional process-count limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_processes: Option<u64>,
    /// Optional open-file-descriptor limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_open_files: Option<u64>,
}

fn default_execution_profile_mode() -> String {
    "host".to_string()
}

fn default_execution_fs_mode() -> String {
    "inherit".to_string()
}

fn default_execution_network_mode() -> String {
    "inherit".to_string()
}

/// EnvStore resource specification.
/// Declares reusable environment variable sets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvStoreSpec {
    /// Key-value environment pairs exposed by the store.
    pub data: HashMap<String, String>,
}

/// SecretStore resource specification.
/// Declares reusable secret variable sets. All values are sensitive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecretStoreSpec {
    /// Key-value secret pairs exposed by the store.
    pub data: HashMap<String, String>,
}

// ── Trigger resource types ──────────────────────────────────────────────────

/// Trigger resource specification.
/// Defines an automatic task creation rule driven by cron schedule or events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TriggerSpec {
    /// Cron-based trigger condition (mutually exclusive with `event`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<TriggerCronSpec>,

    /// Event-based trigger condition (mutually exclusive with `cron`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<TriggerEventSpec>,

    /// Action to take when the trigger fires.
    pub action: TriggerActionSpec,

    /// Concurrency policy for overlapping triggers.
    #[serde(
        default,
        rename = "concurrencyPolicy",
        skip_serializing_if = "ConcurrencyPolicy::is_default"
    )]
    pub concurrency_policy: ConcurrencyPolicy,

    /// Whether the trigger is suspended (paused).
    #[serde(default)]
    pub suspend: bool,

    /// Limits on how many historical tasks to retain.
    #[serde(
        default,
        rename = "historyLimit",
        skip_serializing_if = "Option::is_none"
    )]
    pub history_limit: Option<TriggerHistoryLimit>,

    /// Throttle settings for event-driven triggers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throttle: Option<TriggerThrottleSpec>,
}

/// Cron schedule specification for a Trigger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerCronSpec {
    /// Standard 5-field cron expression (min hour dom month dow).
    pub schedule: String,

    /// IANA timezone name (e.g. "Asia/Shanghai"). Defaults to UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Event-based trigger specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerEventSpec {
    /// Event source type (e.g. "task_completed", "task_failed", "webhook", "filesystem").
    pub source: String,

    /// Optional filter conditions for the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<TriggerEventFilter>,

    /// Webhook-specific authentication configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook: Option<TriggerWebhookSpec>,

    /// Filesystem watcher configuration (required when source = "filesystem").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<TriggerFilesystemSpec>,
}

/// Filesystem watcher specification for filesystem triggers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerFilesystemSpec {
    /// Directories to watch (relative to Workspace root_path).
    pub paths: Vec<String>,

    /// Event types to subscribe to: "create", "modify", "delete".
    /// Defaults to all three if empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,

    /// Debounce window in milliseconds. Defaults to 500.
    #[serde(default = "default_fs_debounce_ms")]
    pub debounce_ms: u64,
}

fn default_fs_debounce_ms() -> u64 {
    500
}

/// Webhook authentication specification for YAML manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerWebhookSpec {
    /// SecretStore reference for signature verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<WebhookSecretRef>,

    /// Custom HTTP header name for the signature.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "signatureHeader"
    )]
    pub signature_header: Option<String>,

    /// CRD kind name for plugin lookup in the webhook request path.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "crdRef")]
    pub crd_ref: Option<String>,
}

/// Reference to a SecretStore for webhook secret resolution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebhookSecretRef {
    /// Name of the SecretStore.
    #[serde(rename = "fromRef")]
    pub from_ref: String,
}

/// Filter conditions for event-based triggers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerEventFilter {
    /// Match events from a specific workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,

    /// CEL expression evaluated against the event context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// Action specification — what happens when a trigger fires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerActionSpec {
    /// Target workflow name to create a task for.
    pub workflow: String,

    /// Target workspace name.
    pub workspace: String,

    /// Optional arguments passed to the created task (e.g. target-file lists).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<HashMap<String, Vec<String>>>,

    /// Whether to start the task immediately after creation. Defaults to true.
    #[serde(default = "default_trigger_action_start")]
    pub start: bool,
}

fn default_trigger_action_start() -> bool {
    true
}

/// Concurrency policy for trigger-created tasks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ConcurrencyPolicy {
    /// Allow concurrent tasks.
    Allow,
    /// Skip trigger if an active task already exists (default).
    #[default]
    Forbid,
    /// Cancel active tasks before creating a new one.
    Replace,
}

impl ConcurrencyPolicy {
    fn is_default(&self) -> bool {
        matches!(self, ConcurrencyPolicy::Forbid)
    }
}

/// History retention limits for trigger-created tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerHistoryLimit {
    /// Number of successful tasks to retain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successful: Option<u32>,

    /// Number of failed tasks to retain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<u32>,
}

/// Throttle configuration for event-driven triggers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerThrottleSpec {
    /// Minimum interval in seconds between trigger firings.
    #[serde(default, rename = "minInterval")]
    pub min_interval: u64,
}

/// A single entry in an Agent's env configuration.
/// Exactly one of the three forms must be used:
/// - `name` + `value`: direct env var
/// - `fromRef`: import all keys from a named store
/// - `name` + `refValue`: import a single key from a named store
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEnvEntry {
    /// Environment variable name for direct values or single-key imports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Literal environment-variable value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Store reference used to import all keys from an EnvStore or SecretStore.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "fromRef")]
    pub from_ref: Option<String>,
    /// Store reference used to import a single key from an EnvStore or SecretStore.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "refValue")]
    pub ref_value: Option<AgentEnvRefValue>,
}

/// Reference to a specific key within an EnvStore or SecretStore.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEnvRefValue {
    /// Name of the source EnvStore or SecretStore.
    pub name: String,
    /// Key within the referenced store.
    pub key: String,
}

/// Agent resource specification.
/// Defines an agent with a command and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSpec {
    /// Command to execute (must contain {prompt} placeholder)
    pub command: String,

    /// Conditional command rules evaluated in order via CEL.
    /// First matching rule's command is used; falls back to `command` if none match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_rules: Vec<crate::config::AgentCommandRule>,

    /// Whether this agent is enabled for scheduling (default: true).
    /// Disabled agents are skipped during task dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

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

    /// Health/disease policy overrides for this agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_policy: Option<HealthPolicySpec>,
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

/// Agent-selection policy embedded in declarative manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSelectionSpec {
    /// Selection strategy used to choose among candidate agents.
    #[serde(default)]
    pub strategy: SelectionStrategy,
    /// Optional scoring weights for adaptive selection.
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
    pub adaptive: Option<crate::adaptive::AdaptivePlannerConfig>,

    /// Safety configuration for self-bootstrap scenarios
    #[serde(default)]
    pub safety: SafetySpec,

    /// Default max parallelism for item-scoped segments (1 = sequential)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,

    /// Delay in ms between successive parallel agent spawns (0 = no delay)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stagger_delay_ms: Option<u64>,

    /// Workflow-level item isolation for item-scoped execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_isolation: Option<crate::config::ItemIsolationConfig>,
}

/// Safety configuration specification for YAML
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SafetySpec {
    /// Maximum consecutive failures before safety actions trigger.
    #[serde(default = "default_max_consecutive_failures")]
    pub max_consecutive_failures: u32,
    /// Enables automatic rollback when configured safety conditions are met.
    #[serde(default)]
    pub auto_rollback: bool,
    /// Checkpoint strategy identifier.
    #[serde(default)]
    pub checkpoint_strategy: String,
    /// Per-step timeout in seconds (default: 1800 = 30 min)
    #[serde(default)]
    pub step_timeout_secs: Option<u64>,
    /// Snapshot the release binary at cycle start for rollback
    #[serde(default)]
    pub binary_snapshot: bool,
    /// Optional named safety profile.
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
    /// FR-035: Per-item per-step consecutive failure threshold before blocking
    #[serde(default = "default_max_item_step_failures")]
    pub max_item_step_failures: u32,
    /// FR-035: Minimum cycle interval in seconds; rapid cycles below this trigger pause
    #[serde(default = "default_min_cycle_interval_secs")]
    pub min_cycle_interval_secs: u64,
    /// Stall auto-kill threshold in seconds (overrides built-in 900s default)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall_timeout_secs: Option<u64>,
    /// FR-052: Maximum seconds to wait for in-flight runs when no heartbeat activity
    #[serde(default = "default_inflight_wait_timeout_secs")]
    pub inflight_wait_timeout_secs: u64,
    /// FR-052: Heartbeat must be within this many seconds to be considered active
    #[serde(default = "default_inflight_heartbeat_grace_secs")]
    pub inflight_heartbeat_grace_secs: u64,
}

impl Default for SafetySpec {
    fn default() -> Self {
        Self {
            max_consecutive_failures: 3,
            auto_rollback: false,
            checkpoint_strategy: String::new(),
            step_timeout_secs: None,
            binary_snapshot: false,
            profile: None,
            invariants: Vec::new(),
            max_spawned_tasks: None,
            max_spawn_depth: None,
            spawn_cooldown_seconds: None,
            max_item_step_failures: 3,
            min_cycle_interval_secs: 60,
            stall_timeout_secs: None,
            inflight_wait_timeout_secs: 300,
            inflight_heartbeat_grace_secs: 60,
        }
    }
}

fn default_max_consecutive_failures() -> u32 {
    3
}

fn default_max_item_step_failures() -> u32 {
    3
}

fn default_inflight_wait_timeout_secs() -> u64 {
    300
}

fn default_inflight_heartbeat_grace_secs() -> u64 {
    60
}

fn default_min_cycle_interval_secs() -> u64 {
    60
}

/// Workflow step specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStepSpec {
    /// Stable step identifier unique within the workflow.
    pub id: String,

    /// Logical step type used for builtin defaults and scheduling semantics.
    #[serde(rename = "type")]
    pub step_type: String,

    /// Capability required when the step is dispatched to an agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,

    /// Reference to a StepTemplate resource name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,

    /// Optional execution profile name for agent steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_profile: Option<String>,

    /// Optional builtin handler name used instead of agent dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,

    /// Whether the step is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Whether the step repeats in later cycles.
    #[serde(default = "default_true")]
    pub repeatable: bool,

    /// Marks this step as a loop guard.
    #[serde(default)]
    pub is_guard: bool,

    /// Optional cost preference used during agent selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_preference: Option<String>,

    /// Optional prehook executed before the step runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<WorkflowPrehookSpec>,

    /// Requests a TTY for interactive commands.
    #[serde(default)]
    pub tty: bool,

    /// Build command for builtin build/test/lint steps
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Serial child steps for chain execution containers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain_steps: Vec<WorkflowStepSpec>,

    /// Execution scope: "task" (once per cycle) or "item" (per QA file)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Maximum parallel items for item-scoped steps (per-step override)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,

    /// Per-step stagger delay override in ms between parallel spawns
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stagger_delay_ms: Option<u64>,

    /// Per-step timeout in seconds (overrides global safety.step_timeout_secs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,

    /// Per-step stall auto-kill threshold in seconds (overrides global safety.stall_timeout_secs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall_timeout_secs: Option<u64>,

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

    /// Step-scoped variable overrides applied as a temporary overlay on pipeline
    /// variables during this step's execution. Does not modify global pipeline state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_vars: Option<std::collections::HashMap<String, String>>,

    /// Captures unknown YAML fields for apply-time warning diagnostics.
    #[serde(flatten, default, skip_serializing)]
    pub extra: std::collections::HashMap<String, serde_yaml::Value>,
}

fn default_true() -> bool {
    true
}

/// Workflow prehook specification for conditional execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowPrehookSpec {
    /// Prehook engine identifier.
    #[serde(default = "default_hook_engine")]
    pub engine: String,
    /// Expression evaluated by the prehook engine.
    pub when: String,

    /// Optional reason surfaced in events or UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Optional untyped UI metadata for the prehook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<serde_json::Value>,

    /// Enables extended prehook decisions beyond boolean run/skip.
    #[serde(default)]
    pub extended: bool,
}

fn default_hook_engine() -> String {
    "cel".to_string()
}

/// Workflow loop policy specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowLoopSpec {
    /// Loop mode such as `once`, `fixed`, or `infinite`.
    #[serde(default)]
    pub mode: String,

    /// Optional maximum number of cycles for fixed/infinite loops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cycles: Option<u32>,

    /// Master switch for loop execution.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Stops execution early when no unresolved items remain.
    #[serde(default = "default_true")]
    pub stop_when_no_unresolved: bool,

    /// Optional agent template used for generated loop steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_template: Option<String>,

    /// Optional CEL convergence expressions evaluated each cycle by the loop guard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence_expr: Option<Vec<ConvergenceExprSpec>>,
}

/// A single convergence expression entry in the CRD spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConvergenceExprSpec {
    /// Expression engine (currently only "cel").
    #[serde(default = "default_cel_engine")]
    pub engine: String,
    /// CEL expression that returns bool — `true` means converged.
    pub when: String,
    /// Human-readable reason logged when expression triggers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn default_cel_engine() -> String {
    "cel".to_string()
}

impl Default for WorkflowLoopSpec {
    fn default() -> Self {
        Self {
            mode: "once".to_string(),
            max_cycles: None,
            enabled: true,
            stop_when_no_unresolved: true,
            agent_template: None,
            convergence_expr: None,
        }
    }
}

/// Workflow finalization rules specification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkflowFinalizeSpec {
    /// Ordered finalization rules evaluated after workflow execution.
    #[serde(default)]
    pub rules: Vec<WorkflowFinalizeRuleSpec>,
}

/// Individual finalization rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowFinalizeRuleSpec {
    /// Stable rule identifier.
    pub id: String,

    /// Rule engine identifier.
    #[serde(default = "default_hook_engine")]
    pub engine: String,

    /// Expression evaluated to activate the rule.
    pub when: String,

    /// Final task status applied when the rule matches.
    pub status: String,

    /// Optional human-readable explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Dynamic step configuration carried by workflow manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DynamicStepSpec {
    /// Stable dynamic-step identifier.
    pub id: String,
    /// Optional operator-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Logical step type emitted at runtime.
    pub step_type: String,
    /// Optional fixed agent identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Optional step-template reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Optional trigger expression controlling activation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Priority used to order dynamic steps.
    #[serde(default)]
    pub priority: i32,
    /// Optional execution cap for the dynamic step.
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
            serde_yaml::from_str(yaml).expect("Failed to parse EnvStore YAML");
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
            serde_yaml::from_str(yaml).expect("Failed to parse SecretStore YAML");
        resource.validate_version().expect("version ok");
        assert_eq!(resource.kind, ResourceKind::SecretStore);
        if let ResourceSpec::SecretStore(spec) = &resource.spec {
            assert_eq!(spec.data.get("OPENAI_API_KEY").unwrap(), "sk-test123");
        } else {
            panic!("Expected SecretStore spec");
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
            serde_yaml::from_str(yaml).expect("Failed to parse Agent with env YAML");
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
            serde_yaml::from_str(yaml).expect("Failed to parse Agent YAML");
        if let ResourceSpec::Agent(spec) = &resource.spec {
            assert!(spec.env.is_none());
        } else {
            panic!("Expected Agent spec");
        }
    }

    #[test]
    fn parse_trigger_webhook_with_crd_ref() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: slack-events
spec:
  event:
    source: webhook
    webhook:
      crdRef: SlackIntegration
      secret:
        fromRef: slack-signing
      signatureHeader: X-Slack-Signature
  action:
    workflow: default
    workspace: default
"#;
        let resource: OrchestratorResource =
            serde_yaml::from_str(yaml).expect("Failed to parse Trigger YAML with crdRef");
        assert_eq!(resource.kind, ResourceKind::Trigger);
        if let ResourceSpec::Trigger(spec) = &resource.spec {
            let webhook = spec.event.as_ref().unwrap().webhook.as_ref().unwrap();
            assert_eq!(webhook.crd_ref.as_deref(), Some("SlackIntegration"));
            assert_eq!(webhook.signature_header.as_deref(), Some("X-Slack-Signature"));
        } else {
            panic!("Expected Trigger spec");
        }
    }
}
