use crate::config::{
    AgentConfig, OrchestratorConfig, WorkflowConfig, WorkflowStepConfig, WorkflowStepType,
};
use crate::config_validation::{
    ErrorCode, ValidationError, ValidationResult, ValidationWarning, WarningCode,
};
use std::collections::HashMap;

pub fn validate_schema(config: &OrchestratorConfig) -> ValidationResult {
    let mut result = ValidationResult::new();
    result.is_valid = true;

    let all_workspaces = collect_all_workspaces(config);
    let all_agents = collect_all_agents(config);

    validate_runner(&config.runner, &mut result);
    validate_defaults(
        &config.defaults,
        &all_workspaces,
        &config.workflows,
        &mut result,
    );
    validate_workspaces(&config.workspaces, &config.projects, &mut result);
    validate_agents(&config.agents, &config.projects, &mut result);
    validate_workflows(&config.workflows, &all_agents, &mut result);
    validate_projects(&config.projects, &config.agents, &mut result);

    result
}

fn collect_all_workspaces(
    config: &OrchestratorConfig,
) -> HashMap<String, crate::config::WorkspaceConfig> {
    let mut all = config.workspaces.clone();
    for project in config.projects.values() {
        for (ws_id, ws) in &project.workspaces {
            all.entry(ws_id.clone()).or_insert_with(|| ws.clone());
        }
    }
    all
}

fn collect_all_agents(config: &OrchestratorConfig) -> HashMap<String, AgentConfig> {
    let mut all = config.agents.clone();
    for project in config.projects.values() {
        for (agent_id, agent) in &project.agents {
            all.entry(agent_id.clone()).or_insert_with(|| agent.clone());
        }
    }
    all
}

fn validate_runner(runner: &crate::config::RunnerConfig, result: &mut ValidationResult) {
    if runner.shell.trim().is_empty() {
        result.add_error(ValidationError {
            code: ErrorCode::MissingRequiredField,
            message: "runner.shell cannot be empty".to_string(),
            field: Some("runner.shell".to_string()),
            context: None,
        });
    }
    if runner.shell_arg.trim().is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "runner.shell_arg is recommended".to_string(),
            field: Some("runner.shell_arg".to_string()),
            suggestion: Some("Add shell argument (e.g., '-lc')".to_string()),
        });
    }
    if runner.policy == crate::config::RunnerPolicy::Allowlist {
        if runner.allowed_shells.is_empty() {
            result.add_error(ValidationError {
                code: ErrorCode::MissingRequiredField,
                message: "runner.allowed_shells cannot be empty when policy=allowlist".to_string(),
                field: Some("runner.allowed_shells".to_string()),
                context: None,
            });
        }
        if runner.allowed_shell_args.is_empty() {
            result.add_error(ValidationError {
                code: ErrorCode::MissingRequiredField,
                message: "runner.allowed_shell_args cannot be empty when policy=allowlist"
                    .to_string(),
                field: Some("runner.allowed_shell_args".to_string()),
                context: None,
            });
        }
    }
}

fn validate_defaults(
    defaults: &crate::config::ConfigDefaults,
    workspaces: &HashMap<String, crate::config::WorkspaceConfig>,
    workflows: &HashMap<String, crate::config::WorkflowConfig>,
    result: &mut ValidationResult,
) {
    if defaults.workspace.trim().is_empty() {
        result.add_error(ValidationError {
            code: ErrorCode::MissingRequiredField,
            message: "defaults.workspace is required".to_string(),
            field: Some("defaults.workspace".to_string()),
            context: None,
        });
    } else if !workspaces.contains_key(&defaults.workspace) {
        result.add_error(ValidationError {
            code: ErrorCode::InvalidReference,
            message: format!("defaults.workspace '{}' does not exist", defaults.workspace),
            field: Some("defaults.workspace".to_string()),
            context: None,
        });
    }

    if defaults.workflow.trim().is_empty() {
        result.add_error(ValidationError {
            code: ErrorCode::MissingRequiredField,
            message: "defaults.workflow is required".to_string(),
            field: Some("defaults.workflow".to_string()),
            context: None,
        });
    } else if !workflows.contains_key(&defaults.workflow) {
        result.add_error(ValidationError {
            code: ErrorCode::InvalidReference,
            message: format!("defaults.workflow '{}' does not exist", defaults.workflow),
            field: Some("defaults.workflow".to_string()),
            context: None,
        });
    }

    if defaults.project.trim().is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "defaults.project is recommended".to_string(),
            field: Some("defaults.project".to_string()),
            suggestion: Some("Add a default project".to_string()),
        });
    }
}

fn validate_workspaces(
    workspaces: &HashMap<String, crate::config::WorkspaceConfig>,
    projects: &HashMap<String, crate::config::ProjectConfig>,
    result: &mut ValidationResult,
) {
    let has_project_workspaces = projects.values().any(|p| !p.workspaces.is_empty());

    if workspaces.is_empty() && !has_project_workspaces {
        result.add_error(ValidationError {
            code: ErrorCode::EmptyCollection,
            message: "At least one workspace is required".to_string(),
            field: Some("workspaces".to_string()),
            context: None,
        });
        return;
    }

    for (id, ws) in workspaces {
        validate_single_workspace(id, ws, "workspaces", result);
    }

    for (proj_id, project) in projects {
        for (ws_id, ws) in &project.workspaces {
            let prefix = format!("projects.{}.workspaces", proj_id);
            validate_single_workspace(ws_id, ws, &prefix, result);
        }
    }
}

fn validate_single_workspace(
    id: &str,
    ws: &crate::config::WorkspaceConfig,
    parent_field: &str,
    result: &mut ValidationResult,
) {
    let field_prefix = format!("{}.{}", parent_field, id);

    if ws.root_path.trim().is_empty() {
        result.add_error(ValidationError {
            code: ErrorCode::MissingRequiredField,
            message: "root_path is required".to_string(),
            field: Some(format!("{}.root_path", field_prefix)),
            context: None,
        });
    }

    if ws.qa_targets.is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "qa_targets is empty".to_string(),
            field: Some(format!("{}.qa_targets", field_prefix)),
            suggestion: Some("Add qa_targets for QA testing".to_string()),
        });
    }

    if ws.ticket_dir.trim().is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "ticket_dir is recommended".to_string(),
            field: Some(format!("{}.ticket_dir", field_prefix)),
            suggestion: Some("Add ticket_dir for ticket tracking".to_string()),
        });
    }
}

fn validate_agents(
    agents: &HashMap<String, AgentConfig>,
    projects: &HashMap<String, crate::config::ProjectConfig>,
    result: &mut ValidationResult,
) {
    let has_project_agents = projects.values().any(|p| !p.agents.is_empty());

    if agents.is_empty() && !has_project_agents {
        result.add_error(ValidationError {
            code: ErrorCode::EmptyCollection,
            message: "At least one agent is required".to_string(),
            field: Some("agents".to_string()),
            context: None,
        });
        return;
    }

    for (id, agent) in agents {
        validate_single_agent(id, agent, "agents", result);
    }

    for (proj_id, project) in projects {
        for (agent_id, agent) in &project.agents {
            let prefix = format!("projects.{}.agents", proj_id);
            validate_single_agent(agent_id, agent, &prefix, result);
        }
    }
}

fn validate_single_agent(
    id: &str,
    agent: &AgentConfig,
    parent_field: &str,
    result: &mut ValidationResult,
) {
    let field_prefix = format!("{}.{}", parent_field, id);

    if agent.templates.is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "Agent has no templates".to_string(),
            field: Some(format!("{}.templates", field_prefix)),
            suggestion: Some("Add at least one template (e.g., qa, fix)".to_string()),
        });
    }

    if agent.capabilities.is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "Agent has no capabilities declared".to_string(),
            field: Some(format!("{}.capabilities", field_prefix)),
            suggestion: Some("Add capabilities for agent selection".to_string()),
        });
    }
}

fn validate_workflows(
    workflows: &HashMap<String, WorkflowConfig>,
    agents: &HashMap<String, AgentConfig>,
    result: &mut ValidationResult,
) {
    if workflows.is_empty() {
        result.add_error(ValidationError {
            code: ErrorCode::EmptyCollection,
            message: "At least one workflow is required".to_string(),
            field: Some("workflows".to_string()),
            context: None,
        });
        return;
    }

    for (id, wf) in workflows {
        validate_workflow(id, wf, agents, result);
    }
}

fn validate_workflow(
    id: &str,
    wf: &WorkflowConfig,
    agents: &HashMap<String, AgentConfig>,
    result: &mut ValidationResult,
) {
    let field_prefix = format!("workflows.{}", id);

    if wf.steps.is_empty() {
        result.add_error(ValidationError {
            code: ErrorCode::EmptyCollection,
            message: "Workflow must have at least one step".to_string(),
            field: Some(format!("{}.steps", field_prefix)),
            context: None,
        });
    }

    let mut seen_steps: HashSet<String> = HashSet::new();
    let mut enabled_count = 0usize;

    for step in &wf.steps {
        validate_workflow_step(
            id,
            step,
            agents,
            &mut seen_steps,
            &mut enabled_count,
            result,
        );
    }

    if enabled_count == 0 {
        result.add_warning(ValidationWarning {
            code: WarningCode::EmptyConfiguration,
            message: "Workflow has no enabled steps".to_string(),
            field: Some(format!("{}.steps", field_prefix)),
            suggestion: Some("Enable at least one step for execution".to_string()),
        });
    }
}

fn validate_workflow_step(
    workflow_id: &str,
    step: &WorkflowStepConfig,
    agents: &HashMap<String, AgentConfig>,
    seen_steps: &mut HashSet<String>,
    enabled_count: &mut usize,
    result: &mut ValidationResult,
) {
    let step_key = step
        .step_type
        .as_ref()
        .map(|t| t.as_str())
        .or(step.builtin.as_deref())
        .or(step.required_capability.as_deref())
        .unwrap_or(&step.id);

    let field_prefix = format!("workflows.{}.steps", workflow_id);

    if seen_steps.contains(step_key) {
        result.add_error(ValidationError {
            code: ErrorCode::DuplicateEntry,
            message: format!("Duplicate step type '{}'", step_key),
            field: Some(field_prefix.clone()),
            context: Some(format!("step: {}", step.id)),
        });
    }
    seen_steps.insert(step_key.to_string());

    if step.id.trim().is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "Step id is empty".to_string(),
            field: Some(field_prefix.clone()),
            suggestion: Some("Add a descriptive step id".to_string()),
        });
    }

    if step.enabled {
        *enabled_count += 1;

        let is_builtin_type = step.builtin.is_some()
            || matches!(
                step.step_type.as_ref(),
                Some(
                    WorkflowStepType::InitOnce
                        | WorkflowStepType::TicketScan
                        | WorkflowStepType::LoopGuard
                )
            );
        let has_template = agents.values().any(|a| a.get_template(step_key).is_some());
        if !has_template && !is_builtin_type {
            result.add_error(ValidationError {
                code: ErrorCode::InvalidReference,
                message: format!("No agent has template for step '{}'", step_key),
                field: Some(field_prefix.clone()),
                context: Some(format!("step: {}", step.id)),
            });
        }

        if let Some(ref cap) = step.required_capability {
            let has_capable_agent = agents.values().any(|a| a.capabilities.contains(cap));
            if !has_capable_agent {
                result.add_error(ValidationError {
                    code: ErrorCode::InvalidReference,
                    message: format!(
                        "step '{}' requires capability '{}' but no agent provides it",
                        step.id, cap
                    ),
                    field: Some(field_prefix),
                    context: Some(format!("workflow: {}", workflow_id)),
                });
            }
        }
    }
}

fn validate_projects(
    projects: &HashMap<String, crate::config::ProjectConfig>,
    global_agents: &HashMap<String, AgentConfig>,
    result: &mut ValidationResult,
) {
    for (proj_id, project) in projects {
        let field_prefix = format!("projects.{}", proj_id);

        for (ws_id, ws) in &project.workspaces {
            if ws.root_path.trim().is_empty() {
                result.add_error(ValidationError {
                    code: ErrorCode::MissingRequiredField,
                    message: "root_path is required".to_string(),
                    field: Some(format!("{}.workspaces.{}.root_path", field_prefix, ws_id)),
                    context: None,
                });
            }
        }

        let merged_agents: HashMap<String, &AgentConfig> = global_agents
            .iter()
            .chain(project.agents.iter())
            .map(|(k, v)| (k.clone(), v))
            .collect();

        for (wf_id, wf) in &project.workflows {
            for step in &wf.steps {
                if let Some(ref cap) = step.required_capability {
                    let has_capable = merged_agents.values().any(|a| a.capabilities.contains(cap));
                    if !has_capable {
                        result.add_error(ValidationError {
                            code: ErrorCode::InvalidReference,
                            message: format!(
                                "step '{}' requires capability '{}' but no agent provides it",
                                step.id, cap
                            ),
                            field: Some(format!("{}.workflows.{}", field_prefix, wf_id)),
                            context: Some(format!("project: {}", proj_id)),
                        });
                    }
                }
            }
        }
    }
}

use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> OrchestratorConfig {
        OrchestratorConfig::default()
    }

    #[test]
    fn test_empty_config_fails() {
        let config = empty_config();
        let result = validate_schema(&config);
        assert!(!result.is_valid);
        assert!(result.errors.len() >= 4);
    }

    #[test]
    fn test_valid_config() {
        let mut config = empty_config();
        config.workspaces.insert(
            "default".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
            },
        );
        let mut agent = AgentConfig::new();
        agent
            .templates
            .insert("qa".to_string(), "echo test".to_string());
        agent.capabilities.push("qa".to_string());
        config.agents.insert("echo".to_string(), agent);
        config.workflows.insert(
            "basic".to_string(),
            WorkflowConfig {
                steps: vec![WorkflowStepConfig {
                    id: "qa".to_string(),
                    step_type: Some(WorkflowStepType::Qa),
                    enabled: true,
                    repeatable: true,
                    is_guard: false,
                    description: None,
                    required_capability: None,
                    builtin: None,
                    cost_preference: None,
                    prehook: None,
                }],
                loop_policy: crate::config::WorkflowLoopConfig::default(),
                finalize: crate::config::WorkflowFinalizeConfig::default(),
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
            },
        );
        config.defaults.workspace = "default".to_string();
        config.defaults.workflow = "basic".to_string();

        let result = validate_schema(&config);
        assert!(result.is_valid, "Errors: {:?}", result.errors);
    }

    #[test]
    fn test_duplicate_workflow_step() {
        let mut config = empty_config();
        config.workspaces.insert(
            "default".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
            },
        );
        config.agents.insert(
            "echo".to_string(),
            AgentConfig {
                templates: [("qa".to_string(), "echo test".to_string())]
                    .into_iter()
                    .collect(),
                metadata: crate::config::AgentMetadata::default(),
                capabilities: vec![],
                selection: crate::config::AgentSelectionConfig::default(),
            },
        );
        config.workflows.insert(
            "basic".to_string(),
            WorkflowConfig {
                steps: vec![
                    WorkflowStepConfig {
                        id: "qa1".to_string(),
                        step_type: Some(WorkflowStepType::Qa),
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        description: None,
                        required_capability: None,
                        builtin: None,
                        cost_preference: None,
                        prehook: None,
                    },
                    WorkflowStepConfig {
                        id: "qa2".to_string(),
                        step_type: Some(WorkflowStepType::Qa),
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        description: None,
                        required_capability: None,
                        builtin: None,
                        cost_preference: None,
                        prehook: None,
                    },
                ],
                loop_policy: crate::config::WorkflowLoopConfig::default(),
                finalize: crate::config::WorkflowFinalizeConfig::default(),
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
            },
        );
        config.defaults.workspace = "default".to_string();
        config.defaults.workflow = "basic".to_string();

        let result = validate_schema(&config);
        assert!(!result.is_valid);
    }
}
