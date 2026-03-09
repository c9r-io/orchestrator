use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, WorkflowSpec};
use crate::config::{LoopMode, OrchestratorConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

mod workflow_convert;

use workflow_convert::parse_loop_mode;
pub(crate) use workflow_convert::workflow_config_to_spec;
pub(crate) use workflow_convert::workflow_spec_to_config;

#[derive(Debug, Clone)]
pub struct WorkflowResource {
    pub metadata: ResourceMetadata,
    pub spec: WorkflowSpec,
}

impl Resource for WorkflowResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Workflow
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        if self.spec.steps.is_empty() {
            return Err(anyhow!("workflow.spec.steps cannot be empty"));
        }
        if self.spec.steps.iter().any(|step| step.id.trim().is_empty()) {
            return Err(anyhow!("workflow.spec.steps[].id cannot be empty"));
        }
        if self
            .spec
            .steps
            .iter()
            .any(|step| step.step_type.trim().is_empty())
        {
            return Err(anyhow!("workflow.spec.steps[].type cannot be empty"));
        }
        for step in &self.spec.steps {
            crate::config::validate_step_type(&step.step_type).map_err(|e| anyhow!(e))?;
        }
        let loop_mode = parse_loop_mode(&self.spec.loop_policy.mode)?;
        if matches!(loop_mode, LoopMode::Fixed) {
            match self.spec.loop_policy.max_cycles {
                None | Some(0) => {
                    return Err(anyhow!("workflow loop.mode=fixed requires max_cycles > 0"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let mut incoming = workflow_spec_to_config(&self.spec)?;
        crate::config_load::normalize_workflow_config(&mut incoming);
        let project = config.ensure_project(self.metadata.project.as_deref());
        Ok(super::helpers::apply_to_map(
            &mut project.workflows,
            self.name(),
            incoming,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Workflow,
            &self.metadata,
            ResourceSpec::Workflow(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.default_project()?.workflows.get(name).map(|workflow| Self {
            metadata: super::metadata_from_store(config, "Workflow", name),
            spec: workflow_config_to_spec(workflow),
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        config
            .project_mut(None)
            .map(|project| project.workflows.remove(name).is_some())
            .unwrap_or(false)
    }
}

pub(super) fn build_workflow(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Workflow {
        return Err(anyhow!("resource kind/spec mismatch for Workflow"));
    }
    match spec {
        ResourceSpec::Workflow(spec) => Ok(RegisteredResource::Workflow(WorkflowResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Workflow")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{
        ResourceMetadata, ResourceSpec, SafetySpec, WorkflowLoopSpec, WorkflowStepSpec,
    };
    use crate::config_load::read_active_config;
    use crate::resource::{dispatch_resource, API_VERSION};
    use crate::test_utils::TestState;

    use super::super::test_fixtures::{make_config, workflow_manifest};

    #[test]
    fn workflow_resource_roundtrip() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(workflow_manifest("wf-roundtrip"))
            .expect("workflow dispatch should succeed");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );

        let loaded = WorkflowResource::get_from(&config, "wf-roundtrip")
            .expect("workflow should be present in config");
        // After normalization, missing standard steps are added as disabled placeholders
        assert!(!loaded.spec.steps.is_empty());
        assert!(loaded.spec.steps.iter().any(|s| s.step_type == "qa"));
        assert_eq!(loaded.spec.loop_policy.mode, "once");
        assert_eq!(loaded.spec.loop_policy.max_cycles, Some(3));
    }

    #[test]
    fn workflow_validate_rejects_empty_step_id() {
        let wf = WorkflowResource {
            metadata: super::super::metadata_with_name("wf-empty-id"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "  ".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                    max_parallel: None,
                    timeout_secs: None,
                    behavior: Default::default(),
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: crate::cli_types::WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetySpec::default(),
                max_parallel: None,
            },
        };
        let err = wf.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("id cannot be empty"));
    }

    #[test]
    fn workflow_validate_rejects_empty_step_type() {
        let wf = WorkflowResource {
            metadata: super::super::metadata_with_name("wf-empty-type"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "step1".to_string(),
                    step_type: "  ".to_string(),
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                    max_parallel: None,
                    timeout_secs: None,
                    behavior: Default::default(),
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: crate::cli_types::WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetySpec::default(),
                max_parallel: None,
            },
        };
        let err = wf.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("type cannot be empty"));
    }

    #[test]
    fn workflow_validate_rejects_fixed_without_max_cycles() {
        let wf = WorkflowResource {
            metadata: super::super::metadata_with_name("wf-fixed-no-max"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                    max_parallel: None,
                    timeout_secs: None,
                    behavior: Default::default(),
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "fixed".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: crate::cli_types::WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetySpec::default(),
                max_parallel: None,
            },
        };
        let err = wf.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("max_cycles > 0"));
    }

    #[test]
    fn workflow_validate_rejects_fixed_with_zero_max_cycles() {
        let wf = WorkflowResource {
            metadata: super::super::metadata_with_name("wf-fixed-zero"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                    max_parallel: None,
                    timeout_secs: None,
                    behavior: Default::default(),
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "fixed".to_string(),
                    max_cycles: Some(0),
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: crate::cli_types::WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetySpec::default(),
                max_parallel: None,
            },
        };
        let err = wf.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("max_cycles > 0"));
    }

    #[test]
    fn workflow_validate_accepts_fixed_with_valid_max_cycles() {
        let wf = WorkflowResource {
            metadata: super::super::metadata_with_name("wf-fixed-ok"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                    max_parallel: None,
                    timeout_secs: None,
                    behavior: Default::default(),
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "fixed".to_string(),
                    max_cycles: Some(3),
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: crate::cli_types::WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetySpec::default(),
                max_parallel: None,
            },
        };
        assert!(wf.validate().is_ok());
    }

    #[test]
    fn workflow_validation_rejects_empty_steps() {
        let workflow = WorkflowResource {
            metadata: ResourceMetadata {
                name: "test-workflow".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: WorkflowSpec {
                steps: vec![],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: crate::cli_types::WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetySpec::default(),
                max_parallel: None,
            },
        };
        let result = workflow.validate();
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("cannot be empty"));
    }

    #[test]
    fn workflow_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(WorkflowResource::get_from(&config, "nonexistent-wf").is_none());
    }

    #[test]
    fn workflow_delete_cleans_up_metadata() {
        let mut config = make_config();
        let wf =
            dispatch_resource(workflow_manifest("meta-wf")).expect("dispatch workflow resource");
        wf.apply(&mut config).expect("apply");
        assert!(config.resource_store.get("Workflow", "meta-wf").is_some());

        WorkflowResource::delete_from(&mut config, "meta-wf");
        assert!(config.resource_store.get("Workflow", "meta-wf").is_none());
    }

    #[test]
    fn workflow_apply_stores_resource_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workflow,
            metadata: ResourceMetadata {
                name: "store-meta-wf".to_string(),
                project: None,
                labels: Some([("version".to_string(), "v2".to_string())].into()),
                annotations: None,
            },
            spec: ResourceSpec::Workflow(WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                    max_parallel: None,
                    timeout_secs: None,
                    behavior: Default::default(),
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: crate::cli_types::WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetySpec::default(),
                max_parallel: None,
            }),
        };
        let rr = dispatch_resource(resource).expect("dispatch workflow resource");
        rr.apply(&mut config).expect("apply");

        let cr = config
            .resource_store
            .get("Workflow", "store-meta-wf")
            .expect("stored workflow CR should exist");
        assert_eq!(
            cr.metadata
                .labels
                .as_ref()
                .expect("labels should exist")
                .get("version")
                .expect("version label should exist"),
            "v2"
        );
    }

    #[test]
    fn build_workflow_rejects_wrong_kind() {
        use crate::cli_types::ProjectSpec;
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workflow,
            metadata: ResourceMetadata {
                name: "bad".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Project(ProjectSpec { description: None }),
        };
        let err = dispatch_resource(resource).expect_err("operation should fail");
        assert!(err.to_string().contains("mismatch"));
    }
}
