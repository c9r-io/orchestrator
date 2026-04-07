use super::*;
use crate::config::{
    CaptureDecl, CaptureSource, ConvergenceExprEntry, LoopMode, OrchestratorConfig, StepBehavior,
    StepHookEngine, StepScope, WorkflowConfig, WorkflowStepConfig,
};
use crate::config_load::tests::{
    make_builtin_step, make_command_step, make_config_with_agent, make_config_with_default_project,
    make_step, make_workflow,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
            },
        ],
        execution: Default::default(),
        loop_policy: crate::config::WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: crate::config::WorkflowLoopGuardConfig {
                enabled: false,
                ..crate::config::WorkflowLoopGuardConfig::default()
            },
            convergence_expr: None,
        },
        finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
        qa: None,
        fix: None,
        retest: None,
        dynamic_steps: vec![],
        adaptive: None,
        safety: crate::config::SafetyConfig::default(),
        max_parallel: None,
        stagger_delay_ms: None,
        item_isolation: None,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
            },
        ],
        execution: Default::default(),
        loop_policy: crate::config::WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: crate::config::WorkflowLoopGuardConfig {
                enabled: false,
                ..crate::config::WorkflowLoopGuardConfig::default()
            },
            convergence_expr: None,
        },
        finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
        qa: None,
        fix: None,
        retest: None,
        dynamic_steps: vec![],
        adaptive: None,
        safety: crate::config::SafetyConfig::default(),
        max_parallel: None,
        stagger_delay_ms: None,
        item_isolation: None,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
            },
        ],
        execution: Default::default(),
        loop_policy: crate::config::WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: crate::config::WorkflowLoopGuardConfig {
                enabled: false,
                ..crate::config::WorkflowLoopGuardConfig::default()
            },
            convergence_expr: None,
        },
        finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
        qa: None,
        fix: None,
        retest: None,
        dynamic_steps: vec![],
        adaptive: None,
        safety: crate::config::SafetyConfig::default(),
        max_parallel: None,
        stagger_delay_ms: None,
        item_isolation: None,
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
fn validate_workflow_config_rejects_json_path_on_exit_code_capture() {
    let workflow = WorkflowConfig {
        steps: vec![WorkflowStepConfig {
            id: "qa".to_string(),
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
            command: Some("echo benchmark".to_string()),
            chain_steps: vec![],
            scope: Some(StepScope::Task),
            behavior: StepBehavior {
                captures: vec![CaptureDecl {
                    var: "score".to_string(),
                    source: CaptureSource::ExitCode,
                    json_path: Some("$.total_score".to_string()),
                }],
                ..StepBehavior::default()
            },
            max_parallel: None,
            stagger_delay_ms: None,
            timeout_secs: None,
            stall_timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
            step_vars: None,
        }],
        execution: Default::default(),
        loop_policy: crate::config::WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: crate::config::WorkflowLoopGuardConfig {
                enabled: false,
                ..crate::config::WorkflowLoopGuardConfig::default()
            },
            convergence_expr: None,
        },
        finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
        qa: None,
        fix: None,
        retest: None,
        dynamic_steps: vec![],
        adaptive: None,
        safety: crate::config::SafetyConfig::default(),
        max_parallel: None,
        stagger_delay_ms: None,
        item_isolation: None,
    };

    let config = make_config_with_default_project();
    let err = validate_workflow_config(&config, &workflow, "test-workflow")
        .expect_err("json_path on exit_code should be rejected");

    assert!(
        err.to_string()
            .contains("uses json_path with unsupported source")
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
            stagger_delay_ms: None,
            timeout_secs: None,
            stall_timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
            step_vars: None,
        }],
        execution: Default::default(),
        loop_policy: crate::config::WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: crate::config::WorkflowLoopGuardConfig::default(),
            convergence_expr: None,
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
        stagger_delay_ms: None,
        item_isolation: None,
    };

    let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
    assert!(result.is_err(), "validation should fail without self_test");
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("self_ref.self_test_required")
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
            },
        ],
        execution: Default::default(),
        loop_policy: crate::config::WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: crate::config::WorkflowLoopGuardConfig::default(),
            convergence_expr: None,
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
        stagger_delay_ms: None,
        item_isolation: None,
    };

    let result = validate_self_referential_safety(&workflow, "test-workflow", "test-ws", true);
    assert!(result.is_ok(), "validation should pass with self_test step");
}

#[test]
fn validate_self_referential_safety_errors_without_checkpoint_strategy() {
    let workflow = WorkflowConfig {
        steps: vec![],
        execution: Default::default(),
        loop_policy: crate::config::WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: crate::config::WorkflowLoopGuardConfig::default(),
            convergence_expr: None,
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
        stagger_delay_ms: None,
        item_isolation: None,
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
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("at least one step")
    );
}

#[test]
fn validate_workflow_rejects_no_enabled_steps() {
    let workflow = make_workflow(vec![make_step("qa", false)]);
    let config = make_config_with_default_project();
    let result = validate_workflow_config(&config, &workflow, "test-wf");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("no enabled steps")
    );
}

#[test]
fn validate_workflow_rejects_missing_agent_template() {
    let workflow = make_workflow(vec![make_step("qa", true)]);
    let config = make_config_with_default_project();
    let result = validate_workflow_config(&config, &workflow, "test-wf");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("no agent supports capability")
    );
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
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("max_cycles must be > 0")
    );
}

#[test]
fn validate_workflow_rejects_fixed_without_max_cycles() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.mode = LoopMode::Fixed;
    workflow.loop_policy.guard.max_cycles = None;
    let config = make_config_with_default_project();
    let result = validate_workflow_config(&config, &workflow, "test-wf");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("loop.mode=fixed requires guard.max_cycles")
    );
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
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("no builtin loop_guard step or agent with loop_guard capability")
    );
}

#[test]
fn validate_workflow_accepts_guard_enabled_with_builtin_loop_guard_step() {
    let mut workflow = make_workflow(vec![
        make_builtin_step("self_test", "self_test", true),
        make_builtin_step("loop_guard", "loop_guard", true),
    ]);
    workflow.loop_policy.mode = LoopMode::Infinite;
    workflow.loop_policy.guard.enabled = true;
    let config = make_config_with_default_project();
    let result = validate_workflow_config(&config, &workflow, "test-wf");
    assert!(
        result.is_ok(),
        "builtin loop_guard step should satisfy guard requirement: {:?}",
        result.err()
    );
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
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("cannot define both builtin and required_capability")
    );
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
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("self_ref.auto_rollback_required")
    );
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
fn validate_workflow_config_rejects_invalid_dynamic_step_trigger_cel() {
    let mut workflow = make_workflow(vec![make_step("qa", true)]);
    workflow
        .dynamic_steps
        .push(crate::dynamic_orchestration::DynamicStepConfig {
            id: "dyn".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count >".to_string()),
            priority: 0,
            max_runs: None,
        });
    let config = make_config_with_default_project();
    validate_workflow_config(&config, &workflow, "wf-cel")
        .expect_err("invalid CEL trigger should fail");
}

#[test]
fn self_referential_probe_without_self_test_is_rejected() {
    let mut workflow = make_workflow(vec![make_command_step("implement", "echo probe")]);
    workflow.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
    workflow.safety.checkpoint_strategy = crate::config::CheckpointStrategy::GitTag;
    workflow.safety.auto_rollback = true;

    let result = validate_self_referential_safety(&workflow, "probe", "self-ref", true);
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("self_ref.self_test_required")
    );
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
        },
    );
    project.agents.insert(
        "agent1".to_string(),
        AgentConfig {
            enabled: true,
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
                enabled: true,
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
                enabled: true,
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

// ============================================================================
// Group 5: validate_execution_profiles_for_project()
// ============================================================================

#[test]
fn exec_profile_rejects_non_agent_step_with_profile() {
    use crate::config::ExecutionProfileConfig;
    let mut step = make_command_step("build", "cargo build");
    step.execution_profile = Some("sandboxed".to_string());

    let workflow = make_workflow(vec![step]);
    let mut config = make_config_with_default_project();
    let pid = crate::config::DEFAULT_PROJECT_ID;
    config
        .projects
        .get_mut(pid)
        .unwrap()
        .execution_profiles
        .insert("sandboxed".to_string(), ExecutionProfileConfig::default());

    let err = validate_execution_profiles_for_project(&config, &workflow, "wf1", pid)
        .expect_err("command step should reject profile");
    assert!(
        err.to_string().contains("only supported on agent steps"),
        "unexpected: {}",
        err
    );
}

#[test]
fn exec_profile_rejects_unknown_profile_name() {
    let mut step = make_step("qa", true);
    step.execution_profile = Some("nonexistent".to_string());

    let workflow = make_workflow(vec![step]);
    let config = make_config_with_agent("qa", "qa.md");
    let pid = crate::config::DEFAULT_PROJECT_ID;

    let err = validate_execution_profiles_for_project(&config, &workflow, "wf1", pid)
        .expect_err("unknown profile should fail");
    assert!(
        err.to_string().contains("unknown execution profile"),
        "unexpected: {}",
        err
    );
}

#[test]
fn exec_profile_rejects_host_mode_with_sandbox_fields() {
    use crate::config::{ExecutionProfileConfig, ExecutionProfileMode};
    let mut step = make_step("qa", true);
    step.execution_profile = Some("bad-host".to_string());

    let workflow = make_workflow(vec![step]);
    let mut config = make_config_with_agent("qa", "qa.md");
    let pid = crate::config::DEFAULT_PROJECT_ID;
    config
        .projects
        .get_mut(pid)
        .unwrap()
        .execution_profiles
        .insert(
            "bad-host".to_string(),
            ExecutionProfileConfig {
                mode: ExecutionProfileMode::Host,
                writable_paths: vec!["/tmp".to_string()],
                ..ExecutionProfileConfig::default()
            },
        );

    let err = validate_execution_profiles_for_project(&config, &workflow, "wf1", pid)
        .expect_err("host with sandbox fields should fail");
    assert!(
        err.to_string().contains("sandbox-only fields"),
        "unexpected: {}",
        err
    );
}

// FR-093: readable_paths is also a sandbox-only field; reject on host mode.
#[test]
fn exec_profile_rejects_host_mode_with_readable_paths() {
    use crate::config::{ExecutionProfileConfig, ExecutionProfileMode};
    let mut step = make_step("qa", true);
    step.execution_profile = Some("bad-host".to_string());

    let workflow = make_workflow(vec![step]);
    let mut config = make_config_with_agent("qa", "qa.md");
    let pid = crate::config::DEFAULT_PROJECT_ID;
    config
        .projects
        .get_mut(pid)
        .unwrap()
        .execution_profiles
        .insert(
            "bad-host".to_string(),
            ExecutionProfileConfig {
                mode: ExecutionProfileMode::Host,
                readable_paths: vec!["/shared/cache".to_string()],
                ..ExecutionProfileConfig::default()
            },
        );

    let err = validate_execution_profiles_for_project(&config, &workflow, "wf1", pid)
        .expect_err("host with readable_paths should fail");
    assert!(
        err.to_string().contains("sandbox-only fields"),
        "unexpected: {}",
        err
    );
}

#[test]
fn exec_profile_accepts_sandbox_mode_with_sandbox_fields() {
    use crate::config::{ExecutionProfileConfig, ExecutionProfileMode};
    let mut step = make_step("qa", true);
    step.execution_profile = Some("sandboxed".to_string());

    let workflow = make_workflow(vec![step]);
    let mut config = make_config_with_agent("qa", "qa.md");
    let pid = crate::config::DEFAULT_PROJECT_ID;
    config
        .projects
        .get_mut(pid)
        .unwrap()
        .execution_profiles
        .insert(
            "sandboxed".to_string(),
            ExecutionProfileConfig {
                mode: ExecutionProfileMode::Sandbox,
                writable_paths: vec!["/tmp".to_string()],
                max_memory_mb: Some(512),
                ..ExecutionProfileConfig::default()
            },
        );

    let result = validate_execution_profiles_for_project(&config, &workflow, "wf1", pid);
    assert!(
        result.is_ok(),
        "sandbox mode should allow sandbox fields: {:?}",
        result.err()
    );
}

#[test]
fn exec_profile_skips_step_without_profile() {
    let workflow = make_workflow(vec![make_step("qa", true)]);
    let config = make_config_with_agent("qa", "qa.md");
    let pid = crate::config::DEFAULT_PROJECT_ID;

    let result = validate_execution_profiles_for_project(&config, &workflow, "wf1", pid);
    assert!(
        result.is_ok(),
        "step without profile should pass: {:?}",
        result.err()
    );
}

#[test]
fn exec_profile_rejects_missing_project() {
    let workflow = make_workflow(vec![make_step("qa", true)]);
    let config = make_config_with_default_project();

    let err = validate_execution_profiles_for_project(&config, &workflow, "wf1", "nonexistent")
        .expect_err("missing project should fail");
    assert!(
        err.to_string().contains("project 'nonexistent' not found"),
        "unexpected: {}",
        err
    );
}

// ============================================================================
// Group 6: validate_adaptive_workflow_config() (owned agents)
// ============================================================================

#[test]
fn adaptive_none_returns_ok() {
    let workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    let agents: HashMap<String, crate::config::AgentConfig> = HashMap::new();
    let result = validate_adaptive_workflow_config(&workflow, "wf1", &agents);
    assert!(result.is_ok());
}

#[test]
fn adaptive_disabled_returns_ok() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: false,
        ..AdaptivePlannerConfig::default()
    });
    let agents: HashMap<String, crate::config::AgentConfig> = HashMap::new();
    let result = validate_adaptive_workflow_config(&workflow, "wf1", &agents);
    assert!(result.is_ok());
}

#[test]
fn adaptive_missing_planner_agent_errors() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: None,
        ..AdaptivePlannerConfig::default()
    });
    let agents: HashMap<String, crate::config::AgentConfig> = HashMap::new();
    let err = validate_adaptive_workflow_config(&workflow, "wf1", &agents)
        .expect_err("missing planner_agent should fail");
    assert!(
        err.to_string().contains("planner_agent is missing"),
        "unexpected: {}",
        err
    );
}

#[test]
fn adaptive_empty_planner_agent_errors() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: Some("  ".to_string()),
        ..AdaptivePlannerConfig::default()
    });
    let agents: HashMap<String, crate::config::AgentConfig> = HashMap::new();
    let err = validate_adaptive_workflow_config(&workflow, "wf1", &agents)
        .expect_err("whitespace-only planner_agent should fail");
    assert!(
        err.to_string().contains("planner_agent is missing"),
        "unexpected: {}",
        err
    );
}

#[test]
fn adaptive_unknown_agent_errors() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: Some("ghost".to_string()),
        ..AdaptivePlannerConfig::default()
    });
    let agents: HashMap<String, crate::config::AgentConfig> = HashMap::new();
    let err = validate_adaptive_workflow_config(&workflow, "wf1", &agents)
        .expect_err("unknown agent should fail");
    assert!(
        err.to_string().contains("unknown agent 'ghost'"),
        "unexpected: {}",
        err
    );
}

#[test]
fn adaptive_agent_missing_capability_errors() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: Some("planner".to_string()),
        ..AdaptivePlannerConfig::default()
    });
    let mut agents: HashMap<String, crate::config::AgentConfig> = HashMap::new();
    agents.insert(
        "planner".to_string(),
        crate::config::AgentConfig {
            enabled: true,
            capabilities: vec!["qa".to_string()],
            command: "echo plan".to_string(),
            ..crate::config::AgentConfig::default()
        },
    );
    let err = validate_adaptive_workflow_config(&workflow, "wf1", &agents)
        .expect_err("agent without adaptive_plan capability should fail");
    assert!(
        err.to_string()
            .contains("must support capability 'adaptive_plan'"),
        "unexpected: {}",
        err
    );
}

#[test]
fn adaptive_valid_config_passes() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: Some("planner".to_string()),
        ..AdaptivePlannerConfig::default()
    });
    let mut agents: HashMap<String, crate::config::AgentConfig> = HashMap::new();
    agents.insert(
        "planner".to_string(),
        crate::config::AgentConfig {
            enabled: true,
            capabilities: vec!["adaptive_plan".to_string()],
            command: "echo plan".to_string(),
            ..crate::config::AgentConfig::default()
        },
    );
    let result = validate_adaptive_workflow_config(&workflow, "wf1", &agents);
    assert!(
        result.is_ok(),
        "valid adaptive config should pass: {:?}",
        result.err()
    );
}

// ============================================================================
// Group 7: validate_adaptive_workflow_config_refs() (borrowed agents)
// ============================================================================

#[test]
fn adaptive_refs_none_returns_ok() {
    let workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_adaptive_workflow_config_refs(&workflow, "wf1", &agents);
    assert!(result.is_ok());
}

#[test]
fn adaptive_refs_disabled_returns_ok() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: false,
        ..AdaptivePlannerConfig::default()
    });
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_adaptive_workflow_config_refs(&workflow, "wf1", &agents);
    assert!(result.is_ok());
}

#[test]
fn adaptive_refs_missing_planner_agent_errors() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: None,
        ..AdaptivePlannerConfig::default()
    });
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_adaptive_workflow_config_refs(&workflow, "wf1", &agents)
        .expect_err("missing planner_agent should fail");
    assert!(err.to_string().contains("planner_agent is missing"));
}

#[test]
fn adaptive_refs_unknown_agent_errors() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: Some("ghost".to_string()),
        ..AdaptivePlannerConfig::default()
    });
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_adaptive_workflow_config_refs(&workflow, "wf1", &agents)
        .expect_err("unknown agent should fail");
    assert!(err.to_string().contains("unknown agent 'ghost'"));
}

#[test]
fn adaptive_refs_agent_missing_capability_errors() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: Some("planner".to_string()),
        ..AdaptivePlannerConfig::default()
    });
    let agent = crate::config::AgentConfig {
        enabled: true,
        capabilities: vec!["qa".to_string()],
        command: "echo plan".to_string(),
        ..crate::config::AgentConfig::default()
    };
    let mut agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    agents.insert("planner".to_string(), &agent);
    let err = validate_adaptive_workflow_config_refs(&workflow, "wf1", &agents)
        .expect_err("missing adaptive_plan capability should fail");
    assert!(
        err.to_string()
            .contains("must support capability 'adaptive_plan'")
    );
}

#[test]
fn adaptive_refs_valid_config_passes() {
    use crate::dynamic_orchestration::AdaptivePlannerConfig;
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.adaptive = Some(AdaptivePlannerConfig {
        enabled: true,
        planner_agent: Some("planner".to_string()),
        ..AdaptivePlannerConfig::default()
    });
    let agent = crate::config::AgentConfig {
        enabled: true,
        capabilities: vec!["adaptive_plan".to_string()],
        command: "echo plan".to_string(),
        ..crate::config::AgentConfig::default()
    };
    let mut agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    agents.insert("planner".to_string(), &agent);
    let result = validate_adaptive_workflow_config_refs(&workflow, "wf1", &agents);
    assert!(
        result.is_ok(),
        "valid refs config should pass: {:?}",
        result.err()
    );
}

// ============================================================================
// Group 8: validate_workflow_config_with_agents()
// ============================================================================

#[test]
fn with_agents_rejects_empty_steps() {
    let workflow = make_workflow(vec![]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("empty steps should fail");
    assert!(err.to_string().contains("at least one step"));
}

#[test]
fn with_agents_rejects_no_enabled_steps() {
    let workflow = make_workflow(vec![make_step("qa", false)]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("no enabled steps should fail");
    assert!(err.to_string().contains("no enabled steps"));
}

#[test]
fn with_agents_rejects_duplicate_step_ids() {
    let workflow = make_workflow(vec![
        make_builtin_step("dup", "self_test", true),
        make_builtin_step("dup", "self_test", true),
    ]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("duplicate step ids should fail");
    assert!(err.to_string().contains("duplicate step id 'dup'"));
}

#[test]
fn with_agents_rejects_missing_capability_agent() {
    let workflow = make_workflow(vec![make_step("qa", true)]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("missing agent should fail");
    assert!(err.to_string().contains("no agent supports capability"));
}

#[test]
fn with_agents_accepts_builtin_step() {
    let workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(result.is_ok(), "builtin should pass: {:?}", result.err());
}

#[test]
fn with_agents_accepts_command_step() {
    let workflow = make_workflow(vec![make_command_step("build", "cargo build")]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(result.is_ok(), "command should pass: {:?}", result.err());
}

#[test]
fn with_agents_accepts_agent_step_with_matching_agent() {
    let workflow = make_workflow(vec![make_step("qa", true)]);
    let agent = crate::config::AgentConfig {
        enabled: true,
        capabilities: vec!["qa".to_string()],
        command: "echo qa".to_string(),
        ..crate::config::AgentConfig::default()
    };
    let mut agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    agents.insert("qa-agent".to_string(), &agent);
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(
        result.is_ok(),
        "agent with capability should pass: {:?}",
        result.err()
    );
}

#[test]
fn with_agents_rejects_zero_max_cycles() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.guard.max_cycles = Some(0);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("zero max_cycles should fail");
    assert!(err.to_string().contains("max_cycles must be > 0"));
}

#[test]
fn with_agents_rejects_fixed_without_max_cycles() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.mode = LoopMode::Fixed;
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("fixed without max_cycles should fail");
    assert!(err.to_string().contains("loop.mode=fixed requires"));
}

#[test]
fn with_agents_rejects_guard_without_loop_guard_agent() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.mode = LoopMode::Infinite;
    workflow.loop_policy.guard.enabled = true;
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("guard without agent should fail");
    assert!(
        err.to_string()
            .contains("no builtin loop_guard step or agent with loop_guard capability")
    );
}

#[test]
fn with_agents_accepts_guard_with_builtin_loop_guard_step() {
    let mut workflow = make_workflow(vec![
        make_builtin_step("self_test", "self_test", true),
        make_builtin_step("loop_guard", "loop_guard", true),
    ]);
    workflow.loop_policy.mode = LoopMode::Infinite;
    workflow.loop_policy.guard.enabled = true;
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(
        result.is_ok(),
        "builtin loop_guard step should pass without agent: {:?}",
        result.err()
    );
}

#[test]
fn with_agents_accepts_guard_with_loop_guard_agent() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.mode = LoopMode::Infinite;
    workflow.loop_policy.guard.enabled = true;
    let agent = crate::config::AgentConfig {
        enabled: true,
        capabilities: vec!["loop_guard".to_string()],
        command: "echo guard".to_string(),
        ..crate::config::AgentConfig::default()
    };
    let mut agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    agents.insert("guard-agent".to_string(), &agent);
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(
        result.is_ok(),
        "guard with agent should pass: {:?}",
        result.err()
    );
}

#[test]
fn with_agents_skips_disabled_steps() {
    let workflow = make_workflow(vec![
        make_step("qa", false),
        make_builtin_step("self_test", "self_test", true),
    ]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(
        result.is_ok(),
        "disabled step should be skipped: {:?}",
        result.err()
    );
}

#[test]
fn with_agents_rejects_invalid_convergence_expr_cel() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.mode = LoopMode::Once;
    workflow.loop_policy.convergence_expr = Some(vec![ConvergenceExprEntry {
        engine: StepHookEngine::Cel,
        when: "this is not valid %%% CEL syntax !!!".to_string(),
        reason: Some("should fail".to_string()),
    }]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("invalid CEL in convergence_expr should fail");
    assert!(
        err.to_string().contains("invalid CEL"),
        "error should mention invalid CEL: {}",
        err
    );
}

#[test]
fn with_agents_accepts_valid_convergence_expr_cel() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.mode = LoopMode::Once;
    workflow.loop_policy.convergence_expr = Some(vec![ConvergenceExprEntry {
        engine: StepHookEngine::Cel,
        when: "cycle >= 2".to_string(),
        reason: Some("converged".to_string()),
    }]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(
        result.is_ok(),
        "valid CEL convergence_expr should pass: {:?}",
        result.err()
    );
}

#[test]
fn with_agents_rejects_empty_convergence_expr_when() {
    let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
    workflow.loop_policy.mode = LoopMode::Once;
    workflow.loop_policy.convergence_expr = Some(vec![ConvergenceExprEntry {
        engine: StepHookEngine::Cel,
        when: "   ".to_string(),
        reason: None,
    }]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err = validate_workflow_config_with_agents(&agents, &workflow, "wf1")
        .expect_err("empty when should fail");
    assert!(
        err.to_string().contains("empty"),
        "error should mention empty: {}",
        err
    );
}

#[test]
fn with_agents_allows_ticket_scan_without_agent() {
    let workflow = make_workflow(vec![
        make_step("ticket_scan", true),
        make_builtin_step("self_test", "self_test", true),
    ]);
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let result = validate_workflow_config_with_agents(&agents, &workflow, "wf1");
    assert!(
        result.is_ok(),
        "ticket_scan should not need agent: {:?}",
        result.err()
    );
}

// ============================================================================
// Group 9: validate_agent_env_store_refs_for_project()
// ============================================================================

#[test]
fn env_store_refs_project_missing_returns_ok() {
    let config = OrchestratorConfig::default();
    let result = validate_agent_env_store_refs_for_project(&config, "nonexistent");
    assert!(result.is_ok(), "missing project should return Ok");
}

#[test]
fn env_store_refs_project_valid_refs_pass() {
    use crate::cli_types::{AgentEnvEntry, AgentEnvRefValue};
    use crate::config::{AgentConfig, SecretStoreConfig};

    let mut config = OrchestratorConfig::default();
    let pid = "my-project";
    let project = config.projects.entry(pid.to_string()).or_default();
    project.secret_stores.insert(
        "vault".to_string(),
        SecretStoreConfig {
            data: [("SECRET".to_string(), "hidden".to_string())].into(),
        },
    );
    project.agents.insert(
        "agent1".to_string(),
        AgentConfig {
            enabled: true,
            env: Some(vec![
                AgentEnvEntry {
                    name: None,
                    value: None,
                    from_ref: Some("vault".to_string()),
                    ref_value: None,
                },
                AgentEnvEntry {
                    name: Some("SECRET_KEY".to_string()),
                    value: None,
                    from_ref: None,
                    ref_value: Some(AgentEnvRefValue {
                        name: "vault".to_string(),
                        key: "SECRET".to_string(),
                    }),
                },
            ]),
            ..AgentConfig::default()
        },
    );
    let result = validate_agent_env_store_refs_for_project(&config, pid);
    assert!(result.is_ok(), "valid refs should pass: {:?}", result.err());
}

#[test]
fn env_store_refs_project_unknown_from_ref_errors() {
    use crate::cli_types::AgentEnvEntry;
    use crate::config::AgentConfig;

    let mut config = OrchestratorConfig::default();
    let pid = "my-project";
    config
        .projects
        .entry(pid.to_string())
        .or_default()
        .agents
        .insert(
            "agent1".to_string(),
            AgentConfig {
                enabled: true,
                env: Some(vec![AgentEnvEntry {
                    name: None,
                    value: None,
                    from_ref: Some("missing-store".to_string()),
                    ref_value: None,
                }]),
                ..AgentConfig::default()
            },
        );
    let err = validate_agent_env_store_refs_for_project(&config, pid)
        .expect_err("unknown from_ref should fail");
    assert!(err.to_string().contains("unknown store"));
    assert!(err.to_string().contains("missing-store"));
}

#[test]
fn env_store_refs_project_unknown_ref_value_errors() {
    use crate::cli_types::{AgentEnvEntry, AgentEnvRefValue};
    use crate::config::AgentConfig;

    let mut config = OrchestratorConfig::default();
    let pid = "my-project";
    config
        .projects
        .entry(pid.to_string())
        .or_default()
        .agents
        .insert(
            "agent1".to_string(),
            AgentConfig {
                enabled: true,
                env: Some(vec![AgentEnvEntry {
                    name: Some("X".to_string()),
                    value: None,
                    from_ref: None,
                    ref_value: Some(AgentEnvRefValue {
                        name: "absent".to_string(),
                        key: "K".to_string(),
                    }),
                }]),
                ..AgentConfig::default()
            },
        );
    let err = validate_agent_env_store_refs_for_project(&config, pid)
        .expect_err("unknown ref_value store should fail");
    assert!(err.to_string().contains("unknown store"));
    assert!(err.to_string().contains("absent"));
}

#[test]
fn env_store_refs_project_no_env_passes() {
    use crate::config::AgentConfig;

    let mut config = OrchestratorConfig::default();
    let pid = "my-project";
    config
        .projects
        .entry(pid.to_string())
        .or_default()
        .agents
        .insert("basic".to_string(), AgentConfig::default());
    let result = validate_agent_env_store_refs_for_project(&config, pid);
    assert!(result.is_ok(), "no env should pass: {:?}", result.err());
}

// ============================================================================
// Consistency: both workflow validators produce the same error
// ============================================================================

#[test]
fn consistency_test_both_validators_same_error() {
    // A step requiring capability "fancy" with no agent providing it should
    // produce the same error from both validate_workflow_config_for_project
    // (via validate_workflow_config) and validate_workflow_config_with_agents.
    let workflow = make_workflow(vec![make_step("fancy", true)]);

    // Path 1: project-scoped validator (no agents in default project)
    let config = make_config_with_default_project();
    let err1 = validate_workflow_config(&config, &workflow, "wf-consistency")
        .expect_err("should fail without agent");

    // Path 2: agents-map validator (empty agents map)
    let agents: HashMap<String, &crate::config::AgentConfig> = HashMap::new();
    let err2 = validate_workflow_config_with_agents(&agents, &workflow, "wf-consistency")
        .expect_err("should fail without agent");

    assert_eq!(
        err1.to_string(),
        err2.to_string(),
        "both validators must produce identical error messages"
    );
}
