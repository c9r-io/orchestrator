use crate::cli_types::{
    AgentSpec, OrchestratorResource, ProjectSpec, ResourceKind, ResourceMetadata, ResourceSpec,
    ResumeSpec, RunnerSpec, RuntimePolicySpec, SafetySpec, StepTemplateSpec,
    WorkflowFinalizeRuleSpec, WorkflowFinalizeSpec, WorkflowLoopSpec, WorkflowSpec,
    WorkflowStepSpec, WorkspaceSpec,
};
use crate::config::OrchestratorConfig;
use crate::config_load::read_active_config;
use crate::test_utils::TestState;

use super::API_VERSION;

pub fn make_config() -> OrchestratorConfig {
    let mut fixture = TestState::new();
    let state = fixture.build();
    let active = read_active_config(&state).expect("state should be readable");
    active.config.clone()
}

pub fn workspace_manifest(name: &str, root_path: &str) -> OrchestratorResource {
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::Workspace,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::Workspace(WorkspaceSpec {
            root_path: root_path.to_string(),
            qa_targets: vec!["docs/qa".to_string()],
            ticket_dir: "docs/ticket".to_string(),
            self_referential: false,
        }),
    }
}

pub fn agent_manifest(name: &str, command: &str) -> OrchestratorResource {
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::Agent,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::Agent(Box::new(AgentSpec {
            enabled: None,
            command: command.to_string(),
            capabilities: None,
            metadata: None,
            selection: None,
            env: None,
            prompt_delivery: None,
        })),
    }
}

pub fn workflow_manifest(name: &str) -> OrchestratorResource {
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::Workflow,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::Workflow(WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "qa".to_string(),
                step_type: "qa".to_string(),
                required_capability: None,
                template: None,
                execution_profile: None,
                builtin: None,
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                command: None,
                chain_steps: vec![],
                scope: None,
                max_parallel: None,
                timeout_secs: None,
                behavior: Default::default(),
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                extra: Default::default(),
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: Some(3),
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
                convergence_expr: None,
            },
            finalize: WorkflowFinalizeSpec {
                rules: vec![WorkflowFinalizeRuleSpec {
                    id: "qa-passed".to_string(),
                    engine: "cel".to_string(),
                    when: "qa_exit_code == 0".to_string(),
                    status: "qa_passed".to_string(),
                    reason: Some("qa succeeded".to_string()),
                }],
            },
            dynamic_steps: vec![],
            adaptive: None,
            safety: SafetySpec::default(),
            max_parallel: None,
            item_isolation: None,
        }),
    }
}

pub fn project_manifest(name: &str, description: &str) -> OrchestratorResource {
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::Project,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::Project(ProjectSpec {
            description: Some(description.to_string()),
        }),
    }
}

pub fn step_template_manifest(name: &str, prompt: &str) -> OrchestratorResource {
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::StepTemplate,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::StepTemplate(StepTemplateSpec {
            prompt: prompt.to_string(),
            description: None,
        }),
    }
}

pub fn execution_profile_manifest(name: &str, mode: &str, fs_mode: &str) -> OrchestratorResource {
    use crate::cli_types::ExecutionProfileSpec;
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::ExecutionProfile,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::ExecutionProfile(ExecutionProfileSpec {
            mode: mode.to_string(),
            fs_mode: fs_mode.to_string(),
            writable_paths: vec![],
            network_mode: "inherit".to_string(),
            network_allowlist: vec![],
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        }),
    }
}

pub fn env_store_manifest(name: &str) -> OrchestratorResource {
    use crate::cli_types::EnvStoreSpec;
    use std::collections::HashMap;
    let mut data = HashMap::new();
    data.insert("KEY".to_string(), "value".to_string());
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::EnvStore,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::EnvStore(EnvStoreSpec { data }),
    }
}

pub fn secret_store_manifest(name: &str) -> OrchestratorResource {
    use crate::cli_types::EnvStoreSpec;
    use std::collections::HashMap;
    let mut data = HashMap::new();
    data.insert("SECRET".to_string(), "s3cret".to_string());
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::SecretStore,
        metadata: ResourceMetadata {
            name: name.to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::EnvStore(EnvStoreSpec { data }),
    }
}

pub fn runtime_policy_manifest() -> OrchestratorResource {
    OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind: ResourceKind::RuntimePolicy,
        metadata: ResourceMetadata {
            name: "runtime".to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: ResourceSpec::RuntimePolicy(RuntimePolicySpec {
            runner: RunnerSpec {
                shell: "/bin/bash".to_string(),
                shell_arg: "-lc".to_string(),
                policy: "unsafe".to_string(),
                executor: "shell".to_string(),
                allowed_shells: vec![],
                allowed_shell_args: vec![],
                env_allowlist: vec![],
                redaction_patterns: vec![],
            },
            resume: ResumeSpec { auto: false },
            observability: None,
        }),
    }
}
