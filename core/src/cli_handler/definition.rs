use crate::cli::{AgentCommands, WorkflowCommands, WorkspaceCommands};
use crate::cli_types::{
    AgentSpec, OrchestratorResource, ResourceKind, ResourceSpec, SafetySpec, WorkflowFinalizeSpec,
    WorkflowLoopSpec, WorkflowSpec, WorkflowStepSpec, WorkspaceSpec,
};
use crate::config_load::read_active_config;
use anyhow::{Context, Result};

use super::parse::{build_resource_metadata, normalize_loop_mode, validate_workflow_step_type};
use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_workspace(&self, cmd: &WorkspaceCommands) -> Result<i32> {
        let active = read_active_config(&self.state)?;
        match cmd {
            WorkspaceCommands::List { output, project } => {
                let ws_map = if let Some(proj_id) = project {
                    active
                        .config
                        .projects
                        .get(proj_id)
                        .map(|p| &p.workspaces)
                        .unwrap_or(&active.config.workspaces)
                } else {
                    &active.config.workspaces
                };
                let workspaces: Vec<_> = ws_map.keys().cloned().collect();
                self.print_workspaces(&workspaces, ws_map, *output)
            }
            WorkspaceCommands::Info {
                workspace_id,
                output,
            } => {
                // Check global workspaces first, then fall back to project-scoped workspaces
                let ws = active
                    .config
                    .workspaces
                    .get(workspace_id)
                    .or_else(|| {
                        active
                            .config
                            .projects
                            .values()
                            .find_map(|p| p.workspaces.get(workspace_id))
                    })
                    .context(format!("workspace not found: {}", workspace_id))?;
                self.print_workspace_detail(workspace_id, ws, *output)
            }
            WorkspaceCommands::Create {
                name,
                root_path,
                qa_target,
                ticket_dir,
                labels,
                annotations,
                dry_run,
                output,
            } => {
                let metadata = build_resource_metadata(name, labels, annotations)?;
                let spec = WorkspaceSpec {
                    root_path: root_path.clone(),
                    qa_targets: if qa_target.is_empty() {
                        vec!["docs/qa".to_string()]
                    } else {
                        qa_target.clone()
                    },
                    ticket_dir: ticket_dir.clone(),
                    self_referential: false,
                };
                let manifest = OrchestratorResource {
                    api_version: "orchestrator.dev/v2".to_string(),
                    kind: ResourceKind::Workspace,
                    metadata,
                    spec: ResourceSpec::Workspace(spec),
                };
                self.apply_or_preview_manifest(manifest, *dry_run, *output)
            }
        }
    }

    pub(super) fn handle_agent(&self, cmd: &AgentCommands) -> Result<i32> {
        match cmd {
            AgentCommands::Create {
                name,
                command,
                capability,
                labels,
                annotations,
                dry_run,
                output,
            } => {
                let metadata = build_resource_metadata(name, labels, annotations)?;
                let spec = AgentSpec {
                    command: command.clone(),
                    capabilities: if capability.is_empty() {
                        None
                    } else {
                        Some(capability.clone())
                    },
                    metadata: None,
                    selection: None,
                    env: None,
                    prompt_delivery: None,
                };
                let manifest = OrchestratorResource {
                    api_version: "orchestrator.dev/v2".to_string(),
                    kind: ResourceKind::Agent,
                    metadata,
                    spec: ResourceSpec::Agent(Box::new(spec)),
                };
                self.apply_or_preview_manifest(manifest, *dry_run, *output)
            }
        }
    }

    pub(super) fn handle_workflow(&self, cmd: &WorkflowCommands) -> Result<i32> {
        match cmd {
            WorkflowCommands::Create {
                name,
                step,
                loop_mode,
                max_cycles,
                labels,
                annotations,
                dry_run,
                output,
            } => {
                let loop_mode_normalized = normalize_loop_mode(loop_mode)?;
                let steps: Vec<WorkflowStepSpec> = step
                    .iter()
                    .map(|step_type| validate_workflow_step_type(step_type))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .map(|step_type| WorkflowStepSpec {
                        id: step_type.clone(),
                        step_type,
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        command: None,
                        scope: None,
                        max_parallel: None,
                        timeout_secs: None,
                        behavior: Default::default(),
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    })
                    .collect();

                let metadata = build_resource_metadata(name, labels, annotations)?;
                let spec = WorkflowSpec {
                    steps,
                    loop_policy: WorkflowLoopSpec {
                        mode: loop_mode_normalized,
                        max_cycles: *max_cycles,
                        enabled: true,
                        stop_when_no_unresolved: true,
                        agent_template: None,
                    },
                    finalize: WorkflowFinalizeSpec { rules: vec![] },
                    dynamic_steps: vec![],
                    adaptive: None,
                    safety: SafetySpec::default(),
                    max_parallel: None,
                };
                let manifest = OrchestratorResource {
                    api_version: "orchestrator.dev/v2".to_string(),
                    kind: ResourceKind::Workflow,
                    metadata,
                    spec: ResourceSpec::Workflow(spec),
                };
                self.apply_or_preview_manifest(manifest, *dry_run, *output)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::CliHandler;
    use crate::cli::{Cli, Commands, OutputFormat, WorkspaceCommands};
    use crate::test_utils::TestState;

    #[test]
    fn workspace_create_dry_run_emits_manifest() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let cli = Cli {
            command: Commands::Workspace(WorkspaceCommands::Create {
                name: "dry-run-ws".to_string(),
                root_path: "workspace/dry-run".to_string(),
                qa_target: vec![],
                ticket_dir: "docs/ticket".to_string(),
                labels: vec!["env=dev".to_string()],
                annotations: vec![],
                dry_run: true,
                output: OutputFormat::Yaml,
            }),
            verbose: false,
            log_level: None,
            log_format: None,
            unsafe_mode: false,
        };

        let code = handler.execute(&cli).expect("dry-run should pass");
        assert_eq!(code, 0);
    }
}
