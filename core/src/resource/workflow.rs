use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, WorkflowSpec};
use crate::config::{LoopMode, OrchestratorConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

mod workflow_convert;

pub(super) use workflow_convert::workflow_config_to_spec;
use workflow_convert::{parse_loop_mode, workflow_spec_to_config};

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

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = workflow_spec_to_config(&self.spec)
            .expect("validated workflow spec must be convertible");
        let result = super::apply_to_map(&mut config.workflows, self.name(), incoming);
        config.resource_meta.workflows.insert(
            self.name().to_string(),
            crate::config::ResourceStoredMetadata {
                labels: self.metadata.labels.clone(),
                annotations: self.metadata.annotations.clone(),
            },
        );
        result
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Workflow,
            &self.metadata,
            ResourceSpec::Workflow(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.workflows.get(name).map(|workflow| Self {
            metadata: match config.resource_meta.workflows.get(name) {
                Some(stored) => super::metadata_from_parts(
                    name,
                    None,
                    stored.labels.clone(),
                    stored.annotations.clone(),
                ),
                None => super::metadata_with_name(name),
            },
            spec: workflow_config_to_spec(workflow),
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        let removed = config.workflows.remove(name).is_some();
        if removed {
            config.resource_meta.workflows.remove(name);
        }
        removed
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
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);

        let loaded = WorkflowResource::get_from(&config, "wf-roundtrip")
            .expect("workflow should be present in config");
        assert_eq!(loaded.spec.steps.len(), 1);
        assert_eq!(loaded.spec.steps[0].step_type, "qa");
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
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
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
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
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
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
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
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
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
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
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
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
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
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
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
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
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
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
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
                safety: SafetySpec::default(),
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
                safety: SafetySpec::default(),
            },
        };
        let result = workflow.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn workflow_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(WorkflowResource::get_from(&config, "nonexistent-wf").is_none());
    }

    #[test]
    fn workflow_delete_cleans_up_metadata() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("meta-wf")).unwrap();
        wf.apply(&mut config);
        assert!(config.resource_meta.workflows.contains_key("meta-wf"));

        WorkflowResource::delete_from(&mut config, "meta-wf");
        assert!(!config.resource_meta.workflows.contains_key("meta-wf"));
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
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
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
                safety: SafetySpec::default(),
            }),
        };
        let rr = dispatch_resource(resource).unwrap();
        rr.apply(&mut config);

        let stored = config.resource_meta.workflows.get("store-meta-wf").unwrap();
        assert_eq!(
            stored.labels.as_ref().unwrap().get("version").unwrap(),
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
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }
}
