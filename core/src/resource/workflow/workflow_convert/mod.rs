use crate::cli_types::{
    ConvergenceExprSpec, DynamicStepSpec, SafetySpec, WorkflowFinalizeRuleSpec,
    WorkflowFinalizeSpec, WorkflowLoopSpec, WorkflowPrehookSpec, WorkflowSpec, WorkflowStepSpec,
};
use crate::config::CONVENTIONS;
use crate::config::{
    CheckpointStrategy, ConvergenceExprEntry, CostPreference, LoopMode, SafetyConfig,
    StepHookEngine, StepPrehookConfig, StepPrehookUiConfig, StepScope, WorkflowConfig,
    WorkflowFinalizeConfig, WorkflowFinalizeRule, WorkflowLoopConfig, WorkflowLoopGuardConfig,
    WorkflowSafetyProfile, WorkflowStepConfig, normalize_step_execution_mode,
};
use anyhow::{Result, anyhow};

pub(crate) fn workflow_spec_to_config(spec: &WorkflowSpec) -> Result<WorkflowConfig> {
    let steps = spec
        .steps
        .iter()
        .map(workflow_step_spec_to_config)
        .collect::<Result<Vec<_>>>()?;

    let convergence_expr = spec.loop_policy.convergence_expr.as_ref().map(|entries| {
        entries
            .iter()
            .map(|e| ConvergenceExprEntry {
                engine: parse_hook_engine(&e.engine),
                when: e.when.clone(),
                reason: e.reason.clone(),
            })
            .collect()
    });
    let loop_policy = WorkflowLoopConfig {
        mode: parse_loop_mode(&spec.loop_policy.mode)?,
        guard: WorkflowLoopGuardConfig {
            max_cycles: spec.loop_policy.max_cycles,
            enabled: spec.loop_policy.enabled,
            stop_when_no_unresolved: spec.loop_policy.stop_when_no_unresolved,
            agent_template: spec.loop_policy.agent_template.clone(),
        },
        convergence_expr,
    };

    let finalize = WorkflowFinalizeConfig {
        rules: spec
            .finalize
            .rules
            .iter()
            .map(|rule| WorkflowFinalizeRule {
                id: rule.id.clone(),
                engine: parse_hook_engine(&rule.engine),
                when: rule.when.clone(),
                status: rule.status.clone(),
                reason: rule.reason.clone(),
            })
            .collect(),
    };

    Ok(WorkflowConfig {
        steps,
        execution: Default::default(),
        loop_policy,
        finalize,
        qa: None,
        fix: None,
        retest: None,
        dynamic_steps: spec
            .dynamic_steps
            .iter()
            .map(
                |dynamic_step| crate::dynamic_orchestration::DynamicStepConfig {
                    id: dynamic_step.id.clone(),
                    description: dynamic_step.description.clone(),
                    step_type: dynamic_step.step_type.clone(),
                    agent_id: dynamic_step.agent_id.clone(),
                    template: dynamic_step.template.clone(),
                    trigger: dynamic_step.trigger.clone(),
                    priority: dynamic_step.priority,
                    max_runs: dynamic_step.max_runs,
                },
            )
            .collect(),
        adaptive: spec.adaptive.clone(),
        safety: crate::config::SafetyConfig {
            max_consecutive_failures: spec.safety.max_consecutive_failures,
            auto_rollback: spec.safety.auto_rollback,
            checkpoint_strategy: match spec.safety.checkpoint_strategy.as_str() {
                "git_tag" => crate::config::CheckpointStrategy::GitTag,
                "git_stash" => crate::config::CheckpointStrategy::GitStash,
                _ => crate::config::CheckpointStrategy::None,
            },
            step_timeout_secs: spec.safety.step_timeout_secs,
            stall_timeout_secs: spec.safety.stall_timeout_secs,
            binary_snapshot: spec.safety.binary_snapshot,
            profile: parse_safety_profile(spec.safety.profile.as_deref()),
            invariants: spec.safety.invariants.clone(),
            max_spawned_tasks: spec.safety.max_spawned_tasks,
            max_spawn_depth: spec.safety.max_spawn_depth,
            spawn_cooldown_seconds: spec.safety.spawn_cooldown_seconds,
            max_item_step_failures: spec.safety.max_item_step_failures,
            min_cycle_interval_secs: spec.safety.min_cycle_interval_secs,
            inflight_wait_timeout_secs: spec.safety.inflight_wait_timeout_secs,
            inflight_heartbeat_grace_secs: spec.safety.inflight_heartbeat_grace_secs,
        },
        max_parallel: spec.max_parallel,
        stagger_delay_ms: spec.stagger_delay_ms,
        item_isolation: spec.item_isolation.clone(),
    })
}

pub(crate) fn workflow_config_to_spec(config: &WorkflowConfig) -> WorkflowSpec {
    let steps = config
        .steps
        .iter()
        .map(workflow_step_config_to_spec)
        .collect();

    let loop_policy = WorkflowLoopSpec {
        mode: loop_mode_as_str(&config.loop_policy.mode).to_string(),
        max_cycles: config.loop_policy.guard.max_cycles,
        enabled: config.loop_policy.guard.enabled,
        stop_when_no_unresolved: config.loop_policy.guard.stop_when_no_unresolved,
        agent_template: config.loop_policy.guard.agent_template.clone(),
        convergence_expr: config.loop_policy.convergence_expr.as_ref().map(|entries| {
            entries
                .iter()
                .map(|e| ConvergenceExprSpec {
                    engine: match e.engine {
                        StepHookEngine::Cel => "cel".to_string(),
                    },
                    when: e.when.clone(),
                    reason: e.reason.clone(),
                })
                .collect()
        }),
    };

    let finalize = WorkflowFinalizeSpec {
        rules: config
            .finalize
            .rules
            .iter()
            .map(|rule| WorkflowFinalizeRuleSpec {
                id: rule.id.clone(),
                engine: hook_engine_as_str(&rule.engine).to_string(),
                when: rule.when.clone(),
                status: rule.status.clone(),
                reason: rule.reason.clone(),
            })
            .collect(),
    };

    WorkflowSpec {
        steps,
        loop_policy,
        finalize,
        dynamic_steps: config
            .dynamic_steps
            .iter()
            .map(|dynamic_step| DynamicStepSpec {
                id: dynamic_step.id.clone(),
                description: dynamic_step.description.clone(),
                step_type: dynamic_step.step_type.clone(),
                agent_id: dynamic_step.agent_id.clone(),
                template: dynamic_step.template.clone(),
                trigger: dynamic_step.trigger.clone(),
                priority: dynamic_step.priority,
                max_runs: dynamic_step.max_runs,
            })
            .collect(),
        adaptive: config.adaptive.clone(),
        safety: safety_config_to_spec(&config.safety),
        max_parallel: config.max_parallel,
        stagger_delay_ms: config.stagger_delay_ms,
        item_isolation: config.item_isolation.clone(),
    }
}

fn workflow_step_spec_to_config(step: &WorkflowStepSpec) -> Result<WorkflowStepConfig> {
    crate::config::validate_step_type(&step.step_type).map_err(|e| anyhow!(e))?;
    let step_type = step.step_type.as_str();
    let is_guard = step_type == "loop_guard";
    let builtin = CONVENTIONS.builtin_name(step_type);
    let prehook = match step.prehook.as_ref() {
        Some(prehook) => Some(StepPrehookConfig {
            engine: parse_hook_engine(&prehook.engine),
            when: prehook.when.clone(),
            reason: prehook.reason.clone(),
            ui: prehook
                .ui
                .as_ref()
                .map(|ui| serde_json::from_value::<StepPrehookUiConfig>(ui.clone()))
                .transpose()
                .map_err(|e| anyhow!("invalid prehook ui: {}", e))?,
            extended: prehook.extended,
        }),
        None => None,
    };
    let scope = match step.scope.as_deref() {
        Some("task") => Some(StepScope::Task),
        Some("item") => Some(StepScope::Item),
        _ => None,
    };
    let is_builtin_type = CONVENTIONS.is_known_builtin(step_type);
    let required_capability = step.required_capability.clone().or_else(|| {
        if is_builtin_type || step.builtin.is_some() || !step.chain_steps.is_empty() {
            None
        } else {
            Some(step_type.to_string())
        }
    });
    let builtin = if is_builtin_type {
        Some(step_type.to_string())
    } else {
        builtin
    };
    let mut config_step = WorkflowStepConfig {
        id: step.id.clone(),
        description: None,
        required_capability,
        execution_profile: step.execution_profile.clone(),
        builtin: step.builtin.clone().or(builtin),
        enabled: step.enabled,
        repeatable: step.repeatable,
        is_guard: step.is_guard || is_guard,
        cost_preference: parse_cost_preference(step.cost_preference.as_deref())?,
        prehook,
        tty: step.tty,
        template: step.template.clone(),
        outputs: Vec::new(),
        pipe_to: None,
        command: step.command.clone(),
        chain_steps: step
            .chain_steps
            .iter()
            .map(workflow_step_spec_to_config)
            .collect::<Result<Vec<_>>>()?,
        scope,
        behavior: step.behavior.clone(),
        max_parallel: step.max_parallel,
        stagger_delay_ms: step.stagger_delay_ms,
        timeout_secs: step.timeout_secs,
        stall_timeout_secs: step.stall_timeout_secs,
        item_select_config: step.item_select_config.clone(),
        store_inputs: step.store_inputs.clone(),
        store_outputs: step.store_outputs.clone(),
        step_vars: step.step_vars.clone(),
    };
    normalize_step_execution_mode(&mut config_step).map_err(|e| anyhow!(e))?;
    Ok(config_step)
}

fn workflow_step_config_to_spec(step: &WorkflowStepConfig) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: step.id.clone(),
        step_type: step
            .builtin
            .clone()
            .or_else(|| step.required_capability.clone())
            .unwrap_or_else(|| step.id.clone()),
        required_capability: step.required_capability.clone(),
        builtin: step.builtin.clone(),
        enabled: step.enabled,
        repeatable: step.repeatable,
        is_guard: step.is_guard,
        cost_preference: step.cost_preference.as_ref().map(|c| match c {
            CostPreference::Performance => "performance".to_string(),
            CostPreference::Quality => "quality".to_string(),
            CostPreference::Balance => "balance".to_string(),
        }),
        prehook: step.prehook.as_ref().map(|prehook| WorkflowPrehookSpec {
            engine: hook_engine_as_str(&prehook.engine).to_string(),
            when: prehook.when.clone(),
            reason: prehook.reason.clone(),
            ui: prehook
                .ui
                .as_ref()
                .map(|value| serde_json::to_value(value).unwrap_or(serde_json::Value::Null)),
            extended: prehook.extended,
        }),
        tty: step.tty,
        template: step.template.clone(),
        execution_profile: step.execution_profile.clone(),
        command: step.command.clone(),
        chain_steps: step
            .chain_steps
            .iter()
            .map(workflow_step_config_to_spec)
            .collect(),
        scope: step.scope.and_then(|s| {
            let default = CONVENTIONS.default_scope(&step.id);
            if s != default {
                Some(match s {
                    StepScope::Task => "task".to_string(),
                    StepScope::Item => "item".to_string(),
                })
            } else {
                None
            }
        }),
        max_parallel: step.max_parallel,
        stagger_delay_ms: step.stagger_delay_ms,
        timeout_secs: step.timeout_secs,
        stall_timeout_secs: step.stall_timeout_secs,
        behavior: step.behavior.clone(),
        item_select_config: step.item_select_config.clone(),
        store_inputs: step.store_inputs.clone(),
        store_outputs: step.store_outputs.clone(),
        step_vars: step.step_vars.clone(),
        extra: Default::default(),
    }
}

pub(super) fn safety_config_to_spec(config: &SafetyConfig) -> SafetySpec {
    SafetySpec {
        max_consecutive_failures: config.max_consecutive_failures,
        auto_rollback: config.auto_rollback,
        checkpoint_strategy: checkpoint_strategy_as_str(&config.checkpoint_strategy).to_string(),
        step_timeout_secs: config.step_timeout_secs,
        stall_timeout_secs: config.stall_timeout_secs,
        binary_snapshot: config.binary_snapshot,
        profile: safety_profile_as_str(&config.profile).map(str::to_string),
        invariants: config.invariants.clone(),
        max_spawned_tasks: config.max_spawned_tasks,
        max_spawn_depth: config.max_spawn_depth,
        spawn_cooldown_seconds: config.spawn_cooldown_seconds,
        max_item_step_failures: config.max_item_step_failures,
        min_cycle_interval_secs: config.min_cycle_interval_secs,
        inflight_wait_timeout_secs: config.inflight_wait_timeout_secs,
        inflight_heartbeat_grace_secs: config.inflight_heartbeat_grace_secs,
    }
}

pub(super) fn checkpoint_strategy_as_str(strategy: &CheckpointStrategy) -> &'static str {
    match strategy {
        CheckpointStrategy::GitTag => "git_tag",
        CheckpointStrategy::GitStash => "git_stash",
        CheckpointStrategy::None => "none",
    }
}

fn parse_safety_profile(value: Option<&str>) -> WorkflowSafetyProfile {
    match value {
        Some("self_referential_probe") => WorkflowSafetyProfile::SelfReferentialProbe,
        _ => WorkflowSafetyProfile::Standard,
    }
}

fn safety_profile_as_str(profile: &WorkflowSafetyProfile) -> Option<&'static str> {
    match profile {
        WorkflowSafetyProfile::Standard => None,
        WorkflowSafetyProfile::SelfReferentialProbe => Some("self_referential_probe"),
    }
}

pub(super) fn parse_hook_engine(value: &str) -> StepHookEngine {
    match value {
        "cel" => StepHookEngine::Cel,
        _ => StepHookEngine::Cel,
    }
}

pub(super) fn hook_engine_as_str(value: &StepHookEngine) -> &'static str {
    match value {
        StepHookEngine::Cel => "cel",
    }
}

pub(super) fn parse_cost_preference(value: Option<&str>) -> Result<Option<CostPreference>> {
    Ok(match value {
        Some("performance") => Some(CostPreference::Performance),
        Some("quality") => Some(CostPreference::Quality),
        Some("balance") => Some(CostPreference::Balance),
        Some(other) => return Err(anyhow!("unknown cost_preference '{}'", other)),
        None => None,
    })
}

pub(super) fn parse_loop_mode(value: &str) -> Result<LoopMode> {
    value.parse::<LoopMode>().map_err(|e| anyhow!(e))
}

pub(super) fn loop_mode_as_str(mode: &LoopMode) -> &'static str {
    match mode {
        LoopMode::Once => "once",
        LoopMode::Fixed => "fixed",
        LoopMode::Infinite => "infinite",
    }
}

#[cfg(test)]
mod tests;
