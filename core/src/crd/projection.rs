use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;

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
    AgentSpec, DefaultsSpec, EnvStoreSpec, ProjectSpec, RuntimePolicySpec, StepTemplateSpec,
    WorkspaceSpec,
};
use crate::config::{
    AgentConfig, ConfigDefaults, EnvStoreConfig, ProjectConfig, ResumeConfig, RunnerConfig,
    StepTemplateConfig, StoreBackendProviderConfig, WorkflowConfig, WorkflowStoreConfig,
    WorkspaceConfig,
};
use crate::resource::agent::{agent_config_to_spec, agent_spec_to_config};
use crate::resource::runtime_policy::{runner_config_to_spec, runner_spec_to_config};
use crate::resource::workflow::{workflow_config_to_spec, workflow_spec_to_config};
use crate::resource::workspace::{workspace_config_to_spec, workspace_spec_to_config};

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
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = ProjectSpec {
            description: self.description.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

impl CrdProjectable for ConfigDefaults {
    fn crd_kind() -> &'static str {
        "Defaults"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let def_spec: DefaultsSpec = serde_json::from_value(spec.clone())?;
        Ok(ConfigDefaults {
            project: def_spec.project,
            workspace: def_spec.workspace,
            workflow: def_spec.workflow,
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = DefaultsSpec {
            project: self.project.clone(),
            workspace: self.workspace.clone(),
            workflow: self.workflow.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

/// Combined type for RuntimePolicy projection (runner + resume).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RuntimePolicyProjection {
    pub runner: RunnerConfig,
    pub resume: ResumeConfig,
}

impl CrdProjectable for RuntimePolicyProjection {
    fn crd_kind() -> &'static str {
        "RuntimePolicy"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let rp_spec: RuntimePolicySpec = serde_json::from_value(spec.clone())?;
        Ok(RuntimePolicyProjection {
            runner: runner_spec_to_config(&rp_spec.runner),
            resume: ResumeConfig {
                auto: rp_spec.resume.auto,
            },
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = RuntimePolicySpec {
            runner: runner_config_to_spec(&self.runner),
            resume: crate::cli_types::ResumeSpec {
                auto: self.resume.auto,
            },
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

impl CrdProjectable for EnvStoreConfig {
    fn crd_kind() -> &'static str {
        "EnvStore"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let es_spec: EnvStoreSpec = serde_json::from_value(spec.clone())?;
        Ok(EnvStoreConfig {
            data: es_spec.data,
            sensitive: false,
        })
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = EnvStoreSpec {
            data: self.data.clone(),
        };
        serde_json::to_value(&spec).unwrap_or_default()
    }
}

/// Wrapper type for SecretStore projection (EnvStoreConfig with sensitive=true).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SecretStoreProjection(pub EnvStoreConfig);

impl CrdProjectable for SecretStoreProjection {
    fn crd_kind() -> &'static str {
        "SecretStore"
    }

    fn from_cr_spec(spec: &serde_json::Value) -> Result<Self> {
        let es_spec: EnvStoreSpec = serde_json::from_value(spec.clone())?;
        Ok(SecretStoreProjection(EnvStoreConfig {
            data: es_spec.data,
            sensitive: true,
        }))
    }

    fn to_cr_spec(&self) -> serde_json::Value {
        let spec = EnvStoreSpec {
            data: self.0.data.clone(),
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
            root_path: "/test".to_string(),
            qa_targets: vec!["src".to_string()],
            ticket_dir: "tickets".to_string(),
            self_referential: false,
        };
        let spec = config.to_cr_spec();
        let back = WorkspaceConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.root_path, "/test");
        assert_eq!(back.qa_targets, vec!["src"]);
    }

    #[test]
    fn defaults_config_round_trip() {
        let config = ConfigDefaults {
            project: "proj".to_string(),
            workspace: "ws".to_string(),
            workflow: "wf".to_string(),
        };
        let spec = config.to_cr_spec();
        let back = ConfigDefaults::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.project, "proj");
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
            sensitive: false,
        };
        let spec = config.to_cr_spec();
        let back = EnvStoreConfig::from_cr_spec(&spec).expect("should deserialize");
        assert_eq!(back.data.get("K").unwrap(), "V");
        assert!(!back.sensitive);
    }

    #[test]
    fn secret_store_projection_round_trip() {
        let config = SecretStoreProjection(EnvStoreConfig {
            data: [("SECRET".to_string(), "val".to_string())].into(),
            sensitive: true,
        });
        let spec = config.to_cr_spec();
        let back = SecretStoreProjection::from_cr_spec(&spec).expect("should deserialize");
        assert!(back.0.sensitive);
        assert_eq!(back.0.data.get("SECRET").unwrap(), "val");
    }

    #[test]
    fn runtime_policy_projection_round_trip() {
        let config = RuntimePolicyProjection {
            runner: RunnerConfig::default(),
            resume: ResumeConfig { auto: true },
        };
        let spec = config.to_cr_spec();
        let back = RuntimePolicyProjection::from_cr_spec(&spec).expect("should deserialize");
        assert!(back.resume.auto);
        assert_eq!(back.runner.shell, "/bin/bash");
    }

    #[test]
    fn project_config_round_trip() {
        let config = ProjectConfig {
            description: Some("test project".to_string()),
            workspaces: Default::default(),
            agents: Default::default(),
            workflows: Default::default(),
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
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                WorkflowStepConfig {
                    id: "self_test".to_string(),
                    description: None,
                    required_capability: None,
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
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Fixed,
                guard: WorkflowLoopGuardConfig {
                    enabled: true,
                    ..WorkflowLoopGuardConfig::default()
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
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
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Fixed,
                guard: WorkflowLoopGuardConfig::default(),
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };
        let spec = config.to_cr_spec();
        let back = WorkflowConfig::from_cr_spec(&spec).expect("should deserialize");
        assert!(matches!(back.loop_policy.mode, LoopMode::Fixed));
    }

    #[test]
    fn from_cr_spec_rejects_malformed_agent_spec() {
        let bad_spec = serde_json::json!({ "not_a_valid_field": 42 });
        // AgentSpec requires "command" field — absence should cause deserialization error
        let result = AgentConfig::from_cr_spec(&bad_spec);
        assert!(
            result.is_err(),
            "should reject spec missing required 'command' field"
        );
    }

    #[test]
    fn all_eleven_kinds_are_unique() {
        let kinds = [
            AgentConfig::crd_kind(),
            WorkflowConfig::crd_kind(),
            WorkspaceConfig::crd_kind(),
            ProjectConfig::crd_kind(),
            ConfigDefaults::crd_kind(),
            RuntimePolicyProjection::crd_kind(),
            StepTemplateConfig::crd_kind(),
            EnvStoreConfig::crd_kind(),
            SecretStoreProjection::crd_kind(),
            WorkflowStoreConfig::crd_kind(),
            StoreBackendProviderConfig::crd_kind(),
        ];
        let mut set = std::collections::HashSet::new();
        for kind in &kinds {
            assert!(set.insert(*kind), "duplicate kind: {}", kind);
        }
        assert_eq!(set.len(), 11);
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
