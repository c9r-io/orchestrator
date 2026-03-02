use crate::config::{
    default_scope_for_step_id, resolve_step_semantic_kind, OrchestratorConfig, StepScope,
    StepSemanticKind, WorkflowConfig, WorkflowSafetyProfile,
};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn validate_workflow_config(
    config: &OrchestratorConfig,
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
            let has_agent = config
                .agents
                .values()
                .any(|a| a.get_template(key).is_some());
            if !has_agent {
                anyhow::bail!(
                    "no agent has template for step '{}' used by workflow '{}'",
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
        let has_loop_guard = config
            .agents
            .values()
            .any(|a| a.get_template("loop_guard").is_some());
        if !has_loop_guard {
            anyhow::bail!(
                "workflow '{}' loop.guard enabled but no agent has loop_guard template",
                workflow_id
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
            let has_agent = all_agents.values().any(|a| a.get_template(key).is_some());
            if !has_agent {
                anyhow::bail!(
                    "no agent has template for step '{}' used by workflow '{}'",
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
            .any(|a| a.get_template("loop_guard").is_some());
        if !has_loop_guard {
            anyhow::bail!(
                "workflow '{}' loop.guard enabled but no agent has loop_guard template",
                workflow_id
            );
        }
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

    if !matches!(
        workflow.safety.checkpoint_strategy,
        crate::config::CheckpointStrategy::GitTag
    ) {
        anyhow::bail!(
            "workflow '{}' with self_referential_probe profile requires safety.checkpoint_strategy=git_tag",
            workflow_id
        );
    }

    if !workflow.safety.auto_rollback {
        anyhow::bail!(
            "workflow '{}' with self_referential_probe profile requires safety.auto_rollback=true",
            workflow_id
        );
    }

    if !matches!(workflow.loop_policy.mode, crate::config::LoopMode::Once) {
        anyhow::bail!(
            "workflow '{}' with self_referential_probe profile requires loop.mode=once",
            workflow_id
        );
    }

    for step in &workflow.steps {
        if !step.enabled {
            continue;
        }

        let scope = step
            .scope
            .unwrap_or_else(|| default_scope_for_step_id(&step.id));
        if scope != StepScope::Task {
            anyhow::bail!(
                "workflow '{}' with self_referential_probe profile only allows task-scoped steps",
                workflow_id
            );
        }

        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if !matches!(semantic, StepSemanticKind::Command) {
            anyhow::bail!(
                "workflow '{}' with self_referential_probe profile only allows self-contained command steps",
                workflow_id
            );
        }

        if !step.chain_steps.is_empty() {
            anyhow::bail!(
                "workflow '{}' with self_referential_probe profile does not allow chain steps",
                workflow_id
            );
        }

        if matches!(
            step.id.as_str(),
            "qa" | "qa_testing"
                | "fix"
                | "ticket_fix"
                | "retest"
                | "guard"
                | "build"
                | "test"
                | "lint"
                | "self_test"
                | "smoke_chain"
                | "ticket_scan"
                | "init_once"
                | "loop_guard"
        ) {
            anyhow::bail!(
                "workflow '{}' with self_referential_probe profile does not allow strict or builtin phases like '{}'",
                workflow_id,
                step.id
            );
        }
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
    if workflow.safety.profile == WorkflowSafetyProfile::SelfReferentialProbe {
        if !workspace_is_self_referential {
            anyhow::bail!(
                "workflow '{}' is marked self_referential_probe but workspace '{}' is not self_referential",
                workflow_id,
                workspace_id
            );
        }
        return Ok(());
    }

    // Hard error: checkpoint_strategy must not be None
    if matches!(
        workflow.safety.checkpoint_strategy,
        crate::config::CheckpointStrategy::None
    ) {
        anyhow::bail!(
            "[SELF_REF_UNSAFE] workspace '{}' is self_referential but checkpoint_strategy is 'none'. \
             Self-referential workspaces MUST have a checkpoint strategy (e.g. git_tag) to enable rollback.",
            workspace_id
        );
    }

    // Warning: auto_rollback should be enabled
    if !workflow.safety.auto_rollback {
        eprintln!(
            "[warn] workspace '{}' is self_referential but auto_rollback is disabled. \
             Consider enabling auto_rollback for self-referential workspaces.",
            workspace_id
        );
    }

    // Warning: no self_test step in workflow
    let has_self_test = workflow.steps.iter().any(|s| s.id == "self_test");
    if !has_self_test {
        eprintln!(
            "[warn] workspace '{}' is self_referential but has no 'self_test' step in its workflow. \
             Consider adding a self_test step after 'implement' to catch breaking changes early.",
            workspace_id
        );
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
        LoopMode, OrchestratorConfig, StepBehavior, WorkflowConfig, WorkflowStepConfig,
    };
    use crate::config_load::tests::{
        make_builtin_step, make_command_step, make_config_with_agent, make_step, make_workflow,
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
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "self_test_recover".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
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
            safety: crate::config::SafetyConfig::default(),
        };

        let config = OrchestratorConfig::default();
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
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo phase-one".to_string()),
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "implement_phase_two".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo phase-two".to_string()),
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
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
            safety: crate::config::SafetyConfig::default(),
        };

        let config = OrchestratorConfig::default();
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
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "duplicate_step".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo duplicate".to_string()),
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
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
            safety: crate::config::SafetyConfig::default(),
        };

        let config = OrchestratorConfig::default();
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
    fn validate_self_referential_safety_warns_missing_self_test() {
        let workflow = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "implement".to_string(),
                description: None,
                builtin: None,
                required_capability: Some("implement".to_string()),
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
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
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitTag,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
            },
        };

        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(
            result.is_ok(),
            "validation should pass even without self_test"
        );
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
                    enabled: true,
                    repeatable: true,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "self_test".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
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
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitTag,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
            },
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
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::None,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
            },
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result.expect_err("operation should fail").to_string().contains("no enabled steps"));
    }

    #[test]
    fn validate_workflow_rejects_missing_agent_template() {
        let workflow = make_workflow(vec![make_step("qa", true)]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("no agent has template"));
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("no agent has loop_guard template"));
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();
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
        let config = OrchestratorConfig::default();

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
    fn validate_self_referential_safety_warns_disabled_auto_rollback() {
        let workflow = WorkflowConfig {
            steps: vec![make_step("implement", true)],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitStash,
                auto_rollback: false,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: WorkflowSafetyProfile::Standard,
            },
            ..make_workflow(vec![])
        };
        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(result.is_ok());
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
            },
            ..make_workflow(vec![])
        };
        let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
        assert!(result.is_ok());
    }

    #[test]
    fn self_referential_probe_without_self_test_does_not_warn_or_error() {
        let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        workflow.safety.auto_rollback = true;

        let result = validate_self_referential_safety(&workflow, "probe", "self-ref", true);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_workflow_config_rejects_probe_without_git_tag_checkpoint() {
        let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        let config = OrchestratorConfig::default();

        let err = validate_workflow_config(&config, &workflow, "probe")
            .expect_err("probe profile should require git_tag");
        assert!(err.to_string().contains("checkpoint_strategy=git_tag"));
    }

    #[test]
    fn validate_workflow_config_rejects_probe_without_auto_rollback() {
        let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
        workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
        let config = OrchestratorConfig::default();

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
        let config = OrchestratorConfig::default();

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
        let config = OrchestratorConfig::default();

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
        let config = OrchestratorConfig::default();

        let err = validate_workflow_config(&config, &workflow, "probe")
            .expect_err("probe profile should reject strict phases");
        assert!(err.to_string().contains("does not allow"));
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
}
