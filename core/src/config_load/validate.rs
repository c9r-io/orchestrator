use crate::config::{
    resolve_step_semantic_kind, ExecutionProfileMode, OrchestratorConfig, StepSemanticKind,
    WorkflowConfig, WorkflowSafetyProfile,
};
use crate::self_referential_policy::{
    evaluate_self_referential_policy, format_blocking_policy_error,
};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::warn;

pub fn validate_workflow_config(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    validate_workflow_config_for_project(config, workflow, workflow_id, None)
}

/// Project-scoped workflow validation. `project_id` of `None` defaults to the
/// default project.
pub fn validate_workflow_config_for_project(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
    project_id: Option<&str>,
) -> Result<()> {
    let pid = config.effective_project_id(project_id);
    let project = config
        .projects
        .get(pid)
        .ok_or_else(|| anyhow::anyhow!("project '{}' not found", pid))?;
    let project_agents = &project.agents;
    if workflow.steps.is_empty() {
        anyhow::bail!("workflow '{}' must define at least one step", workflow_id);
    }
    validate_probe_workflow_shape(workflow, workflow_id)?;
    validate_execution_profiles_for_project(config, workflow, workflow_id, pid)?;

    let mut enabled_count = 0usize;
    let mut seen_ids: HashSet<String> = HashSet::new();
    for step in &workflow.steps {
        if !seen_ids.insert(step.id.clone()) {
            anyhow::bail!(
                "workflow '{}' has duplicate step id '{}'",
                workflow_id,
                step.id
            );
        }
        let key = step
            .builtin
            .as_deref()
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if matches!(
            semantic,
            StepSemanticKind::Builtin { ref name } if name == "ticket_scan"
        ) {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        let is_self_contained = matches!(
            semantic,
            StepSemanticKind::Builtin { .. } | StepSemanticKind::Command | StepSemanticKind::Chain
        );
        if !is_self_contained {
            let has_agent = project_agents.values().any(|a| a.supports_capability(key));
            if !has_agent {
                anyhow::bail!(
                    "no agent supports capability for step '{}' used by workflow '{}'",
                    key,
                    workflow_id
                );
            }
        }
        if let Some(prehook) = step.prehook.as_ref() {
            crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
        }
    }
    if enabled_count == 0 {
        anyhow::bail!("workflow '{}' has no enabled steps", workflow_id);
    }
    for rule in &workflow.finalize.rules {
        crate::prehook::validate_workflow_finalize_rule(rule, workflow_id)?;
    }
    if let Some(max_cycles) = workflow.loop_policy.guard.max_cycles {
        if max_cycles == 0 {
            anyhow::bail!(
                "workflow '{}' loop.guard.max_cycles must be > 0",
                workflow_id
            );
        }
    }
    if matches!(workflow.loop_policy.mode, crate::config::LoopMode::Fixed)
        && workflow.loop_policy.guard.max_cycles.is_none()
    {
        anyhow::bail!(
            "workflow '{}' loop.mode=fixed requires guard.max_cycles > 0",
            workflow_id
        );
    }
    if workflow.loop_policy.guard.enabled
        && !matches!(workflow.loop_policy.mode, crate::config::LoopMode::Once)
    {
        let has_loop_guard = project_agents
            .values()
            .any(|a| a.supports_capability("loop_guard"));
        if !has_loop_guard {
            anyhow::bail!(
                "workflow '{}' loop.guard enabled but no agent supports loop_guard capability",
                workflow_id
            );
        }
    }
    validate_adaptive_workflow_config(workflow, workflow_id, project_agents)?;
    let self_referential_workspaces: Vec<_> = project
        .workspaces
        .iter()
        .filter(|(_, workspace)| workspace.self_referential)
        .collect();

    if workflow.safety.profile == WorkflowSafetyProfile::SelfReferentialProbe {
        if self_referential_workspaces.is_empty() {
            validate_self_referential_safety(workflow, workflow_id, "__unbound__", false)?;
        } else {
            for (workspace_id, _) in &self_referential_workspaces {
                validate_self_referential_safety(workflow, workflow_id, workspace_id, true)?;
            }
        }
    } else {
        for (workspace_id, _) in self_referential_workspaces {
            validate_self_referential_safety(workflow, workflow_id, workspace_id, true)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_execution_profiles_for_project(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
    project_id: &str,
) -> Result<()> {
    let project = config
        .projects
        .get(project_id)
        .ok_or_else(|| anyhow::anyhow!("project '{}' not found", project_id))?;
    for step in &workflow.steps {
        let Some(profile_name) = step.execution_profile.as_deref() else {
            continue;
        };
        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if !matches!(semantic, StepSemanticKind::Agent { .. }) {
            anyhow::bail!(
                "workflow '{}' step '{}' execution_profile is only supported on agent steps",
                workflow_id,
                step.id
            );
        }
        let profile = project
            .execution_profiles
            .get(profile_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "workflow '{}' step '{}' references unknown execution profile '{}'",
                    workflow_id,
                    step.id,
                    profile_name
                )
            })?;
        if profile.mode == ExecutionProfileMode::Host
            && (!profile.writable_paths.is_empty()
                || !profile.network_allowlist.is_empty()
                || profile.max_memory_mb.is_some()
                || profile.max_cpu_seconds.is_some()
                || profile.max_processes.is_some()
                || profile.max_open_files.is_some())
        {
            anyhow::bail!(
                "workflow '{}' step '{}' uses host execution profile '{}' with sandbox-only fields",
                workflow_id,
                step.id,
                profile_name
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_workflow_config_with_agents(
    all_agents: &HashMap<String, &crate::config::AgentConfig>,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    if workflow.steps.is_empty() {
        anyhow::bail!("workflow '{}' must define at least one step", workflow_id);
    }
    validate_probe_workflow_shape(workflow, workflow_id)?;

    let mut enabled_count = 0usize;
    let mut seen_ids: HashSet<String> = HashSet::new();
    for step in &workflow.steps {
        if !seen_ids.insert(step.id.clone()) {
            anyhow::bail!(
                "workflow '{}' has duplicate step id '{}'",
                workflow_id,
                step.id
            );
        }
        let key = step
            .builtin
            .as_deref()
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if matches!(
            semantic,
            StepSemanticKind::Builtin { ref name } if name == "ticket_scan"
        ) {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        let is_self_contained = matches!(
            semantic,
            StepSemanticKind::Builtin { .. } | StepSemanticKind::Command | StepSemanticKind::Chain
        );
        if !is_self_contained {
            let has_agent = all_agents.values().any(|a| a.supports_capability(key));
            if !has_agent {
                anyhow::bail!(
                    "no agent supports capability for step '{}' used by workflow '{}'",
                    key,
                    workflow_id
                );
            }
        }
        if let Some(prehook) = step.prehook.as_ref() {
            crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
        }
    }
    if enabled_count == 0 {
        anyhow::bail!("workflow '{}' has no enabled steps", workflow_id);
    }
    for rule in &workflow.finalize.rules {
        crate::prehook::validate_workflow_finalize_rule(rule, workflow_id)?;
    }
    if let Some(max_cycles) = workflow.loop_policy.guard.max_cycles {
        if max_cycles == 0 {
            anyhow::bail!(
                "workflow '{}' loop.guard.max_cycles must be > 0",
                workflow_id
            );
        }
    }
    if matches!(workflow.loop_policy.mode, crate::config::LoopMode::Fixed)
        && workflow.loop_policy.guard.max_cycles.is_none()
    {
        anyhow::bail!(
            "workflow '{}' loop.mode=fixed requires guard.max_cycles > 0",
            workflow_id
        );
    }
    if workflow.loop_policy.guard.enabled
        && !matches!(workflow.loop_policy.mode, crate::config::LoopMode::Once)
    {
        let has_loop_guard = all_agents
            .values()
            .any(|a| a.supports_capability("loop_guard"));
        if !has_loop_guard {
            anyhow::bail!(
                "workflow '{}' loop.guard enabled but no agent supports loop_guard capability",
                workflow_id
            );
        }
    }
    validate_adaptive_workflow_config_refs(workflow, workflow_id, all_agents)?;
    Ok(())
}

fn validate_adaptive_workflow_config(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    all_agents: &HashMap<String, crate::config::AgentConfig>,
) -> Result<()> {
    let Some(adaptive) = workflow.adaptive.as_ref() else {
        return Ok(());
    };
    if !adaptive.enabled {
        return Ok(());
    }

    let planner_agent = adaptive
        .planner_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "workflow '{}' adaptive planner is enabled but adaptive.planner_agent is missing",
                workflow_id
            )
        })?;

    let agent = all_agents.get(planner_agent).ok_or_else(|| {
        anyhow::anyhow!(
            "workflow '{}' adaptive planner references unknown agent '{}'",
            workflow_id,
            planner_agent
        )
    })?;

    if !agent.supports_capability("adaptive_plan") {
        anyhow::bail!(
            "workflow '{}' adaptive planner agent '{}' must support capability 'adaptive_plan'",
            workflow_id,
            planner_agent
        );
    }

    Ok(())
}

fn validate_adaptive_workflow_config_refs(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    all_agents: &HashMap<String, &crate::config::AgentConfig>,
) -> Result<()> {
    let Some(adaptive) = workflow.adaptive.as_ref() else {
        return Ok(());
    };
    if !adaptive.enabled {
        return Ok(());
    }

    let planner_agent = adaptive
        .planner_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "workflow '{}' adaptive planner is enabled but adaptive.planner_agent is missing",
                workflow_id
            )
        })?;

    let agent = all_agents.get(planner_agent).copied().ok_or_else(|| {
        anyhow::anyhow!(
            "workflow '{}' adaptive planner references unknown agent '{}'",
            workflow_id,
            planner_agent
        )
    })?;

    if !agent.supports_capability("adaptive_plan") {
        anyhow::bail!(
            "workflow '{}' adaptive planner agent '{}' must support capability 'adaptive_plan'",
            workflow_id,
            planner_agent
        );
    }

    Ok(())
}

pub(crate) fn validate_probe_workflow_shape(
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    if workflow.safety.profile != WorkflowSafetyProfile::SelfReferentialProbe {
        return Ok(());
    }
    let evaluation = evaluate_self_referential_policy(workflow, workflow_id, "__probe__", true)?;
    if evaluation.has_blocking_errors() {
        anyhow::bail!(format_blocking_policy_error(&evaluation));
    }
    Ok(())
}

/// Validate safety configuration for self-referential workspaces.
pub fn validate_self_referential_safety(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    workspace_id: &str,
    workspace_is_self_referential: bool,
) -> Result<()> {
    let evaluation = evaluate_self_referential_policy(
        workflow,
        workflow_id,
        workspace_id,
        workspace_is_self_referential,
    )?;
    for diagnostic in evaluation
        .diagnostics
        .iter()
        .filter(|diagnostic| !diagnostic.blocking)
    {
        warn!(
            workspace_id,
            rule_id = diagnostic.rule_id,
            "{}",
            diagnostic.message
        );
    }
    if evaluation.has_blocking_errors() {
        anyhow::bail!(format_blocking_policy_error(&evaluation));
    }
    Ok(())
}

/// Validates that all agent env store references (fromRef, refValue.name) point to
/// existing entries in config.env_stores.
pub fn validate_agent_env_store_refs(config: &OrchestratorConfig) -> Result<()> {
    for (project_id, project) in &config.projects {
        for (agent_name, agent_cfg) in &project.agents {
            if let Some(ref entries) = agent_cfg.env {
                for entry in entries {
                    if let Some(ref store_name) = entry.from_ref {
                        if !project.env_stores.contains_key(store_name.as_str()) {
                            anyhow::bail!(
                                "agent '{}'(project '{}') env fromRef '{}' references unknown store",
                                agent_name,
                                project_id,
                                store_name
                            );
                        }
                    }
                    if let Some(ref rv) = entry.ref_value {
                        if !project.env_stores.contains_key(&rv.name) {
                            anyhow::bail!(
                                "agent '{}'(project '{}') env refValue.name '{}' references unknown store",
                                agent_name,
                                project_id,
                                rv.name
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Like `validate_agent_env_store_refs` but only validates agents in the given project.
pub fn validate_agent_env_store_refs_for_project(
    config: &OrchestratorConfig,
    project_id: &str,
) -> Result<()> {
    if let Some(project) = config.projects.get(project_id) {
        for (agent_name, agent_cfg) in &project.agents {
            if let Some(ref entries) = agent_cfg.env {
                for entry in entries {
                    if let Some(ref store_name) = entry.from_ref {
                        if !project.env_stores.contains_key(store_name.as_str()) {
                            anyhow::bail!(
                                "agent '{}'(project '{}') env fromRef '{}' references unknown store",
                                agent_name,
                                project_id,
                                store_name
                            );
                        }
                    }
                    if let Some(ref rv) = entry.ref_value {
                        if !project.env_stores.contains_key(&rv.name) {
                            anyhow::bail!(
                                "agent '{}'(project '{}') env refValue.name '{}' references unknown store",
                                agent_name,
                                project_id,
                                rv.name
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn ensure_within_root(root: &Path, target: &Path, field: &str) -> Result<()> {
    let root_canon = root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize workspace root {}", root.display()))?;
    let target_canon = target.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize path {} for {}",
            target.display(),
            field
        )
    })?;
    if !target_canon.starts_with(&root_canon) {
        anyhow::bail!(
            "{} resolves outside workspace root: {}",
            field,
            target_canon.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        LoopMode, OrchestratorConfig, StepBehavior, StepScope, WorkflowConfig, WorkflowStepConfig,
    };
    use crate::config_load::tests::{
        make_builtin_step, make_command_step, make_config_with_agent,
        make_config_with_default_project, make_step, make_workflow,
    };
    #[allow(unused_imports)]
    use std::collections::HashMap;

    #[test]
    fn validate_workflow_config_allows_multiple_self_test_steps() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "self_test_fail".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    execution_profile: None,
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
                    id: "self_test_recover".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    execution_profile: None,
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
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig {
                    enabled: false,
                    ..crate::config::WorkflowLoopGuardConfig::default()
                },
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };

        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-workflow");
        assert!(
            result.is_ok(),
            "validation should allow multiple self_test steps"
        );
    }

    #[test]
    fn validate_workflow_config_allows_multiple_implement_steps() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "implement_phase_one".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    execution_profile: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo phase-one".to_string()),
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
                    id: "implement_phase_two".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    execution_profile: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo phase-two".to_string()),
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
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig {
                    enabled: false,
                    ..crate::config::WorkflowLoopGuardConfig::default()
                },
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };

        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-workflow");
        assert!(
            result.is_ok(),
            "validation should allow multiple implement steps when step ids are unique"
        );
    }

    #[test]
    fn validate_workflow_config_rejects_duplicate_step_ids() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "duplicate_step".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    execution_profile: None,
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
                    id: "duplicate_step".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    execution_profile: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo duplicate".to_string()),
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
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig {
                    enabled: false,
                    ..crate::config::WorkflowLoopGuardConfig::default()
                },
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };

        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-workflow");
        assert!(
            result.is_err(),
            "validation should reject duplicate step ids"
        );
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("duplicate step id 'duplicate_step'"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn validate_self_referential_safety_errors_missing_self_test() {
        let workflow = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "implement".to_string(),
                description: None,
                builtin: None,
                required_capability: Some("implement".to_string()),
                execution_profile: None,
                enabled: true,
                repeatable: true,
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
            }],
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig::default(),
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitTag,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
                ..crate::config::SafetyConfig::default()
            },
            max_parallel: None,
        };

        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(result.is_err(), "validation should fail without self_test");
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("self_ref.self_test_required"));
    }

    #[test]
    fn validate_self_referential_safety_passes_with_self_test() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "implement".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: Some("implement".to_string()),
                    execution_profile: None,
                    enabled: true,
                    repeatable: true,
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
                    builtin: None,
                    required_capability: None,
                    execution_profile: None,
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
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig::default(),
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitTag,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
                ..crate::config::SafetyConfig::default()
            },
            max_parallel: None,
        };

        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(result.is_ok(), "validation should pass with self_test step");
    }

    #[test]
    fn validate_self_referential_safety_errors_without_checkpoint_strategy() {
        let workflow = WorkflowConfig {
            steps: vec![],
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig::default(),
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::None,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
                ..crate::config::SafetyConfig::default()
            },
            max_parallel: None,
        };

        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(result.is_err(), "should error without checkpoint_strategy");
        let err_msg = result.expect_err("operation should fail").to_string();
        assert!(
            err_msg.contains("checkpoint_strategy"),
            "error should mention checkpoint_strategy"
        );
    }

    #[test]
    fn validate_workflow_rejects_empty_steps() {
        let workflow = make_workflow(vec![]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("at least one step"));
    }

    #[test]
    fn validate_workflow_rejects_no_enabled_steps() {
        let workflow = make_workflow(vec![make_step("qa", false)]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("no enabled steps"));
    }

    #[test]
    fn validate_workflow_rejects_missing_agent_template() {
        let workflow = make_workflow(vec![make_step("qa", true)]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("no agent supports capability"));
    }

    #[test]
    fn validate_workflow_accepts_step_with_agent_template() {
        let workflow = make_workflow(vec![make_step("qa", true)]);
        let config = make_config_with_agent("qa", "qa_template.md");
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "should accept step when agent has template: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_accepts_builtin_step_without_agent() {
        let workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "builtin steps should not require agent: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_accepts_command_step_without_agent() {
        let workflow = make_workflow(vec![make_command_step("build", "cargo build")]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "command steps should not require agent: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_accepts_chain_steps_without_agent() {
        let mut step = make_step("smoke_chain", true);
        step.chain_steps = vec![make_command_step("sub", "echo sub")];
        let workflow = make_workflow(vec![step]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "chain_steps should count as self-contained: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_rejects_zero_max_cycles() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.guard.max_cycles = Some(0);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("max_cycles must be > 0"));
    }

    #[test]
    fn validate_workflow_rejects_fixed_without_max_cycles() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.mode = LoopMode::Fixed;
        workflow.loop_policy.guard.max_cycles = None;
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("loop.mode=fixed requires guard.max_cycles"));
    }

    #[test]
    fn validate_workflow_accepts_fixed_with_max_cycles() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.mode = LoopMode::Fixed;
        workflow.loop_policy.guard.max_cycles = Some(2);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "fixed mode with max_cycles should pass: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_rejects_guard_enabled_without_loop_guard_agent() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.mode = LoopMode::Infinite;
        workflow.loop_policy.guard.enabled = true;
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("no agent supports loop_guard capability"));
    }

    #[test]
    fn validate_workflow_accepts_guard_enabled_with_loop_guard_agent() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.mode = LoopMode::Infinite;
        workflow.loop_policy.guard.enabled = true;
        let config = make_config_with_agent("loop_guard", "loop_guard_template.md");
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "guard with agent should pass: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_skips_guard_check_for_once_mode() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.mode = LoopMode::Once;
        workflow.loop_policy.guard.enabled = true;
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "once mode should skip guard agent check: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_skips_disabled_steps() {
        let workflow = make_workflow(vec![
            make_step("qa", false),
            make_builtin_step("self_test", "self_test", true),
        ]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "disabled step missing agent should not error: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_allows_ticket_scan_without_agent() {
        let workflow = make_workflow(vec![
            make_step("ticket_scan", true),
            make_builtin_step("self_test", "self_test", true),
        ]);
        let config = make_config_with_default_project();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(
            result.is_ok(),
            "ticket_scan should not require agent: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_workflow_rejects_step_with_builtin_and_required_capability() {
        let mut step = make_builtin_step("self_test", "self_test", true);
        step.required_capability = Some("self_test".to_string());
        let workflow = make_workflow(vec![step]);
        let config = make_config_with_default_project();

        let result = validate_workflow_config(&config, &workflow, "test-wf");

        assert!(
            result.is_err(),
            "conflicting semantic fields should fail validation"
        );
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("cannot define both builtin and required_capability"));
    }

    #[test]
    fn validate_self_referential_safety_errors_disabled_auto_rollback() {
        let workflow = WorkflowConfig {
            steps: vec![make_step("implement", true)],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitStash,
                auto_rollback: false,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
                ..crate::config::SafetyConfig::default()
            },
            ..make_workflow(vec![])
        };
        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("self_ref.auto_rollback_required"));
    }

    #[test]
    fn validate_self_referential_safety_passes_with_git_stash() {
        let workflow = WorkflowConfig {
            steps: vec![make_step("implement", true), make_step("self_test", true)],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitStash,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
                ..crate::config::SafetyConfig::default()
            },
            ..make_workflow(vec![])
        };
        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(result.is_ok());
    }

    #[test]
    fn self_referential_probe_without_self_test_is_rejected() {
        let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;

        let result = validate_self_referential_safety(&workflow, "probe", "self-ref", true);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("self_ref.self_test_required"));
    }

    #[test]
    fn validate_workflow_config_rejects_probe_without_git_tag_checkpoint() {
        let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        let config = make_config_with_default_project();

        let err = validate_workflow_config(&config, &workflow, "probe")
            .expect_err("probe profile should require git_tag");
        assert!(err.to_string().contains("checkpoint_strategy=git_tag"));
    }

    #[test]
    fn validate_workflow_config_rejects_probe_without_auto_rollback() {
        let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        let config = make_config_with_default_project();

        let err = validate_workflow_config(&config, &workflow, "probe")
            .expect_err("probe profile should require auto_rollback");
        assert!(err.to_string().contains("auto_rollback=true"));
    }

    #[test]
    fn validate_workflow_config_rejects_probe_with_item_scoped_steps() {
        let mut workflow = make_workflow(vec![make_command_step("qa", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        let config = make_config_with_default_project();

        let err = validate_workflow_config(&config, &workflow, "probe")
            .expect_err("probe profile should reject item scope");
        assert!(err.to_string().contains("task-scoped"));
    }

    #[test]
    fn validate_workflow_config_rejects_probe_with_agent_steps() {
        let mut workflow = make_workflow(vec![make_step("implement", true)]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        let config = make_config_with_default_project();

        let err = validate_workflow_config(&config, &workflow, "probe")
            .expect_err("probe profile should reject agent steps");
        assert!(err.to_string().contains("self-contained command"));
    }

    #[test]
    fn validate_workflow_config_rejects_probe_with_strict_phase() {
        let mut workflow = make_workflow(vec![make_command_step("build", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        let config = make_config_with_default_project();

        let err = validate_workflow_config(&config, &workflow, "probe")
            .expect_err("probe profile should reject strict phases");
        let message = err.to_string();
        assert!(message.contains("self_ref.probe_forbidden_phase"));
        assert!(message.contains("forbidden phase"));
    }

    #[test]
    fn validate_self_referential_safety_rejects_probe_on_non_self_referential_workspace() {
        let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;

        let err = validate_self_referential_safety(&workflow, "probe", "plain-ws", false)
            .expect_err("probe profile should require self_referential workspace");
        assert!(err.to_string().contains("not self_referential"));
    }

    #[test]
    fn ensure_within_root_accepts_child_path() {
        let root = std::env::temp_dir();
        let child = root.join(format!("test-within-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&child).expect("create child directory");
        let result = ensure_within_root(&root, &child, "test");
        assert!(result.is_ok());
        std::fs::remove_dir_all(&child).ok();
    }

    #[test]
    fn ensure_within_root_rejects_nonexistent_path() {
        let root = std::env::temp_dir();
        let nonexistent = root.join("nonexistent-path-xyz-abc");
        let result = ensure_within_root(&root, &nonexistent, "test");
        assert!(result.is_err(), "should fail for nonexistent path");
    }

    // ============================================================================
    // Group 1: ensure_within_root() additional tests
    // ============================================================================

    #[test]
    fn ensure_within_root_rejects_path_outside_root() {
        // Create a unique temp root directory
        let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root directory");

        // Use a sibling directory (outside root) as target
        let outside = std::env::temp_dir().join(format!("test-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&outside).expect("create outside directory");

        let result = ensure_within_root(&root, &outside, "test_field");
        assert!(result.is_err(), "should reject path outside root");
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("resolves outside workspace root"),
            "unexpected error: {}",
            err
        );

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn ensure_within_root_accepts_root_equals_target() {
        let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root directory");

        let result = ensure_within_root(&root, &root, "test_field");
        assert!(
            result.is_ok(),
            "root equals target should pass: {:?}",
            result.err()
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_within_root_accepts_deeply_nested_child() {
        let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
        let deep_child = root.join("a").join("b").join("c").join("d").join("e");
        std::fs::create_dir_all(&deep_child).expect("create deep child directory");

        let result = ensure_within_root(&root, &deep_child, "test_field");
        assert!(
            result.is_ok(),
            "deeply nested child should pass: {:?}",
            result.err()
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_within_root_rejects_symlink_escaping_root() {
        // Create root and outside directories
        let root = std::env::temp_dir().join(format!("test-root-{}", uuid::Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("test-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root directory");
        std::fs::create_dir_all(&outside).expect("create outside directory");

        // Create symlink inside root pointing outside
        let symlink = root.join("escape_link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &symlink).expect("create symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, &symlink).expect("create symlink");

        let result = ensure_within_root(&root, &symlink, "test_field");
        assert!(result.is_err(), "symlink escaping root should be rejected");
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("resolves outside workspace root"),
            "unexpected error: {}",
            err
        );

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    // ============================================================================
    // Group 2: validate_probe_workflow_shape() direct tests
    // ============================================================================

    #[test]
    fn validate_probe_workflow_shape_rejects_chain_steps_via_semantic_kind() {
        // chain_steps causes resolve_step_semantic_kind to return Chain (not Command),
        // which is rejected by the "only allows self-contained command steps" check.
        // Note: the explicit chain_steps.is_empty() check (line 259) is unreachable
        // because Chain semantic is caught first at line 252.
        let mut step = make_command_step("implement", "echo probe");
        step.chain_steps = vec![make_command_step("sub", "echo sub")];

        let mut workflow = make_workflow(vec![
            step,
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        workflow.loop_policy.mode = LoopMode::Once;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("self_ref.probe_command_steps_only"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn validate_probe_workflow_shape_rejects_fixed_loop_mode() {
        let mut workflow = make_workflow(vec![
            make_command_step("implement", "echo probe"),
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        workflow.loop_policy.mode = LoopMode::Fixed;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("self_ref.probe_requires_loop_mode_once"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn validate_probe_workflow_shape_rejects_infinite_loop_mode() {
        let mut workflow = make_workflow(vec![
            make_command_step("implement", "echo probe"),
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        workflow.loop_policy.mode = LoopMode::Infinite;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("self_ref.probe_requires_loop_mode_once"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn validate_probe_workflow_shape_rejects_forbidden_phase_qa_testing() {
        let mut step = make_command_step("qa_testing", "echo qa");
        step.scope = Some(StepScope::Task); // Set to Task scope to pass scope check
        let mut workflow = make_workflow(vec![
            step,
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("self_ref.probe_forbidden_phase"),
            "unexpected error: {}",
            err
        );
        assert!(
            err.contains("qa_testing"),
            "error should mention phase name"
        );
    }

    #[test]
    fn validate_probe_workflow_shape_rejects_forbidden_phase_ticket_fix() {
        let mut step = make_command_step("ticket_fix", "echo fix");
        step.scope = Some(StepScope::Task); // Set to Task scope to pass scope check
        let mut workflow = make_workflow(vec![
            step,
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("ticket_fix"),
            "error should mention phase name"
        );
    }

    #[test]
    fn validate_probe_workflow_shape_rejects_forbidden_phase_loop_guard() {
        let mut step = make_command_step("loop_guard", "echo guard");
        step.scope = Some(StepScope::Task);
        let mut workflow = make_workflow(vec![
            step,
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(result.is_err());
        let err = result.expect_err("operation should fail").to_string();
        assert!(
            err.contains("loop_guard"),
            "error should mention phase name"
        );
    }

    #[test]
    fn validate_probe_workflow_shape_accepts_custom_phase_name() {
        let mut workflow = make_workflow(vec![
            make_command_step("custom_probe_task", "echo custom"),
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        workflow.loop_policy.mode = LoopMode::Once;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(
            result.is_ok(),
            "custom phase name should pass: {:?}",
            result.err()
        );
    }

    // ============================================================================
    // Group 3: validate_self_referential_safety() edge case tests
    // ============================================================================

    #[test]
    fn validate_self_referential_safety_standard_profile_skips_non_self_ref_workspace() {
        let mut workflow = make_workflow(vec![make_step("implement", true)]);
        workflow.safety.profile = WorkflowSafetyProfile::Standard;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::None;

        let result = validate_self_referential_safety(&workflow, "standard-wf", "plain-ws", false);
        assert!(
            result.is_ok(),
            "non self-referential workspace should skip checks"
        );
    }

    #[test]
    fn validate_self_referential_safety_standard_profile_accepts_valid_checkpoint() {
        let mut workflow = make_workflow(vec![make_step("implement", true)]);
        workflow.safety.profile = WorkflowSafetyProfile::Standard;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;

        let result = validate_self_referential_safety(&workflow, "standard-wf", "plain-ws", false);
        assert!(
            result.is_ok(),
            "standard profile with valid checkpoint should pass: {:?}",
            result.err()
        );
    }

    // ============================================================================
    // Group 4: validate_probe_workflow_shape() — disabled steps and coverage gaps
    // ============================================================================

    #[test]
    fn validate_probe_workflow_shape_skips_disabled_forbidden_phase() {
        // Disabled steps should be skipped even if they have forbidden phase names.
        let mut step = make_command_step("self_test", "echo forbidden");
        step.enabled = false;
        step.scope = Some(StepScope::Task);
        let mut workflow = make_workflow(vec![
            make_command_step("custom_task", "echo ok"),
            make_builtin_step("self_test", "self_test", true),
            step,
        ]);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;
        workflow.loop_policy.mode = LoopMode::Once;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(
            result.is_ok(),
            "disabled forbidden phase should be skipped: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_probe_workflow_shape_allows_builtin_self_test() {
        let mut workflow = make_workflow(vec![
            make_command_step("implement", "echo test"),
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.steps[0].scope = Some(StepScope::Task);
        workflow.steps[1].scope = Some(StepScope::Task);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;

        let result = validate_probe_workflow_shape(&workflow, "probe");
        assert!(
            result.is_ok(),
            "builtin self_test should be allowed for probe"
        );
    }

    #[test]
    fn validate_probe_workflow_shape_non_probe_profile_returns_ok() {
        // Standard profile should pass through without probe-specific checks.
        let mut workflow = make_workflow(vec![make_step("qa", true)]);
        workflow.safety.profile = WorkflowSafetyProfile::Standard;

        let result = validate_probe_workflow_shape(&workflow, "test-wf");
        assert!(
            result.is_ok(),
            "non-probe profile should skip all probe checks: {:?}",
            result.err()
        );
    }

    #[test]
    fn validate_agent_env_store_refs_passes_with_valid_refs() {
        use crate::cli_types::{AgentEnvEntry, AgentEnvRefValue};
        use crate::config::{AgentConfig, EnvStoreConfig};

        let mut config = OrchestratorConfig::default();
        let project = config
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default();
        project.env_stores.insert(
            "shared".to_string(),
            EnvStoreConfig {
                data: [("K".to_string(), "V".to_string())].into(),
                sensitive: false,
            },
        );
        project.agents.insert(
            "agent1".to_string(),
            AgentConfig {
                env: Some(vec![
                    AgentEnvEntry {
                        name: None,
                        value: None,
                        from_ref: Some("shared".to_string()),
                        ref_value: None,
                    },
                    AgentEnvEntry {
                        name: Some("X".to_string()),
                        value: None,
                        from_ref: None,
                        ref_value: Some(AgentEnvRefValue {
                            name: "shared".to_string(),
                            key: "K".to_string(),
                        }),
                    },
                ]),
                ..AgentConfig::default()
            },
        );
        assert!(validate_agent_env_store_refs(&config).is_ok());
    }

    #[test]
    fn validate_agent_env_store_refs_rejects_missing_from_ref() {
        use crate::cli_types::AgentEnvEntry;
        use crate::config::AgentConfig;

        let mut config = OrchestratorConfig::default();
        config
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .agents
            .insert(
                "bad-agent".to_string(),
                AgentConfig {
                    env: Some(vec![AgentEnvEntry {
                        name: None,
                        value: None,
                        from_ref: Some("nonexistent".to_string()),
                        ref_value: None,
                    }]),
                    ..AgentConfig::default()
                },
            );
        let err = validate_agent_env_store_refs(&config).unwrap_err();
        assert!(err.to_string().contains("unknown store"));
        assert!(err.to_string().contains("bad-agent"));
    }

    #[test]
    fn validate_agent_env_store_refs_rejects_missing_ref_value_store() {
        use crate::cli_types::{AgentEnvEntry, AgentEnvRefValue};
        use crate::config::AgentConfig;

        let mut config = OrchestratorConfig::default();
        config
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .agents
            .insert(
                "bad-agent".to_string(),
                AgentConfig {
                    env: Some(vec![AgentEnvEntry {
                        name: Some("X".to_string()),
                        value: None,
                        from_ref: None,
                        ref_value: Some(AgentEnvRefValue {
                            name: "missing-store".to_string(),
                            key: "KEY".to_string(),
                        }),
                    }]),
                    ..AgentConfig::default()
                },
            );
        let err = validate_agent_env_store_refs(&config).unwrap_err();
        assert!(err.to_string().contains("unknown store"));
    }

    #[test]
    fn validate_agent_env_store_refs_passes_with_no_env() {
        use crate::config::AgentConfig;

        let mut config = OrchestratorConfig::default();
        config
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .agents
            .insert("basic-agent".to_string(), AgentConfig::default());
        assert!(validate_agent_env_store_refs(&config).is_ok());
    }
}
