use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Trait for types that can be projected to/from CRD custom resource specs.
///
/// Implemented by each of the 9 builtin config types to enable round-trip
/// conversion between typed config and `serde_json::Value` spec.
pub trait CrdProjectable: Sized + Serialize + DeserializeOwned {
    /// The CRD kind string for this type (e.g. "Agent", "Workflow").
    fn crd_kind() -> &'static str;

    /// Construct a typed config from a CR spec JSON value.
    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self>;

    /// Convert a typed config to a CR spec JSON value.
    fn to_cr_spec(&self) -> serde_json::Value;
}

// ── Implementations for the 9 builtin config types ───────────────────────────

use crate::cli_types::{
    AgentSpec, EnvStoreSpec, ExecutionProfileSpec, ProjectSpec, RuntimePolicySpec, SecretStoreSpec,
    SourceTaskBindingMatchSpec, SourceTaskBindingSpec, SourceTaskTemplateActionSpec,
    SourceTaskTemplateSkillSpec, SourceTaskTemplateSpec, StepTemplateSpec, TriggerSpec,
    WorkspaceSpec,
};
use crate::config::{
    AgentConfig, EnvStoreConfig, ExecutionProfileConfig, ProjectConfig, ResumeConfig, RunnerConfig,
    SecretStoreConfig, SourceTaskBindingConfig, SourceTaskBindingMatchConfig,
    SourceTaskTemplateActionConfig, SourceTaskTemplateConfig, SourceTaskTemplateSkillConfig,
    StepTemplateConfig, StoreBackendProviderConfig, TriggerConfig, WorkflowConfig,
    WorkflowStoreConfig, WorkspaceConfig,
};
use crate::resource::agent::{agent_config_to_spec, agent_spec_to_config};
use crate::resource::execution_profile::{
    execution_profile_config_to_spec, execution_profile_spec_to_config,
};
use crate::resource::runtime_policy::{runner_config_to_spec, runner_spec_to_config};
use crate::resource::workflow::{workflow_config_to_spec, workflow_spec_to_config};
use crate::resource::workspace::{workspace_config_to_spec, workspace_spec_to_config};
use crate::resource::{trigger_config_to_spec, trigger_spec_to_config};

impl CrdProjectable for AgentConfig {
    fn crd_kind() -> &'static str {
        "Agent"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let agent_spec: AgentSpec = serde_json::from_value(spec.clone())?;
        Ok(agent_spec_to_config(&agent_spec))
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = agent_config_to_spec(self);
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for WorkflowConfig {
    fn crd_kind() -> &'static str {
        "Workflow"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let wf_spec: crate::cli_types::WorkflowSpec = serde_json::from_value(spec.clone())?;
        workflow_spec_to_config(&wf_spec)
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = workflow_config_to_spec(self);
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for WorkspaceConfig {
    fn crd_kind() -> &'static str {
        "Workspace"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let ws_spec: WorkspaceSpec = serde_json::from_value(spec.clone())?;
        Ok(workspace_spec_to_config(&ws_spec))
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = workspace_config_to_spec(self);
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for ProjectConfig {
    fn crd_kind() -> &'static str {
        "Project"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let proj_spec: ProjectSpec = serde_json::from_value(spec.clone())?;
        Ok(ProjectConfig {
            description: proj_spec.description,
            workspaces: Default::default(),
            agents: Default::default(),
            workflows: Default::default(),
            step_templates: Default::default(),
            source_task_templates: Default::default(),
            source_task_bindings: Default::default(),
            env_stores: Default::default(),
            secret_stores: Default::default(),
            execution_profiles: Default::default(),
            triggers: Default::default(),
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = ProjectSpec {
            description: self.description.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

/// Combined type for RuntimePolicy projection (runner + resume + observability).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RuntimePolicyProjection {
    #[serde(default)]
    /// Runner policy configuration.
    pub runner: RunnerConfig,
    #[serde(default)]
    /// Resume policy configuration.
    pub resume: ResumeConfig,
    #[serde(default)]
    /// Observability policy configuration.
    pub observability: crate::config::ObservabilityConfig,
    #[serde(default = "default_attention_inbox_enabled")]
    /// Whether the daemon materializes new Attention Inbox records.
    pub attention_inbox_enabled: bool,
    #[serde(default = "default_handoff_enabled")]
    /// Whether immutable handoff snapshots may be generated.
    pub handoff_enabled: bool,
    #[serde(default)]
    /// Whether resume plans may perform state-changing execution.
    pub mutating_resume_enabled: bool,
    #[serde(default)]
    /// Whether non-idempotent replay may be elevated by an operator.
    pub elevated_resume_enabled: bool,
    #[serde(default = "default_handoff_enabled")]
    /// Whether interactive session reads are enabled.
    pub session_read_enabled: bool,
    #[serde(default)]
    /// Whether interactive session mutations are enabled.
    pub session_control_enabled: bool,
    #[serde(default)]
    /// Whether external adapters may ingest source events.
    pub source_ingest_enabled: bool,
    #[serde(default = "default_action_audit_mode")]
    /// Canonical mutation audit enforcement mode.
    pub action_audit_mode: String,
}

impl Default for RuntimePolicyProjection {
    fn default() -> Self {
        Self {
            runner: RunnerConfig::default(),
            resume: ResumeConfig::default(),
            observability: crate::config::ObservabilityConfig::default(),
            attention_inbox_enabled: true,
            handoff_enabled: true,
            mutating_resume_enabled: false,
            elevated_resume_enabled: false,
            session_read_enabled: true,
            session_control_enabled: false,
            source_ingest_enabled: false,
            action_audit_mode: default_action_audit_mode(),
        }
    }
}

fn default_attention_inbox_enabled() -> bool {
    true
}

fn default_handoff_enabled() -> bool {
    true
}

fn default_action_audit_mode() -> String {
    "compatibility".to_string()
}

impl CrdProjectable for RuntimePolicyProjection {
    fn crd_kind() -> &'static str {
        "RuntimePolicy"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let rp_spec: RuntimePolicySpec = serde_json::from_value(spec.clone())?;
        let observability = rp_spec
            .observability
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(RuntimePolicyProjection {
            runner: runner_spec_to_config(&rp_spec.runner),
            resume: ResumeConfig {
                auto: rp_spec.resume.auto,
            },
            observability,
            attention_inbox_enabled: rp_spec.attention_inbox_enabled,
            handoff_enabled: rp_spec.handoff_enabled,
            mutating_resume_enabled: rp_spec.mutating_resume_enabled,
            elevated_resume_enabled: rp_spec.elevated_resume_enabled,
            session_read_enabled: rp_spec.session_read_enabled,
            session_control_enabled: rp_spec.session_control_enabled,
            source_ingest_enabled: rp_spec.source_ingest_enabled,
            action_audit_mode: rp_spec.action_audit_mode,
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = RuntimePolicySpec {
            runner: runner_config_to_spec(&self.runner),
            resume: crate::cli_types::ResumeSpec {
                auto: self.resume.auto,
            },
            observability: serde_json::to_value(&self.observability).ok(),
            attention_inbox_enabled: self.attention_inbox_enabled,
            handoff_enabled: self.handoff_enabled,
            mutating_resume_enabled: self.mutating_resume_enabled,
            elevated_resume_enabled: self.elevated_resume_enabled,
            session_read_enabled: self.session_read_enabled,
            session_control_enabled: self.session_control_enabled,
            source_ingest_enabled: self.source_ingest_enabled,
            action_audit_mode: self.action_audit_mode.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for StepTemplateConfig {
    fn crd_kind() -> &'static str {
        "StepTemplate"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let st_spec: StepTemplateSpec = serde_json::from_value(spec.clone())?;
        Ok(StepTemplateConfig {
            prompt: st_spec.prompt,
            description: st_spec.description,
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = StepTemplateSpec {
            prompt: self.prompt.clone(),
            description: self.description.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for SourceTaskTemplateConfig {
    fn crd_kind() -> &'static str {
        "SourceTaskTemplate"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let value: SourceTaskTemplateSpec = serde_json::from_value(spec.clone())?;
        Ok(Self {
            skill: SourceTaskTemplateSkillConfig {
                name: value.skill.name,
                invocation: value.skill.invocation,
                args: value.skill.args,
            },
            action: SourceTaskTemplateActionConfig {
                workflow: value.action.workflow,
                workspace: value.action.workspace,
                start: value.action.start,
                initial_vars: value.action.initial_vars,
            },
            goal_template: value.goal_template,
            allowed_variables: value.allowed_variables,
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = SourceTaskTemplateSpec {
            skill: SourceTaskTemplateSkillSpec {
                name: self.skill.name.clone(),
                invocation: self.skill.invocation.clone(),
                args: self.skill.args.clone(),
            },
            action: SourceTaskTemplateActionSpec {
                workflow: self.action.workflow.clone(),
                workspace: self.action.workspace.clone(),
                start: self.action.start,
                initial_vars: self.action.initial_vars.clone(),
            },
            goal_template: self.goal_template.clone(),
            allowed_variables: self.allowed_variables.clone(),
        };
        serde_json::to_value(spec).unwrap_or_default()
    }
}

impl CrdProjectable for SourceTaskBindingConfig {
    fn crd_kind() -> &'static str {
        "SourceTaskBinding"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let value: SourceTaskBindingSpec = serde_json::from_value(spec.clone())?;
        Ok(Self {
            trigger_ref: value.trigger_ref,
            match_rule: SourceTaskBindingMatchConfig {
                event_kind: value.match_rule.event_kind,
                reaction: value.match_rule.reaction,
                target_kind: value.match_rule.target_kind,
                channels: value.match_rule.channels,
                all_channels: value.match_rule.all_channels,
            },
            template_ref: value.template_ref,
            allowed_actor_roles: value.allowed_actor_roles,
            suspend: value.suspend,
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = SourceTaskBindingSpec {
            trigger_ref: self.trigger_ref.clone(),
            match_rule: SourceTaskBindingMatchSpec {
                event_kind: self.match_rule.event_kind.clone(),
                reaction: self.match_rule.reaction.clone(),
                target_kind: self.match_rule.target_kind.clone(),
                channels: self.match_rule.channels.clone(),
                all_channels: self.match_rule.all_channels,
            },
            template_ref: self.template_ref.clone(),
            allowed_actor_roles: self.allowed_actor_roles.clone(),
            suspend: self.suspend,
        };
        serde_json::to_value(spec).unwrap_or_default()
    }
}

impl CrdProjectable for TriggerConfig {
    fn crd_kind() -> &'static str {
        "Trigger"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let value: TriggerSpec = serde_json::from_value(spec.clone())?;
        Ok(trigger_spec_to_config(&value))
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        serde_json::to_value(trigger_config_to_spec(self)).unwrap_or_default()
    }
}

impl CrdProjectable for ExecutionProfileConfig {
    fn crd_kind() -> &'static str {
        "ExecutionProfile"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let profile_spec: ExecutionProfileSpec = serde_json::from_value(spec.clone())?;
        Ok(execution_profile_spec_to_config(&profile_spec))
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = execution_profile_config_to_spec(self);
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for EnvStoreConfig {
    fn crd_kind() -> &'static str {
        "EnvStore"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let es_spec: EnvStoreSpec = serde_json::from_value(spec.clone())?;
        Ok(EnvStoreConfig { data: es_spec.data })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = EnvStoreSpec {
            data: self.data.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for SecretStoreConfig {
    fn crd_kind() -> &'static str {
        "SecretStore"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let ss_spec: SecretStoreSpec = serde_json::from_value(spec.clone())?;
        Ok(SecretStoreConfig { data: ss_spec.data })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = SecretStoreSpec {
            data: self.data.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for WorkflowStoreConfig {
    fn crd_kind() -> &'static str {
        "WorkflowStore"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        Ok(serde_json::from_value(spec.clone())?)
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

impl CrdProjectable for StoreBackendProviderConfig {
    fn crd_kind() -> &'static str {
        "StoreBackendProvider"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        Ok(serde_json::from_value(spec.clone())?)
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_config_round_trip() {
        let config = AgentConfig {
            enabled: true,
            command: "echo {prompt}".to_string(),
            capabilities: vec!["plan".to_string()],
            ..Default::default()
        };
        let spec = config.to_cr_spec();
        let back = AgentConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.command, "echo {prompt}");
        assert!(back.capabilities.contains(&"plan".to_string()));
    }

    #[test]
    fn workspace_config_round_trip() {
        let config = WorkspaceConfig {
            kind: Default::default(),
            root_path: "/test".to_string(),
            qa_targets: vec!["src".to_string()],
            ticket_dir: "tickets".to_string(),
            self_referential: false,
            health_policy: Default::default(),
            artifacts_dir: None,
        };
        let spec = config.to_cr_spec();
        let back = WorkspaceConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.root_path, "/test");
        assert_eq!(back.qa_targets, vec!["src"]);
    }

    #[test]
    fn step_template_config_round_trip() {
        let config = StepTemplateConfig {
            prompt: "Do qa".to_string(),
            description: Some("QA template".to_string()),
        };
        let spec = config.to_cr_spec();
        let back = StepTemplateConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.prompt, "Do qa");
        assert_eq!(back.description, Some("QA template".to_string()));
    }

    #[test]
    fn env_store_config_round_trip() {
        let config = EnvStoreConfig {
            data: [("K".to_string(), "V".to_string())].into(),
        };
        let spec = config.to_cr_spec();
        let back = EnvStoreConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.data.get("K").unwrap(), "V");
    }

    #[test]
    fn secret_store_config_round_trip() {
        let config = SecretStoreConfig {
            data: [("SECRET".to_string(), "val".to_string())].into(),
        };
        let spec = config.to_cr_spec();
        let back = SecretStoreConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.data.get("SECRET").unwrap(), "val");
    }

    #[test]
    fn runtime_policy_projection_round_trip() {
        let config = RuntimePolicyProjection {
            runner: RunnerConfig::default(),
            resume: ResumeConfig { auto: true },
            observability: crate::config::ObservabilityConfig::default(),
            attention_inbox_enabled: true,
            handoff_enabled: true,
            mutating_resume_enabled: true,
            elevated_resume_enabled: false,
            session_read_enabled: true,
            session_control_enabled: false,
            source_ingest_enabled: false,
            action_audit_mode: "enforced".to_string(),
        };
        let spec = config.to_cr_spec();
        let back = RuntimePolicyProjection::from_cr_spec(&spec).expect("should deserialize");
        assert!(back.resume.auto);
        assert_eq!(back.runner.shell, "/bin/bash");
        assert!(back.attention_inbox_enabled);
        assert!(back.handoff_enabled);
        assert!(back.mutating_resume_enabled);
        assert!(back.session_read_enabled);
        assert!(!back.session_control_enabled);
        assert!(!back.source_ingest_enabled);
        assert_eq!(back.action_audit_mode, "enforced");
        assert!(RuntimePolicyProjection::default().attention_inbox_enabled);
    }

    #[test]
    fn project_config_round_trip() {
        let config = ProjectConfig {
            description: Some("test project".to_string()),
            workspaces: Default::default(),
            agents: Default::default(),
            workflows: Default::default(),
            step_templates: Default::default(),
            source_task_templates: Default::default(),
            source_task_bindings: Default::default(),
            env_stores: Default::default(),
            secret_stores: Default::default(),
            execution_profiles: Default::default(),
            triggers: Default::default(),
        };
        let spec = config.to_cr_spec();
        let back = ProjectConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.description, Some("test project".to_string()));
        // Nested maps are not preserved through projection — that's expected
        assert!(back.workspaces.is_empty());
    }

    #[test]
    fn workflow_config_round_trip() {
        use crate::config::{
            LoopMode, StepBehavior, WorkflowFinalizeConfig, WorkflowLoopConfig,
            WorkflowLoopGuardConfig, WorkflowStepConfig,
        };
        let config = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "plan".to_string(),
                    description: Some("Planning step".to_string()),
                    required_capability: Some("plan".to_string()),
                    execution_profile: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    stagger_delay_ms: None,
                    timeout_secs: None,
                    stall_timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                    step_vars: None,
                },
                WorkflowStepConfig {
                    id: "self_test".to_string(),
                    description: None,
                    required_capability: None,
                    execution_profile: None,
                    builtin: Some("self_test".to_string()),
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    stagger_delay_ms: None,
                    timeout_secs: None,
                    stall_timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                    step_vars: None,
                },
            ],
            execution: Default::default(),
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Fixed,
                guard: WorkflowLoopGuardConfig {
                    enabled: true,
                    ..WorkflowLoopGuardConfig::default()
                },
                convergence_expr: None,
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
            stagger_delay_ms: None,
            item_isolation: None,
        };
        let spec = config.to_cr_spec();
        let back = WorkflowConfig::from_cr_spec(&spec).expect("should deserialize workflow");
        assert_eq!(back.steps.len(), 2);

        let plan_step = back
            .steps
            .iter()
            .find(|s| s.id == "plan")
            .expect("plan step");
        assert_eq!(plan_step.required_capability.as_deref(), Some("plan"));
        assert!(plan_step.enabled);

        let builtin_step = back
            .steps
            .iter()
            .find(|s| s.id == "self_test")
            .expect("self_test step");
        assert_eq!(builtin_step.builtin.as_deref(), Some("self_test"));
    }

    #[test]
    fn workflow_config_round_trip_preserves_loop_mode() {
        use crate::config::{
            LoopMode, WorkflowFinalizeConfig, WorkflowLoopConfig, WorkflowLoopGuardConfig,
        };
        let config = WorkflowConfig {
            steps: vec![],
            execution: Default::default(),
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Fixed,
                guard: WorkflowLoopGuardConfig::default(),
                convergence_expr: None,
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
            stagger_delay_ms: None,
            item_isolation: None,
        };
        let spec = config.to_cr_spec();
        let back = WorkflowConfig::from_cr_spec(&spec).expect("should deserialize");
        assert!(matches!(back.loop_policy.mode, LoopMode::Fixed));
    }

    #[test]
    fn from_cr_spec_rejects_malformed_agent_spec() {
        let bad_spec = serde_json::json!({ "capabilities": "not-an-array" });
        // A command-less Agent may now be backed by an explicit driver. Keep this
        // projection test focused on a genuinely malformed field type.
        let result = AgentConfig::from_cr_spec(&bad_spec);
        assert!(
            result.is_err(),
            "should reject a malformed capabilities field"
        );
    }

    #[test]
    fn all_fourteen_projectable_kinds_are_unique() {
        let kinds = [
            AgentConfig::crd_kind(),
            WorkflowConfig::crd_kind(),
            WorkspaceConfig::crd_kind(),
            ProjectConfig::crd_kind(),
            RuntimePolicyProjection::crd_kind(),
            StepTemplateConfig::crd_kind(),
            SourceTaskTemplateConfig::crd_kind(),
            SourceTaskBindingConfig::crd_kind(),
            ExecutionProfileConfig::crd_kind(),
            TriggerConfig::crd_kind(),
            EnvStoreConfig::crd_kind(),
            SecretStoreConfig::crd_kind(),
            WorkflowStoreConfig::crd_kind(),
            StoreBackendProviderConfig::crd_kind(),
        ];
        let mut set = std::collections::HashSet::new();
        for kind in &kinds {
            assert!(set.insert(*kind), "duplicate kind: {}", kind);
        }
        assert_eq!(set.len(), 14);
    }

    #[test]
    fn workflow_store_config_round_trip() {
        let config = WorkflowStoreConfig {
            provider: "redis".to_string(),
            base_path: None,
            schema: Some(serde_json::json!({"type": "object"})),
            retention: crate::config::StoreRetention {
                max_entries: Some(200),
                ttl_days: Some(90),
            },
        };
        let spec = config.to_cr_spec();
        let back = WorkflowStoreConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.provider, "redis");
        assert_eq!(back.retention.max_entries, Some(200));
    }

    #[test]
    fn store_backend_provider_config_round_trip() {
        let config = StoreBackendProviderConfig {
            builtin: false,
            commands: Some(crate::config::StoreBackendCommands {
                get: "redis-cli GET $KEY".to_string(),
                put: "redis-cli SET $KEY $VALUE".to_string(),
                delete: "redis-cli DEL $KEY".to_string(),
                list: "redis-cli KEYS *".to_string(),
                prune: None,
            }),
        };
        let spec = config.to_cr_spec();
        let back = StoreBackendProviderConfig::from_cr_spec(&spec).expect("should deserialize");
        assert!(!back.builtin);
        assert_eq!(
            back.commands.as_ref().map(|c| c.get.as_str()),
            Some("redis-cli GET $KEY")
        );
    }
}
