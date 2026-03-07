use crate::cli_types::{
    DynamicStepSpec, SafetySpec, WorkflowFinalizeRuleSpec, WorkflowFinalizeSpec, WorkflowLoopSpec,
    WorkflowPrehookSpec, WorkflowSpec, WorkflowStepSpec,
};
use crate::config::{
    normalize_step_execution_mode, CheckpointStrategy, CostPreference, LoopMode, SafetyConfig,
    StepHookEngine, StepPrehookConfig, StepPrehookUiConfig, StepScope, WorkflowConfig,
    WorkflowFinalizeConfig, WorkflowFinalizeRule, WorkflowLoopConfig, WorkflowLoopGuardConfig,
    WorkflowSafetyProfile, WorkflowStepConfig,
};
use anyhow::{anyhow, Result};

pub(crate) fn workflow_spec_to_config(spec: &WorkflowSpec) -> Result<WorkflowConfig> {
    let steps = spec
        .steps
        .iter()
        .map(|step| {
            crate::config::validate_step_type(&step.step_type).map_err(|e| anyhow!(e))?;
            let is_guard = step.step_type == "loop_guard";
            let builtin = if matches!(
                step.step_type.as_str(),
                "init_once" | "loop_guard" | "self_test" | "self_restart"
            ) {
                Some(step.step_type.clone())
            } else {
                None
            };
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
            let is_builtin_type = matches!(
                step.step_type.as_str(),
                "init_once"
                    | "loop_guard"
                    | "ticket_scan"
                    | "self_test"
                    | "self_restart"
                    | "item_select"
            );
            let required_capability = step.required_capability.clone().or_else(|| {
                if is_builtin_type || step.builtin.is_some() {
                    None
                } else {
                    Some(step.step_type.clone())
                }
            });
            let builtin = if is_builtin_type {
                Some(step.step_type.clone())
            } else {
                builtin
            };
            let mut config_step = WorkflowStepConfig {
                id: step.id.clone(),
                description: None,
                required_capability,
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
                chain_steps: vec![],
                scope,
                behavior: step.behavior.clone(),
                max_parallel: step.max_parallel,
                timeout_secs: step.timeout_secs,
                item_select_config: step.item_select_config.clone(),
                store_inputs: step.store_inputs.clone(),
                store_outputs: step.store_outputs.clone(),
            };
            normalize_step_execution_mode(&mut config_step).map_err(|e| anyhow!(e))?;
            Ok(config_step)
        })
        .collect::<Result<Vec<_>>>()?;

    let loop_policy = WorkflowLoopConfig {
        mode: parse_loop_mode(&spec.loop_policy.mode)?,
        guard: WorkflowLoopGuardConfig {
            max_cycles: spec.loop_policy.max_cycles,
            enabled: spec.loop_policy.enabled,
            stop_when_no_unresolved: spec.loop_policy.stop_when_no_unresolved,
            agent_template: spec.loop_policy.agent_template.clone(),
        },
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
        safety: crate::config::SafetyConfig {
            max_consecutive_failures: spec.safety.max_consecutive_failures,
            auto_rollback: spec.safety.auto_rollback,
            checkpoint_strategy: match spec.safety.checkpoint_strategy.as_str() {
                "git_tag" => crate::config::CheckpointStrategy::GitTag,
                "git_stash" => crate::config::CheckpointStrategy::GitStash,
                _ => crate::config::CheckpointStrategy::None,
            },
            step_timeout_secs: spec.safety.step_timeout_secs,
            binary_snapshot: spec.safety.binary_snapshot,
            profile: parse_safety_profile(spec.safety.profile.as_deref()),
            invariants: spec.safety.invariants.clone(),
            max_spawned_tasks: spec.safety.max_spawned_tasks,
            max_spawn_depth: spec.safety.max_spawn_depth,
            spawn_cooldown_seconds: spec.safety.spawn_cooldown_seconds,
        },
        max_parallel: spec.max_parallel,
    })
}

pub(crate) fn workflow_config_to_spec(config: &WorkflowConfig) -> WorkflowSpec {
    let steps = config
        .steps
        .iter()
        .map(|step| WorkflowStepSpec {
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
            command: step.command.clone(),
            scope: step.scope.and_then(|s| {
                // Only serialize when it differs from default
                let default = crate::config::default_scope_for_step_id(&step.id);
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
            timeout_secs: step.timeout_secs,
            behavior: step.behavior.clone(),
            item_select_config: step.item_select_config.clone(),
            store_inputs: step.store_inputs.clone(),
            store_outputs: step.store_outputs.clone(),
        })
        .collect();

    let loop_policy = WorkflowLoopSpec {
        mode: loop_mode_as_str(&config.loop_policy.mode).to_string(),
        max_cycles: config.loop_policy.guard.max_cycles,
        enabled: config.loop_policy.guard.enabled,
        stop_when_no_unresolved: config.loop_policy.guard.stop_when_no_unresolved,
        agent_template: config.loop_policy.guard.agent_template.clone(),
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
        safety: safety_config_to_spec(&config.safety),
        max_parallel: config.max_parallel,
    }
}

pub(super) fn safety_config_to_spec(config: &SafetyConfig) -> SafetySpec {
    SafetySpec {
        max_consecutive_failures: config.max_consecutive_failures,
        auto_rollback: config.auto_rollback,
        checkpoint_strategy: checkpoint_strategy_as_str(&config.checkpoint_strategy).to_string(),
        step_timeout_secs: config.step_timeout_secs,
        binary_snapshot: config.binary_snapshot,
        profile: safety_profile_as_str(&config.profile).map(str::to_string),
        invariants: config.invariants.clone(),
        max_spawned_tasks: config.max_spawned_tasks,
        max_spawn_depth: config.max_spawn_depth,
        spawn_cooldown_seconds: config.spawn_cooldown_seconds,
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
mod tests {
    use super::*;
    use crate::cli_types::{
        DynamicStepSpec, SafetySpec, WorkflowFinalizeRuleSpec, WorkflowFinalizeSpec,
        WorkflowLoopSpec, WorkflowPrehookSpec, WorkflowSpec, WorkflowStepSpec,
    };
    use crate::config::{
        CheckpointStrategy, CostPreference, ExecutionMode, LoopMode, SafetyConfig, StepBehavior,
        StepHookEngine, StepPrehookConfig, StepScope, WorkflowConfig, WorkflowFinalizeConfig,
        WorkflowFinalizeRule, WorkflowLoopConfig, WorkflowLoopGuardConfig, WorkflowStepConfig,
    };

    // ── parse_cost_preference tests ─────────────────────────────────

    #[test]
    fn parse_cost_preference_all_variants() {
        assert_eq!(
            parse_cost_preference(Some("performance")).expect("parse performance"),
            Some(CostPreference::Performance)
        );
        assert_eq!(
            parse_cost_preference(Some("quality")).expect("parse quality"),
            Some(CostPreference::Quality)
        );
        assert_eq!(
            parse_cost_preference(Some("balance")).expect("parse balance"),
            Some(CostPreference::Balance)
        );
        assert_eq!(parse_cost_preference(None).expect("parse none"), None);
    }

    #[test]
    fn parse_cost_preference_rejects_unknown() {
        let err = parse_cost_preference(Some("turbo")).expect_err("operation should fail");
        assert!(err.to_string().contains("unknown cost_preference"));
    }

    // ── parse_hook_engine / hook_engine_as_str ──────────────────────

    #[test]
    fn parse_hook_engine_defaults_to_cel() {
        assert!(matches!(parse_hook_engine("cel"), StepHookEngine::Cel));
        assert!(matches!(parse_hook_engine("unknown"), StepHookEngine::Cel));
        assert!(matches!(parse_hook_engine(""), StepHookEngine::Cel));
    }

    #[test]
    fn hook_engine_as_str_returns_cel() {
        assert_eq!(hook_engine_as_str(&StepHookEngine::Cel), "cel");
    }

    // ── parse_loop_mode / loop_mode_as_str ──────────────────────────

    #[test]
    fn parse_loop_mode_infinite() {
        let mode = parse_loop_mode("infinite").expect("infinite should parse");
        match mode {
            LoopMode::Infinite => (), // pass
            other => assert!(matches!(other, LoopMode::Infinite), "expected Infinite"),
        }
    }

    #[test]
    fn parse_loop_mode_fixed() {
        let mode = parse_loop_mode("fixed").expect("fixed should parse");
        assert!(matches!(mode, LoopMode::Fixed));
    }

    #[test]
    fn parse_loop_mode_rejects_invalid() {
        assert!(matches!(parse_loop_mode("once"), Ok(LoopMode::Once)));
        assert!(matches!(parse_loop_mode("fixed"), Ok(LoopMode::Fixed)));
        let invalid = parse_loop_mode("anything_else").expect_err("invalid mode should fail");
        assert!(invalid.to_string().contains("unknown loop mode"));
    }

    #[test]
    fn loop_mode_as_str_returns_correct_values() {
        assert_eq!(loop_mode_as_str(&LoopMode::Once), "once");
        assert_eq!(loop_mode_as_str(&LoopMode::Fixed), "fixed");
        assert_eq!(loop_mode_as_str(&LoopMode::Infinite), "infinite");
    }

    // ── workflow_spec_to_config conversion details ──────────────────

    #[test]
    fn workflow_spec_to_config_with_prehook_and_cost() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "qa".to_string(),
                step_type: "qa".to_string(),
                required_capability: Some("qa".to_string()),
                template: None,
                builtin: None,
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: Some("quality".to_string()),
                prehook: Some(WorkflowPrehookSpec {
                    engine: "cel".to_string(),
                    when: "is_last_cycle".to_string(),
                    reason: Some("only run on last cycle".to_string()),
                    ui: None,
                    extended: false,
                }),
                tty: true,
                command: Some("cargo test".to_string()),
                scope: Some("task".to_string()),
                max_parallel: None,
                timeout_secs: None,
                behavior: Default::default(),
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "fixed".to_string(),
                max_cycles: Some(2),
                enabled: true,
                stop_when_no_unresolved: false,
                agent_template: Some("guard_template".to_string()),
            },
            finalize: WorkflowFinalizeSpec {
                rules: vec![WorkflowFinalizeRuleSpec {
                    id: "rule1".to_string(),
                    engine: "cel".to_string(),
                    when: "qa_exit_code == 0".to_string(),
                    status: "passed".to_string(),
                    reason: Some("QA passed".to_string()),
                }],
            },
            dynamic_steps: vec![DynamicStepSpec {
                id: "dyn1".to_string(),
                description: Some("dynamic step".to_string()),
                step_type: "qa".to_string(),
                agent_id: Some("agent1".to_string()),
                template: Some("tmpl".to_string()),
                trigger: Some("always".to_string()),
                priority: 10,
                max_runs: Some(3),
            }],
            safety: SafetySpec::default(),
            max_parallel: None,
        };

        let config = workflow_spec_to_config(&spec).expect("should convert");
        assert_eq!(config.steps.len(), 1);
        let step = &config.steps[0];
        assert_eq!(step.id, "qa");
        assert_eq!(step.cost_preference, Some(CostPreference::Quality));
        assert!(step.prehook.is_some());
        let prehook = step.prehook.as_ref().expect("prehook should exist");
        assert_eq!(prehook.when, "is_last_cycle");
        assert_eq!(prehook.reason.as_deref(), Some("only run on last cycle"));
        assert!(step.tty);
        assert_eq!(step.command.as_deref(), Some("cargo test"));
        assert_eq!(step.scope, Some(StepScope::Task));

        // Loop config
        assert!(matches!(config.loop_policy.mode, LoopMode::Fixed));
        assert_eq!(config.loop_policy.guard.max_cycles, Some(2));
        assert!(!config.loop_policy.guard.stop_when_no_unresolved);
        assert_eq!(
            config.loop_policy.guard.agent_template.as_deref(),
            Some("guard_template")
        );

        // Finalize
        assert_eq!(config.finalize.rules.len(), 1);
        assert_eq!(config.finalize.rules[0].id, "rule1");
        assert_eq!(config.finalize.rules[0].status, "passed");

        // Dynamic steps
        assert_eq!(config.dynamic_steps.len(), 1);
        assert_eq!(config.dynamic_steps[0].id, "dyn1");
        assert_eq!(config.dynamic_steps[0].priority, 10);
        assert_eq!(config.dynamic_steps[0].max_runs, Some(3));
    }

    #[test]
    fn workflow_spec_to_config_init_once_sets_builtin() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "init".to_string(),
                step_type: "init_once".to_string(),
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
            max_parallel: None,
        };
        let config = workflow_spec_to_config(&spec).expect("convert workflow spec");
        assert_eq!(config.steps[0].builtin.as_deref(), Some("init_once"));
        assert_eq!(
            config.steps[0].behavior.execution,
            ExecutionMode::Builtin {
                name: "init_once".to_string()
            }
        );
    }

    #[test]
    fn workflow_spec_to_config_self_test_sets_builtin_execution() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "self_test".to_string(),
                step_type: "self_test".to_string(),
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
            max_parallel: None,
        };

        let config = workflow_spec_to_config(&spec).expect("convert workflow spec");

        assert_eq!(config.steps[0].builtin.as_deref(), Some("self_test"));
        assert_eq!(config.steps[0].required_capability, None);
        assert_eq!(
            config.steps[0].behavior.execution,
            ExecutionMode::Builtin {
                name: "self_test".to_string()
            }
        );
    }

    #[test]
    fn workflow_spec_to_config_loop_guard_sets_is_guard_and_builtin() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "guard".to_string(),
                step_type: "loop_guard".to_string(),
                required_capability: None,
                template: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false, // should be set to true by conversion
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
            max_parallel: None,
        };
        let config = workflow_spec_to_config(&spec).expect("convert workflow spec");
        assert!(config.steps[0].is_guard);
        assert_eq!(config.steps[0].builtin.as_deref(), Some("loop_guard"));
    }

    #[test]
    fn workflow_spec_to_config_scope_item() {
        let spec = WorkflowSpec {
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
                scope: Some("item".to_string()),
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
            max_parallel: None,
        };
        let config = workflow_spec_to_config(&spec).expect("convert workflow spec");
        assert_eq!(config.steps[0].scope, Some(StepScope::Item));
    }

    #[test]
    fn workflow_spec_to_config_safety_git_tag() {
        let spec = WorkflowSpec {
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec {
                max_consecutive_failures: 5,
                auto_rollback: true,
                checkpoint_strategy: "git_tag".to_string(),
                step_timeout_secs: Some(600),
                binary_snapshot: true,
                profile: Some("self_referential_probe".to_string()),
                invariants: vec![],
                max_spawned_tasks: None,
                max_spawn_depth: None,
                spawn_cooldown_seconds: None,
            },
            max_parallel: None,
        };
        let config = workflow_spec_to_config(&spec).expect("convert workflow spec");
        assert_eq!(config.safety.max_consecutive_failures, 5);
        assert!(config.safety.auto_rollback);
        assert!(matches!(
            config.safety.checkpoint_strategy,
            crate::config::CheckpointStrategy::GitTag
        ));
        assert_eq!(config.safety.step_timeout_secs, Some(600));
        assert!(config.safety.binary_snapshot);
        assert_eq!(
            config.safety.profile,
            WorkflowSafetyProfile::SelfReferentialProbe
        );
    }

    #[test]
    fn workflow_spec_to_config_safety_git_stash() {
        let spec = WorkflowSpec {
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec {
                max_consecutive_failures: 3,
                auto_rollback: false,
                checkpoint_strategy: "git_stash".to_string(),
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: None,
                invariants: vec![],
                max_spawned_tasks: None,
                max_spawn_depth: None,
                spawn_cooldown_seconds: None,
            },
            max_parallel: None,
        };
        let config = workflow_spec_to_config(&spec).expect("convert workflow spec");
        assert!(matches!(
            config.safety.checkpoint_strategy,
            crate::config::CheckpointStrategy::GitStash
        ));
    }

    #[test]
    fn workflow_spec_to_config_safety_none_strategy() {
        let spec = WorkflowSpec {
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec {
                max_consecutive_failures: 3,
                auto_rollback: false,
                checkpoint_strategy: "unknown_strat".to_string(),
                step_timeout_secs: None,
                binary_snapshot: false,
                profile: None,
                invariants: vec![],
                max_spawned_tasks: None,
                max_spawn_depth: None,
                spawn_cooldown_seconds: None,
            },
            max_parallel: None,
        };
        let config = workflow_spec_to_config(&spec).expect("convert workflow spec");
        assert!(matches!(
            config.safety.checkpoint_strategy,
            crate::config::CheckpointStrategy::None
        ));
    }

    // ── workflow_config_to_spec conversion details ──────────────────

    #[test]
    fn workflow_config_to_spec_cost_preference_mapping() {
        let config = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "perf".to_string(),
                    description: None,
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: Some(CostPreference::Performance),
                    prehook: None,
                    tty: false,
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
                    id: "qual".to_string(),
                    description: None,
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: Some(CostPreference::Quality),
                    prehook: None,
                    tty: false,
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
                    id: "bal".to_string(),
                    description: None,
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: Some(CostPreference::Balance),
                    prehook: None,
                    tty: false,
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
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };
        let spec = workflow_config_to_spec(&config);
        assert_eq!(
            spec.steps[0].cost_preference.as_deref(),
            Some("performance")
        );
        assert_eq!(spec.steps[1].cost_preference.as_deref(), Some("quality"));
        assert_eq!(spec.steps[2].cost_preference.as_deref(), Some("balance"));
    }

    #[test]
    fn workflow_config_to_spec_dynamic_steps_roundtrip() {
        let config = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "qa".to_string(),
                description: None,
                required_capability: None,
                template: None,
                builtin: None,
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
                max_parallel: None,
                timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Infinite,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: false,
                    stop_when_no_unresolved: false,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![crate::dynamic_orchestration::DynamicStepConfig {
                id: "ds1".to_string(),
                description: Some("dynamic".to_string()),
                step_type: "implement".to_string(),
                agent_id: Some("agent-x".to_string()),
                template: Some("tmpl-x".to_string()),
                trigger: Some("on_failure".to_string()),
                priority: 5,
                max_runs: Some(2),
            }],
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };
        let spec = workflow_config_to_spec(&config);
        assert_eq!(spec.loop_policy.mode, "infinite");
        assert_eq!(spec.dynamic_steps.len(), 1);
        assert_eq!(spec.dynamic_steps[0].id, "ds1");
        assert_eq!(spec.dynamic_steps[0].priority, 5);
        assert_eq!(spec.dynamic_steps[0].max_runs, Some(2));
    }

    #[test]
    fn workflow_config_to_spec_prehook_roundtrip() {
        let config = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "qa_testing".to_string(),
                description: None,
                required_capability: None,
                template: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: Some(StepPrehookConfig {
                    engine: StepHookEngine::Cel,
                    when: "is_last_cycle".to_string(),
                    reason: Some("deferred".to_string()),
                    ui: None,
                    extended: true,
                }),
                tty: false,
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
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };
        let spec = workflow_config_to_spec(&config);
        let prehook = spec.steps[0]
            .prehook
            .as_ref()
            .expect("prehook should round-trip");
        assert_eq!(prehook.engine, "cel");
        assert_eq!(prehook.when, "is_last_cycle");
        assert_eq!(prehook.reason.as_deref(), Some("deferred"));
        assert!(prehook.extended);
    }

    #[test]
    fn workflow_config_to_spec_finalize_rules() {
        let config = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "qa".to_string(),
                description: None,
                required_capability: None,
                template: None,
                builtin: None,
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
                max_parallel: None,
                timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig {
                rules: vec![
                    WorkflowFinalizeRule {
                        id: "r1".to_string(),
                        engine: StepHookEngine::Cel,
                        when: "qa_exit == 0".to_string(),
                        status: "passed".to_string(),
                        reason: Some("passed QA".to_string()),
                    },
                    WorkflowFinalizeRule {
                        id: "r2".to_string(),
                        engine: StepHookEngine::Cel,
                        when: "true".to_string(),
                        status: "failed".to_string(),
                        reason: None,
                    },
                ],
            },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };
        let spec = workflow_config_to_spec(&config);
        assert_eq!(spec.finalize.rules.len(), 2);
        assert_eq!(spec.finalize.rules[0].id, "r1");
        assert_eq!(spec.finalize.rules[0].engine, "cel");
        assert_eq!(spec.finalize.rules[0].reason.as_deref(), Some("passed QA"));
        assert_eq!(spec.finalize.rules[1].id, "r2");
        assert!(spec.finalize.rules[1].reason.is_none());
    }

    // ── safety_config_to_spec / checkpoint_strategy_as_str ──────────

    #[test]
    fn safety_config_to_spec_all_fields() {
        let config = SafetyConfig {
            max_consecutive_failures: 7,
            auto_rollback: true,
            checkpoint_strategy: CheckpointStrategy::GitTag,
            step_timeout_secs: Some(900),
            binary_snapshot: true,
            profile: WorkflowSafetyProfile::SelfReferentialProbe,
            ..SafetyConfig::default()
        };
        let spec = safety_config_to_spec(&config);
        assert_eq!(spec.max_consecutive_failures, 7);
        assert!(spec.auto_rollback);
        assert_eq!(spec.checkpoint_strategy, "git_tag");
        assert_eq!(spec.step_timeout_secs, Some(900));
        assert!(spec.binary_snapshot);
        assert_eq!(spec.profile.as_deref(), Some("self_referential_probe"));
    }

    #[test]
    fn safety_config_to_spec_defaults() {
        let config = SafetyConfig::default();
        let spec = safety_config_to_spec(&config);
        assert_eq!(spec.max_consecutive_failures, 3);
        assert!(!spec.auto_rollback);
        assert_eq!(spec.checkpoint_strategy, "none");
        assert!(spec.step_timeout_secs.is_none());
        assert!(!spec.binary_snapshot);
    }

    #[test]
    fn checkpoint_strategy_as_str_all_variants() {
        assert_eq!(
            checkpoint_strategy_as_str(&CheckpointStrategy::GitTag),
            "git_tag"
        );
        assert_eq!(
            checkpoint_strategy_as_str(&CheckpointStrategy::GitStash),
            "git_stash"
        );
        assert_eq!(
            checkpoint_strategy_as_str(&CheckpointStrategy::None),
            "none"
        );
    }

    #[test]
    fn workflow_safety_full_roundtrip() {
        let spec = WorkflowSpec {
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
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec {
                max_consecutive_failures: 7,
                auto_rollback: true,
                checkpoint_strategy: "git_tag".to_string(),
                step_timeout_secs: Some(900),
                binary_snapshot: true,
                profile: Some("self_referential_probe".to_string()),
                invariants: vec![],
                max_spawned_tasks: None,
                max_spawn_depth: None,
                spawn_cooldown_seconds: None,
            },
            max_parallel: None,
        };
        let config = workflow_spec_to_config(&spec).expect("spec->config should succeed");
        let roundtripped = workflow_config_to_spec(&config);
        assert_eq!(roundtripped.safety.max_consecutive_failures, 7);
        assert!(roundtripped.safety.auto_rollback);
        assert_eq!(roundtripped.safety.checkpoint_strategy, "git_tag");
        assert_eq!(roundtripped.safety.step_timeout_secs, Some(900));
        assert!(roundtripped.safety.binary_snapshot);
        assert_eq!(
            roundtripped.safety.profile.as_deref(),
            Some("self_referential_probe")
        );
    }

    #[test]
    fn workflow_config_to_spec_preserves_safety_fields() {
        let config = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "qa".to_string(),
                description: None,
                required_capability: None,
                template: None,
                builtin: None,
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
                max_parallel: None,
                timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: SafetyConfig {
                max_consecutive_failures: 5,
                auto_rollback: true,
                checkpoint_strategy: CheckpointStrategy::GitTag,
                step_timeout_secs: Some(600),
                binary_snapshot: true,
                profile: WorkflowSafetyProfile::SelfReferentialProbe,
                ..SafetyConfig::default()
            },
            max_parallel: None,
        };
        let spec = workflow_config_to_spec(&config);
        assert_eq!(spec.safety.max_consecutive_failures, 5);
        assert!(spec.safety.auto_rollback);
        assert_eq!(spec.safety.checkpoint_strategy, "git_tag");
        assert_eq!(spec.safety.step_timeout_secs, Some(600));
        assert!(spec.safety.binary_snapshot);
        assert_eq!(
            spec.safety.profile.as_deref(),
            Some("self_referential_probe")
        );
    }
}
