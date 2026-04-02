use super::cel::{evaluate_finalize_rule_expression, evaluate_step_prehook_expression};
use super::finalize::resolve_workflow_finalize_outcome;
use super::{validate_step_prehook, validate_workflow_finalize_rule};
use crate::config::{
    ItemFinalizeContext, StepHookEngine, StepPrehookConfig, StepPrehookContext,
    WorkflowFinalizeConfig, WorkflowFinalizeRule,
};

#[test]
fn test_validate_step_prehook_valid_cel() {
    let prehook = StepPrehookConfig {
        when: "active_ticket_count > 0".to_string(),
        reason: None,
        engine: StepHookEngine::Cel,
        ui: None,
        extended: false,
    };
    let result = validate_step_prehook(&prehook, "test-workflow", "qa");
    assert!(result.is_ok());
}

#[test]
fn test_validate_step_prehook_empty_expression() {
    let prehook = StepPrehookConfig {
        when: "".to_string(),
        reason: None,
        engine: StepHookEngine::Cel,
        ui: None,
        extended: false,
    };
    let result = validate_step_prehook(&prehook, "test-workflow", "qa");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("prehook.when cannot be empty")
    );
}

#[test]
fn test_validate_step_prehook_invalid_cel() {
    let prehook = StepPrehookConfig {
        when: "invalid cel expression @#$%".to_string(),
        reason: None,
        engine: StepHookEngine::Cel,
        ui: None,
        extended: false,
    };
    let result = validate_step_prehook(&prehook, "test-workflow", "qa");
    assert!(result.is_err());
}

#[test]
fn test_validate_workflow_finalize_rule_valid() {
    let rule = WorkflowFinalizeRule {
        id: "test-rule".to_string(),
        engine: StepHookEngine::Cel,
        when: "active_ticket_count == 0".to_string(),
        status: "skipped".to_string(),
        reason: Some("no tickets".to_string()),
    };
    let result = validate_workflow_finalize_rule(&rule, "test-workflow");
    assert!(result.is_ok());
}

#[test]
fn test_validate_workflow_finalize_rule_empty_id() {
    let rule = WorkflowFinalizeRule {
        id: "".to_string(),
        engine: StepHookEngine::Cel,
        when: "true".to_string(),
        status: "skipped".to_string(),
        reason: None,
    };
    let result = validate_workflow_finalize_rule(&rule, "test-workflow");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty id")
    );
}

#[test]
fn test_validate_workflow_finalize_rule_empty_status() {
    let rule = WorkflowFinalizeRule {
        id: "test-rule".to_string(),
        engine: StepHookEngine::Cel,
        when: "true".to_string(),
        status: "".to_string(),
        reason: None,
    };
    let result = validate_workflow_finalize_rule(&rule, "test-workflow");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty status")
    );
}

#[test]
fn test_validate_workflow_finalize_rule_empty_when() {
    let rule = WorkflowFinalizeRule {
        id: "test-rule".to_string(),
        engine: StepHookEngine::Cel,
        when: "".to_string(),
        status: "skipped".to_string(),
        reason: None,
    };
    let result = validate_workflow_finalize_rule(&rule, "test-workflow");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty when")
    );
}

#[test]
fn test_evaluate_step_prehook_expression_true() {
    let context = StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 1,
        step: "qa".to_string(),
        qa_file_path: "test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(1),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 5,
        new_ticket_count: 2,
        qa_failed: true,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: None,
        test_exit_code: None,
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 1,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        vars: Default::default(),
    };
    let result = evaluate_step_prehook_expression("active_ticket_count > 0", &context);
    assert!(result.is_ok());
    assert!(result.expect("expression should evaluate to true"));
}

#[test]
fn test_evaluate_step_prehook_expression_false() {
    let context = StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 1,
        step: "qa".to_string(),
        qa_file_path: "test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(0),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 0,
        new_ticket_count: 0,
        qa_failed: false,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: None,
        test_exit_code: None,
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 1,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        vars: Default::default(),
    };
    let result = evaluate_step_prehook_expression("active_ticket_count > 0", &context);
    assert!(result.is_ok());
    assert!(!result.expect("expression should evaluate to false"));
}

#[test]
fn test_evaluate_step_prehook_expression_invalid() {
    let context = StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 1,
        step: "qa".to_string(),
        qa_file_path: "test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(0),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 0,
        new_ticket_count: 0,
        qa_failed: false,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: None,
        test_exit_code: None,
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 1,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        vars: Default::default(),
    };
    let result = evaluate_step_prehook_expression("invalid @#$ expression", &context);
    assert!(result.is_err());
}

#[test]
fn test_evaluate_step_prehook_expression_qa_failed() {
    let context = StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 1,
        step: "fix".to_string(),
        qa_file_path: "test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(1),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 3,
        new_ticket_count: 1,
        qa_failed: true,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: None,
        test_exit_code: None,
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 1,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        vars: Default::default(),
    };
    let result = evaluate_step_prehook_expression("qa_failed == true", &context);
    assert!(result.is_ok());
    assert!(result.expect("qa_failed expression should be true"));
}

#[test]
fn test_evaluate_step_prehook_expression_compound() {
    let context = StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 2,
        step: "retest".to_string(),
        qa_file_path: "test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(0),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 2,
        new_ticket_count: 0,
        qa_failed: false,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: None,
        test_exit_code: None,
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 1,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        vars: Default::default(),
    };
    let result = evaluate_step_prehook_expression(
        "active_ticket_count > 0 && cycle >= 2 && qa_exit_code == 0",
        &context,
    );
    assert!(result.is_ok());
    assert!(result.expect("compound expression should be true"));
}

#[test]
fn test_build_errors_prehook_expression() {
    // Test the expression used by self-bootstrap fix step prehook
    let context_with_errors = StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 2,
        step: "fix".to_string(),
        qa_file_path: ".".to_string(),
        item_status: "build_failed".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: None,
        fix_exit_code: None,
        retest_exit_code: None,
        active_ticket_count: 0,
        new_ticket_count: 0,
        qa_failed: false,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 3,
        test_failure_count: 0,
        build_exit_code: Some(1),
        test_exit_code: Some(0),
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 1,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        vars: Default::default(),
    };
    let result = evaluate_step_prehook_expression(
        "build_errors > 0 || test_failures > 0",
        &context_with_errors,
    );
    assert!(result.is_ok());
    assert!(
        result.expect("build error expression should evaluate"),
        "should trigger fix when build errors exist"
    );

    let context_no_errors = StepPrehookContext {
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: Some(0),
        test_exit_code: Some(0),
        ..context_with_errors
    };
    let result = evaluate_step_prehook_expression(
        "build_errors > 0 || test_failures > 0",
        &context_no_errors,
    );
    assert!(result.is_ok());
    assert!(
        !result.expect("no-error expression should evaluate"),
        "should not trigger fix when no errors"
    );
}

// --- Helper to create a default StepPrehookContext ---
fn default_step_prehook_context() -> StepPrehookContext {
    StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 1,
        step: "qa".to_string(),
        qa_file_path: "test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(0),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 0,
        new_ticket_count: 0,
        qa_failed: false,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: None,
        test_exit_code: None,
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 1,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        vars: Default::default(),
    }
}

// --- Helper to create a default ItemFinalizeContext ---
fn default_item_finalize_context() -> crate::config::ItemFinalizeContext {
    crate::config::ItemFinalizeContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 1,
        qa_file_path: "qa.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(0),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 0,
        new_ticket_count: 0,
        retest_new_ticket_count: 0,
        qa_failed: false,
        fix_required: false,
        qa_configured: true,
        qa_observed: true,
        qa_enabled: true,
        qa_ran: true,
        qa_skipped: false,
        fix_configured: true,
        fix_enabled: true,
        fix_ran: false,
        fix_skipped: false,
        fix_success: false,
        retest_enabled: true,
        retest_ran: false,
        retest_success: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_confidence: None,
        fix_quality_score: None,
        total_artifacts: 0,
        has_ticket_artifacts: false,
        has_code_change_artifacts: false,
        is_last_cycle: true,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
    }
}

// --- Helper to create a default WorkflowFinalizeRule ---
fn make_rule(id: &str, when: &str, status: &str, reason: Option<&str>) -> WorkflowFinalizeRule {
    WorkflowFinalizeRule {
        id: id.to_string(),
        engine: StepHookEngine::Cel,
        when: when.to_string(),
        status: status.to_string(),
        reason: reason.map(String::from),
    }
}

#[test]
fn test_max_cycles_and_is_last_cycle_cel_variables() {
    let context = StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 1,
        step: "qa_testing".to_string(),
        qa_file_path: "test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: None,
        fix_exit_code: None,
        retest_exit_code: None,
        active_ticket_count: 0,
        new_ticket_count: 0,
        qa_failed: false,
        fix_required: false,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
        build_error_count: 0,
        test_failure_count: 0,
        build_exit_code: None,
        test_exit_code: None,
        self_test_exit_code: None,
        self_test_passed: false,
        max_cycles: 2,
        is_last_cycle: false,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        vars: Default::default(),
    };
    // cycle 1 of 2: not last cycle, skip qa_testing
    let result = evaluate_step_prehook_expression("is_last_cycle", &context);
    assert!(result.is_ok());
    assert!(!result.expect("is_last_cycle should be false"));

    let result = evaluate_step_prehook_expression("max_cycles == 2", &context);
    assert!(result.is_ok());
    assert!(result.expect("max_cycles expression should be true"));

    // cycle 2 of 2: is last cycle, run qa_testing
    let last_ctx = StepPrehookContext {
        cycle: 2,
        is_last_cycle: true,
        ..context
    };
    let result = evaluate_step_prehook_expression("is_last_cycle", &last_ctx);
    assert!(result.is_ok());
    assert!(result.expect("last cycle expression should be true"));
}

// ========================================================================
// self_referential_safe CEL variable
// ========================================================================

#[test]
fn test_self_referential_safe_cel_variable_true() {
    let context = StepPrehookContext {
        self_referential_safe: true,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("self_referential_safe", &context);
    assert!(result.is_ok());
    assert!(result.expect("self_referential_safe should be true"));
}

#[test]
fn test_self_referential_safe_cel_variable_false() {
    let context = StepPrehookContext {
        self_referential_safe: false,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("self_referential_safe", &context);
    assert!(result.is_ok());
    assert!(!result.expect("self_referential_safe should be false"));
}

#[test]
fn test_self_referential_safe_combined_with_is_last_cycle() {
    let context = StepPrehookContext {
        is_last_cycle: true,
        self_referential_safe: true,
        ..default_step_prehook_context()
    };
    let result =
        evaluate_step_prehook_expression("is_last_cycle && self_referential_safe", &context);
    assert!(result.is_ok());
    assert!(result.expect("combined expression should be true"));

    let unsafe_context = StepPrehookContext {
        is_last_cycle: true,
        self_referential_safe: false,
        ..default_step_prehook_context()
    };
    let result =
        evaluate_step_prehook_expression("is_last_cycle && self_referential_safe", &unsafe_context);
    assert!(result.is_ok());
    assert!(!result.expect("combined expression should be false when doc is unsafe"));
}

// ========================================================================
// self_referential_safe_scenarios CEL variable
// ========================================================================

#[test]
fn test_self_referential_safe_scenarios_empty() {
    let context = StepPrehookContext {
        self_referential_safe: false,
        self_referential_safe_scenarios: vec![],
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression(
        "self_referential_safe || size(self_referential_safe_scenarios) > 0",
        &context,
    );
    assert!(result.is_ok());
    assert!(!result.expect("should be false when unsafe with no safe scenarios"));
}

#[test]
fn test_self_referential_safe_scenarios_non_empty() {
    let context = StepPrehookContext {
        self_referential_safe: false,
        self_referential_safe_scenarios: vec!["S2".to_string(), "S3".to_string()],
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression(
        "self_referential_safe || size(self_referential_safe_scenarios) > 0",
        &context,
    );
    assert!(result.is_ok());
    assert!(result.expect("should be true when has safe scenarios"));
}

#[test]
fn test_self_referential_safe_scenarios_safe_doc_overrides() {
    let context = StepPrehookContext {
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression(
        "self_referential_safe || size(self_referential_safe_scenarios) > 0",
        &context,
    );
    assert!(result.is_ok());
    assert!(result.expect("should be true when doc is globally safe"));
}

// ========================================================================
// validate_step_prehook: additional edge cases
// ========================================================================

#[test]
fn test_validate_step_prehook_whitespace_only_expression() {
    let prehook = StepPrehookConfig {
        when: "   ".to_string(),
        reason: None,
        engine: StepHookEngine::Cel,
        ui: None,
        extended: false,
    };
    let result = validate_step_prehook(&prehook, "wf", "step");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("prehook.when cannot be empty")
    );
}

#[test]
fn test_validate_step_prehook_complex_valid_cel() {
    let prehook = StepPrehookConfig {
        when: "is_last_cycle && active_ticket_count > 0 || qa_failed == true".to_string(),
        reason: Some("complex condition".to_string()),
        engine: StepHookEngine::Cel,
        ui: None,
        extended: false,
    };
    let result = validate_step_prehook(&prehook, "wf", "qa_testing");
    assert!(result.is_ok());
}

// ========================================================================
// validate_workflow_finalize_rule: additional edge cases
// ========================================================================

#[test]
fn test_validate_workflow_finalize_rule_invalid_cel() {
    let rule = make_rule("bad-cel", "invalid @#$% expression", "failed", None);
    let result = validate_workflow_finalize_rule(&rule, "wf");
    assert!(result.is_err());
    let err_msg = result.expect_err("operation should fail").to_string();
    assert!(
        err_msg.contains("invalid CEL") || err_msg.contains("parser panic"),
        "expected CEL error, got: {}",
        err_msg
    );
}

#[test]
fn test_validate_workflow_finalize_rule_whitespace_id() {
    let rule = make_rule("  ", "true", "skipped", None);
    let result = validate_workflow_finalize_rule(&rule, "wf");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty id")
    );
}

#[test]
fn test_validate_workflow_finalize_rule_whitespace_status() {
    let rule = WorkflowFinalizeRule {
        id: "rule-1".to_string(),
        engine: StepHookEngine::Cel,
        when: "true".to_string(),
        status: "   ".to_string(),
        reason: None,
    };
    let result = validate_workflow_finalize_rule(&rule, "wf");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty status")
    );
}

#[test]
fn test_validate_workflow_finalize_rule_whitespace_when() {
    let rule = WorkflowFinalizeRule {
        id: "rule-1".to_string(),
        engine: StepHookEngine::Cel,
        when: "   ".to_string(),
        status: "skipped".to_string(),
        reason: None,
    };
    let result = validate_workflow_finalize_rule(&rule, "wf");
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty when")
    );
}

// ========================================================================
// evaluate_finalize_rule_expression: full coverage
// ========================================================================

#[test]
fn test_evaluate_finalize_rule_expression_true() {
    let rule = make_rule(
        "r1",
        "qa_skipped && active_ticket_count == 0",
        "skipped",
        None,
    );
    let context = ItemFinalizeContext {
        qa_skipped: true,
        active_ticket_count: 0,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("finalize rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_expression_false() {
    let rule = make_rule(
        "r1",
        "qa_skipped && active_ticket_count == 0",
        "skipped",
        None,
    );
    let context = ItemFinalizeContext {
        qa_skipped: false,
        active_ticket_count: 0,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(!result.expect("finalize rule should not match"));
}

#[test]
fn test_evaluate_finalize_rule_expression_invalid_cel() {
    let rule = make_rule("r1", "not valid @#$ cel", "failed", None);
    let context = default_item_finalize_context();
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_err());
}

#[test]
fn test_evaluate_finalize_rule_expression_non_bool_result() {
    // An expression that returns an integer instead of a bool
    let rule = make_rule("r1", "active_ticket_count + 1", "failed", None);
    let context = default_item_finalize_context();
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("must return bool")
    );
}

#[test]
fn test_evaluate_finalize_rule_qa_enabled_variables() {
    let rule = make_rule("r1", "qa_enabled && qa_ran && !qa_skipped", "passed", None);
    let context = ItemFinalizeContext {
        qa_enabled: true,
        qa_ran: true,
        qa_skipped: false,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("qa_enabled rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_fix_variables() {
    let rule = make_rule("r1", "fix_enabled && fix_ran && fix_success", "fixed", None);
    let context = ItemFinalizeContext {
        fix_enabled: true,
        fix_ran: true,
        fix_success: true,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("fix rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_retest_variables() {
    let rule = make_rule(
        "r1",
        "retest_enabled && retest_ran && retest_success",
        "verified",
        None,
    );
    let context = ItemFinalizeContext {
        retest_enabled: true,
        retest_ran: true,
        retest_success: true,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("retest rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_is_last_cycle() {
    let rule = make_rule(
        "r1",
        "qa_skipped && active_ticket_count == 0 && is_last_cycle",
        "skipped",
        None,
    );
    // Not last cycle -- rule should not match
    let context = ItemFinalizeContext {
        qa_skipped: true,
        active_ticket_count: 0,
        is_last_cycle: false,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(!result.expect("rule should not match before last cycle"));

    // Last cycle -- rule should match
    let context_last = ItemFinalizeContext {
        is_last_cycle: true,
        ..context
    };
    let result = evaluate_finalize_rule_expression(&rule, &context_last);
    assert!(result.is_ok());
    assert!(result.expect("rule should match on last cycle"));
}

#[test]
fn test_evaluate_finalize_rule_retest_new_ticket_count() {
    let rule = make_rule("r1", "retest_new_ticket_count > 0", "needs_review", None);
    let context = ItemFinalizeContext {
        retest_new_ticket_count: 3,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("retest_new_ticket_count rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_new_ticket_count() {
    let rule = make_rule("r1", "new_ticket_count > 0 && qa_failed", "failing", None);
    let context = ItemFinalizeContext {
        new_ticket_count: 5,
        qa_failed: true,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("new_ticket_count rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_exit_codes() {
    let rule = make_rule(
        "r1",
        "qa_exit_code == 1 && fix_exit_code == 0",
        "fixed",
        None,
    );
    let context = ItemFinalizeContext {
        qa_exit_code: Some(1),
        fix_exit_code: Some(0),
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("exit code rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_retest_exit_code() {
    let rule = make_rule("r1", "retest_exit_code == 0", "verified", None);
    let context = ItemFinalizeContext {
        retest_exit_code: Some(0),
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("retest exit code rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_fix_required() {
    let rule = make_rule("r1", "fix_required && !fix_ran", "needs_fix", None);
    let context = ItemFinalizeContext {
        fix_required: true,
        fix_ran: false,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("fix_required rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_task_and_item_ids() {
    let rule = make_rule(
        "r1",
        "task_id == \"my-task\" && task_item_id == \"my-item\"",
        "matched",
        None,
    );
    let context = ItemFinalizeContext {
        task_id: "my-task".to_string(),
        task_item_id: "my-item".to_string(),
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("task/item id rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_cycle_variable() {
    let rule = make_rule("r1", "cycle >= 2", "advanced", None);
    let context = ItemFinalizeContext {
        cycle: 3,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("cycle rule should match"));

    let context_early = ItemFinalizeContext {
        cycle: 1,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context_early);
    assert!(result.is_ok());
    assert!(!result.expect("early cycle rule should not match"));
}

#[test]
fn test_evaluate_finalize_rule_item_status_variable() {
    let rule = make_rule("r1", "item_status == \"completed\"", "done", None);
    let context = ItemFinalizeContext {
        item_status: "completed".to_string(),
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("item_status rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_task_status_variable() {
    let rule = make_rule("r1", "task_status == \"running\"", "active", None);
    let context = default_item_finalize_context();
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("task_status rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_qa_file_path_variable() {
    let rule = make_rule("r1", "qa_file_path == \"qa.md\"", "found", None);
    let context = default_item_finalize_context();
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("qa_file_path rule should match"));
}

// ========================================================================
// resolve_workflow_finalize_outcome: full coverage
// ========================================================================

#[test]
fn test_resolve_workflow_finalize_outcome_no_rules() {
    let finalize = WorkflowFinalizeConfig { rules: vec![] };
    let context = default_item_finalize_context();
    let result = resolve_workflow_finalize_outcome(&finalize, &context);
    assert!(result.is_ok());
    assert!(
        result
            .expect("finalize without rules should resolve")
            .is_none()
    );
}

#[test]
fn test_resolve_workflow_finalize_outcome_no_match() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![make_rule(
            "r1",
            "active_ticket_count > 100",
            "skipped",
            None,
        )],
    };
    let context = default_item_finalize_context();
    let result = resolve_workflow_finalize_outcome(&finalize, &context);
    assert!(result.is_ok());
    assert!(
        result
            .expect("finalize without matches should resolve")
            .is_none()
    );
}

#[test]
fn test_resolve_workflow_finalize_outcome_first_match_wins() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "true", "first_status", Some("first reason")),
            make_rule("r2", "true", "second_status", Some("second reason")),
        ],
    };
    let context = default_item_finalize_context();
    let outcome = resolve_workflow_finalize_outcome(&finalize, &context)
        .expect("finalize should resolve")
        .expect("first rule should match");
    assert_eq!(outcome.rule_id, "r1");
    assert_eq!(outcome.status, "first_status");
    assert_eq!(outcome.reason, "first reason");
}

#[test]
fn test_resolve_workflow_finalize_outcome_second_rule_matches() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "false", "skipped", Some("skip reason")),
            make_rule("r2", "true", "passed", Some("pass reason")),
        ],
    };
    let context = default_item_finalize_context();
    let outcome = resolve_workflow_finalize_outcome(&finalize, &context)
        .expect("finalize should resolve")
        .expect("second rule should match");
    assert_eq!(outcome.rule_id, "r2");
    assert_eq!(outcome.status, "passed");
    assert_eq!(outcome.reason, "pass reason");
}

#[test]
fn test_resolve_workflow_finalize_outcome_default_reason() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![make_rule("my-rule", "true", "done", None)],
    };
    let context = default_item_finalize_context();
    let outcome = resolve_workflow_finalize_outcome(&finalize, &context)
        .expect("finalize should resolve")
        .expect("default reason rule should match");
    assert_eq!(outcome.reason, "finalize rule 'my-rule' matched");
}

#[test]
fn test_resolve_workflow_finalize_outcome_complex_conditions() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule(
                "skip_without_tickets",
                "qa_skipped && active_ticket_count == 0 && is_last_cycle",
                "skipped",
                Some("QA skipped, no tickets"),
            ),
            make_rule(
                "qa_passed",
                "qa_ran && !qa_failed",
                "passed",
                Some("QA passed"),
            ),
            make_rule(
                "qa_failed_fixed",
                "qa_failed && fix_ran && fix_success && retest_success",
                "fixed",
                Some("Fixed and verified"),
            ),
        ],
    };

    // Case 1: QA skipped, last cycle, no tickets => skip_without_tickets
    let ctx1 = ItemFinalizeContext {
        qa_skipped: true,
        qa_ran: false,
        active_ticket_count: 0,
        is_last_cycle: true,
        ..default_item_finalize_context()
    };
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx1)
        .expect("ctx1 finalize should resolve")
        .expect("ctx1 should match first rule");
    assert_eq!(outcome.rule_id, "skip_without_tickets");

    // Case 2: QA skipped, NOT last cycle => skip rule doesn't match, qa_ran also false
    let ctx2 = ItemFinalizeContext {
        qa_skipped: true,
        qa_ran: false,
        active_ticket_count: 0,
        is_last_cycle: false,
        ..default_item_finalize_context()
    };
    let result =
        resolve_workflow_finalize_outcome(&finalize, &ctx2).expect("finalize should resolve");
    assert!(result.is_none());

    // Case 3: QA ran and passed => qa_passed
    let ctx3 = ItemFinalizeContext {
        qa_ran: true,
        qa_failed: false,
        qa_skipped: false,
        ..default_item_finalize_context()
    };
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx3)
        .expect("ctx3 finalize should resolve")
        .expect("ctx3 should match qa_passed");
    assert_eq!(outcome.rule_id, "qa_passed");

    // Case 4: QA failed, fix ran and succeeded, retest succeeded
    let ctx4 = ItemFinalizeContext {
        qa_ran: true,
        qa_failed: true,
        qa_skipped: false,
        fix_ran: true,
        fix_success: true,
        retest_success: true,
        ..default_item_finalize_context()
    };
    // First matching rule: qa_ran && !qa_failed is false, so check qa_failed_fixed
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx4)
        .expect("ctx4 finalize should resolve")
        .expect("ctx4 should match qa_failed_fixed");
    assert_eq!(outcome.rule_id, "qa_failed_fixed");
}

#[test]
fn test_fix_skipped_variable_available_in_cel_context() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![make_rule(
            "fix_skipped_check",
            "fix_enabled == true && fix_ran == false && fix_skipped == false && active_ticket_count > 0",
            "unresolved",
            Some("fix did not run"),
        )],
    };
    let ctx = ItemFinalizeContext {
        fix_enabled: true,
        fix_ran: false,
        fix_skipped: false,
        fix_success: false,
        active_ticket_count: 2,
        ..default_item_finalize_context()
    };
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx)
        .expect("fix_skipped CEL evaluation should succeed")
        .expect("should match fix_skipped_check");
    assert_eq!(outcome.rule_id, "fix_skipped_check");

    // When fix_skipped is true, the rule should NOT match
    let ctx_skipped = ItemFinalizeContext {
        fix_skipped: true,
        ..ctx
    };
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx_skipped)
        .expect("fix_skipped=true CEL evaluation should succeed");
    assert!(
        outcome.is_none(),
        "rule should not match when fix_skipped is true"
    );
}

#[test]
fn test_resolve_workflow_finalize_outcome_invalid_cel_returns_error() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![make_rule("bad", "not @#$ valid", "error", None)],
    };
    let context = default_item_finalize_context();
    let result = resolve_workflow_finalize_outcome(&finalize, &context);
    assert!(result.is_err());
}

// ========================================================================
// evaluate_step_prehook_expression: additional edge cases
// ========================================================================

#[test]
fn test_evaluate_step_prehook_expression_non_bool_result() {
    let context = default_step_prehook_context();
    // Expression returns an integer, not a bool
    let result = evaluate_step_prehook_expression("active_ticket_count + 1", &context);
    assert!(result.is_err());
    assert!(
        result
            .expect_err("operation should fail")
            .to_string()
            .contains("must return bool")
    );
}

#[test]
fn test_evaluate_step_prehook_expression_string_comparison() {
    let context = StepPrehookContext {
        item_status: "build_failed".to_string(),
        step: "fix".to_string(),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("item_status == \"build_failed\"", &context);
    assert!(result.is_ok());
    assert!(result.expect("string comparison should be true"));
}

#[test]
fn test_evaluate_step_prehook_expression_step_variable() {
    let context = StepPrehookContext {
        step: "qa_testing".to_string(),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("step == \"qa_testing\"", &context);
    assert!(result.is_ok());
    assert!(result.expect("step variable should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_task_id_variable() {
    let context = StepPrehookContext {
        task_id: "special-task".to_string(),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("task_id == \"special-task\"", &context);
    assert!(result.is_ok());
    assert!(result.expect("task_id should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_task_item_id_variable() {
    let context = StepPrehookContext {
        task_item_id: "item-42".to_string(),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("task_item_id == \"item-42\"", &context);
    assert!(result.is_ok());
    assert!(result.expect("task_item_id should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_qa_file_path_variable() {
    let context = StepPrehookContext {
        qa_file_path: "/tmp/qa_report.md".to_string(),
        ..default_step_prehook_context()
    };
    let result =
        evaluate_step_prehook_expression("qa_file_path == \"/tmp/qa_report.md\"", &context);
    assert!(result.is_ok());
    assert!(result.expect("qa_file_path should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_task_status_variable() {
    let context = StepPrehookContext {
        task_status: "paused".to_string(),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("task_status == \"paused\"", &context);
    assert!(result.is_ok());
    assert!(result.expect("task_status should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_new_ticket_count() {
    let context = StepPrehookContext {
        new_ticket_count: 7,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("new_ticket_count >= 5", &context);
    assert!(result.is_ok());
    assert!(result.expect("new_ticket_count should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_fix_required() {
    let context = StepPrehookContext {
        fix_required: true,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("fix_required", &context);
    assert!(result.is_ok());
    assert!(result.expect("fix_required should be true"));
}

#[test]
fn test_evaluate_step_prehook_expression_fix_exit_code() {
    let context = StepPrehookContext {
        fix_exit_code: Some(1),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("fix_exit_code == 1", &context);
    assert!(result.is_ok());
    assert!(result.expect("fix_exit_code should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_retest_exit_code() {
    let context = StepPrehookContext {
        retest_exit_code: Some(2),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("retest_exit_code == 2", &context);
    assert!(result.is_ok());
    assert!(result.expect("retest_exit_code should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_build_exit_code() {
    let context = StepPrehookContext {
        build_exit_code: Some(1),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("build_exit_code == 1", &context);
    assert!(result.is_ok());
    assert!(result.expect("build_exit_code should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_test_exit_code() {
    let context = StepPrehookContext {
        test_exit_code: Some(1),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("test_exit_code == 1", &context);
    assert!(result.is_ok());
    assert!(result.expect("test_exit_code should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_test_failures() {
    let context = StepPrehookContext {
        test_failure_count: 5,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("test_failures > 0", &context);
    assert!(result.is_ok());
    assert!(result.expect("test_failures should match"));
}

#[test]
fn test_evaluate_step_prehook_expression_literal_true() {
    let context = default_step_prehook_context();
    let result = evaluate_step_prehook_expression("true", &context);
    assert!(result.is_ok());
    assert!(result.expect("literal true should evaluate"));
}

#[test]
fn test_evaluate_step_prehook_expression_literal_false() {
    let context = default_step_prehook_context();
    let result = evaluate_step_prehook_expression("false", &context);
    assert!(result.is_ok());
    assert!(!result.expect("literal false should evaluate"));
}

#[test]
fn test_evaluate_step_prehook_expression_negation() {
    let context = StepPrehookContext {
        qa_failed: false,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("!qa_failed", &context);
    assert!(result.is_ok());
    assert!(result.expect("negation should evaluate true"));
}

#[test]
fn test_evaluate_step_prehook_expression_or_operator() {
    let context = StepPrehookContext {
        qa_failed: false,
        fix_required: true,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("qa_failed || fix_required", &context);
    assert!(result.is_ok());
    assert!(result.expect("or operator should evaluate true"));
}

#[test]
fn test_evaluate_step_prehook_expression_cycle_arithmetic() {
    let context = StepPrehookContext {
        cycle: 3,
        max_cycles: 5,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("cycle > 1 && cycle < max_cycles", &context);
    assert!(result.is_ok());
    assert!(result.expect("cycle arithmetic should evaluate true"));
}

// ========================================================================
// build_finalize_cel_context: exercising all variables
// ========================================================================

#[test]
fn test_evaluate_finalize_rule_all_bool_flags_false() {
    let rule = make_rule(
        "r1",
        "!qa_enabled && !qa_ran && !qa_skipped && !fix_enabled && !fix_ran && !fix_success && !retest_enabled && !retest_ran && !retest_success && !qa_failed && !fix_required",
        "none",
        None,
    );
    let context = ItemFinalizeContext {
        qa_enabled: false,
        qa_ran: false,
        qa_skipped: false,
        fix_enabled: false,
        fix_ran: false,
        fix_success: false,
        retest_enabled: false,
        retest_ran: false,
        retest_success: false,
        qa_failed: false,
        fix_required: false,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("all false flags rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_all_bool_flags_true() {
    let rule = make_rule(
        "r1",
        "qa_enabled && qa_ran && qa_skipped && fix_enabled && fix_ran && fix_success && retest_enabled && retest_ran && retest_success && qa_failed && fix_required",
        "all_true",
        None,
    );
    let context = ItemFinalizeContext {
        qa_enabled: true,
        qa_ran: true,
        qa_skipped: true,
        fix_enabled: true,
        fix_ran: true,
        fix_success: true,
        retest_enabled: true,
        retest_ran: true,
        retest_success: true,
        qa_failed: true,
        fix_required: true,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("all true flags rule should match"));
}

// ========================================================================
// resolve_workflow_finalize_outcome: multiple rules, none match
// ========================================================================

#[test]
fn test_resolve_workflow_finalize_outcome_all_false() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "false", "a", None),
            make_rule("r2", "false", "b", None),
            make_rule("r3", "false", "c", None),
        ],
    };
    let context = default_item_finalize_context();
    let result = resolve_workflow_finalize_outcome(&finalize, &context)
        .expect("finalize all-false should resolve");
    assert!(result.is_none());
}

#[test]
fn test_resolve_workflow_finalize_outcome_third_rule_matches() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "false", "first", None),
            make_rule("r2", "false", "second", None),
            make_rule("r3", "true", "third", Some("third wins")),
        ],
    };
    let context = default_item_finalize_context();
    let outcome = resolve_workflow_finalize_outcome(&finalize, &context)
        .expect("finalize third-rule should resolve")
        .expect("third rule should match");
    assert_eq!(outcome.rule_id, "r3");
    assert_eq!(outcome.status, "third");
    assert_eq!(outcome.reason, "third wins");
}

// ========================================================================
// validate_step_prehook: with reason set
// ========================================================================

#[test]
fn test_validate_step_prehook_with_reason() {
    let prehook = StepPrehookConfig {
        when: "is_last_cycle".to_string(),
        reason: Some("Only run on last cycle".to_string()),
        engine: StepHookEngine::Cel,
        ui: None,
        extended: false,
    };
    let result = validate_step_prehook(&prehook, "wf", "qa_testing");
    assert!(result.is_ok());
}

// ========================================================================
// validate_workflow_finalize_rule: with reason set
// ========================================================================

#[test]
fn test_validate_workflow_finalize_rule_with_reason() {
    let rule = WorkflowFinalizeRule {
        id: "rule-with-reason".to_string(),
        engine: StepHookEngine::Cel,
        when: "qa_failed && active_ticket_count > 0".to_string(),
        status: "needs_fix".to_string(),
        reason: Some("QA failures found with active tickets".to_string()),
    };
    let result = validate_workflow_finalize_rule(&rule, "wf");
    assert!(result.is_ok());
}

// ========================================================================
// evaluate_step_prehook_expression: self_test_passed variable
// ========================================================================

#[test]
fn test_evaluate_step_prehook_expression_self_test_passed_not_in_cel() {
    // self_test_passed is a field on StepPrehookContext but is NOT added
    // as a CEL variable in build_step_prehook_cel_context.
    // Attempting to use it should fail at execution time.
    let context = StepPrehookContext {
        self_test_passed: true,
        ..default_step_prehook_context()
    };
    // The expression references a variable not in the CEL context
    let result = evaluate_step_prehook_expression("self_test_passed == true", &context);
    // This should either error or return false depending on CEL semantics
    // The important thing is it doesn't panic
    assert!(result.is_err() || !result.expect("self_test_passed expression should evaluate"));
}

// ── resolve_workflow_finalize_outcome: multi-rule matching ────────

#[test]
fn test_resolve_workflow_finalize_outcome_no_rules_returns_none() {
    let finalize = WorkflowFinalizeConfig { rules: vec![] };
    let context = default_item_finalize_context();
    let result = resolve_workflow_finalize_outcome(&finalize, &context);
    assert!(result.is_ok());
    assert!(result.expect("should succeed").is_none());
}

#[test]
fn test_resolve_workflow_finalize_outcome_first_false_second_matches() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule(
                "r1",
                "qa_skipped && active_ticket_count > 0",
                "skipped",
                None,
            ),
            make_rule(
                "r2",
                "qa_ran && active_ticket_count == 0",
                "resolved",
                Some("qa passed"),
            ),
        ],
    };
    let context = ItemFinalizeContext {
        qa_ran: true,
        qa_skipped: false,
        active_ticket_count: 0,
        ..default_item_finalize_context()
    };
    let result = resolve_workflow_finalize_outcome(&finalize, &context).expect("should succeed");
    let outcome = result.expect("should match second rule");
    assert_eq!(outcome.rule_id, "r2");
    assert_eq!(outcome.status, "resolved");
    assert_eq!(outcome.reason, "qa passed");
}

#[test]
fn test_resolve_workflow_finalize_outcome_none_match() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "qa_skipped", "skipped", None),
            make_rule("r2", "fix_ran && fix_success", "resolved", None),
        ],
    };
    let context = ItemFinalizeContext {
        qa_skipped: false,
        fix_ran: false,
        fix_success: false,
        ..default_item_finalize_context()
    };
    let result = resolve_workflow_finalize_outcome(&finalize, &context).expect("should succeed");
    assert!(result.is_none());
}

// ── evaluate_finalize_rule_expression: non-bool return ───────────

#[test]
fn test_evaluate_finalize_rule_expression_non_bool_return() {
    let rule = make_rule("r1", "active_ticket_count + 1", "resolved", None);
    let context = default_item_finalize_context();
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("must return bool"));
}

// ── evaluate_finalize_rule_expression: additional variables ──────

#[test]
fn test_evaluate_finalize_rule_retest_new_ticket_count_positive() {
    let rule = make_rule("r1", "retest_new_ticket_count == 3", "unresolved", None);
    let context = ItemFinalizeContext {
        retest_new_ticket_count: 3,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_retest_enabled_and_success() {
    let rule = make_rule(
        "r1",
        "retest_enabled && retest_ran && retest_success",
        "resolved",
        None,
    );
    let context = ItemFinalizeContext {
        retest_enabled: true,
        retest_ran: true,
        retest_success: true,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_fix_skipped_variable() {
    let rule = make_rule("r1", "fix_skipped && !fix_ran", "skipped", None);
    let context = ItemFinalizeContext {
        fix_skipped: true,
        fix_ran: false,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_fix_configured_variable() {
    let rule = make_rule("r1", "fix_configured && !fix_enabled", "pending", None);
    let context = ItemFinalizeContext {
        fix_configured: true,
        fix_enabled: false,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_qa_enabled_and_observed() {
    let rule = make_rule(
        "r1",
        "qa_enabled && qa_observed && !qa_failed",
        "resolved",
        None,
    );
    let context = ItemFinalizeContext {
        qa_enabled: true,
        qa_observed: true,
        qa_failed: false,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("rule should match"));
}

#[test]
fn test_evaluate_finalize_rule_is_last_cycle_with_qa_ran() {
    let rule = make_rule("r1", "is_last_cycle && qa_ran", "resolved", None);
    let context = ItemFinalizeContext {
        is_last_cycle: true,
        qa_ran: true,
        ..default_item_finalize_context()
    };
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.expect("rule should match"));
}

// ── evaluate_step_prehook_expression: additional coverage ────────

#[test]
fn test_evaluate_step_prehook_expression_confidence_and_quality() {
    // qa_confidence is accessible via context.qa_confidence
    let context = StepPrehookContext {
        qa_confidence: Some(0.85),
        qa_quality_score: Some(0.9),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("cycle == 1", &context);
    assert!(result.is_ok());
    assert!(result.expect("should evaluate to true"));
}

#[test]
fn test_evaluate_step_prehook_expression_build_errors_positive() {
    let context = StepPrehookContext {
        build_error_count: 5,
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("build_errors > 0", &context);
    assert!(result.is_ok());
    assert!(result.expect("should evaluate to true"));
}

#[test]
fn test_evaluate_step_prehook_expression_item_status_comparison() {
    let context = StepPrehookContext {
        item_status: "resolved".to_string(),
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression("item_status == \"resolved\"", &context);
    assert!(result.is_ok());
    assert!(result.expect("should evaluate to true"));
}

// ── validate_step_prehook: edge cases ────────────────────────────

#[test]
fn test_validate_step_prehook_only_whitespace_is_rejected() {
    let prehook = StepPrehookConfig {
        when: "   ".to_string(),
        reason: None,
        engine: StepHookEngine::Cel,
        ui: None,
        extended: false,
    };
    let result = validate_step_prehook(&prehook, "wf1", "step1");
    assert!(result.is_err());
}

#[test]
fn test_validate_workflow_finalize_rule_whitespace_when_rejected() {
    let rule = WorkflowFinalizeRule {
        id: "r1".to_string(),
        engine: StepHookEngine::Cel,
        when: "   ".to_string(),
        status: "resolved".to_string(),
        reason: None,
    };
    let result = validate_workflow_finalize_rule(&rule, "wf1");
    assert!(result.is_err());
}

// ── CEL context coverage: exercise all StepPrehookContext variables ──

fn make_prehook_ctx() -> StepPrehookContext {
    StepPrehookContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 2,
        step: "qa_testing".to_string(),
        qa_file_path: "docs/qa/test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(1),
        fix_exit_code: Some(0),
        retest_exit_code: Some(2),
        active_ticket_count: 3,
        new_ticket_count: 1,
        qa_failed: true,
        fix_required: true,
        qa_confidence: Some(0.85),
        qa_quality_score: Some(0.9),
        fix_has_changes: Some(true),
        upstream_artifacts: vec![],
        build_error_count: 2,
        test_failure_count: 3,
        build_exit_code: Some(0),
        test_exit_code: Some(1),
        self_test_exit_code: Some(0),
        self_test_passed: true,
        max_cycles: 3,
        is_last_cycle: false,
        self_referential_safe: true,
        self_referential_safe_scenarios: vec![],
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
        vars: Default::default(),
    }
}

#[test]
fn test_prehook_cel_context_cycle_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("cycle == 2", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_max_cycles_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("max_cycles == 3", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_is_last_cycle_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("is_last_cycle == false", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_step_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("step == 'qa_testing'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_qa_file_path_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("qa_file_path == 'docs/qa/test.md'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_item_status_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("item_status == 'pending'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_task_status_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("task_status == 'running'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_build_errors_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("build_errors == 2", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_test_failures_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("test_failures == 3", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_build_exit_code_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("build_exit_code == 0", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_test_exit_code_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("test_exit_code == 1", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_self_referential_safe_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("self_referential_safe == true", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_task_id_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("task_id == 'task-1'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_task_item_id_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("task_item_id == 'item-1'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_fix_required_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("fix_required == true", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_retest_exit_code_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("retest_exit_code == 2", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_qa_confidence_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("qa_confidence > 0.8", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_qa_quality_score_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("qa_quality_score > 0.89", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_fix_has_changes_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("fix_has_changes == true", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_self_test_exit_code_variable() {
    let ctx = make_prehook_ctx();
    let result = evaluate_step_prehook_expression("self_test_exit_code == 0", &ctx);
    assert!(result.unwrap());
}

// ── CEL finalize context coverage: exercise ItemFinalizeContext variables ──

fn make_finalize_ctx() -> ItemFinalizeContext {
    ItemFinalizeContext {
        task_id: "task-1".to_string(),
        task_item_id: "item-1".to_string(),
        cycle: 2,
        qa_file_path: "docs/qa/test.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: Some(1),
        fix_exit_code: Some(0),
        retest_exit_code: Some(0),
        active_ticket_count: 2,
        new_ticket_count: 1,
        retest_new_ticket_count: 0,
        qa_failed: true,
        fix_required: true,
        qa_configured: true,
        qa_observed: true,
        qa_enabled: true,
        qa_ran: true,
        qa_skipped: false,
        fix_configured: true,
        fix_enabled: true,
        fix_ran: true,
        fix_skipped: false,
        fix_success: true,
        retest_enabled: true,
        retest_ran: true,
        retest_success: false,
        qa_confidence: Some(0.85),
        qa_quality_score: Some(0.9),
        fix_confidence: Some(0.7),
        fix_quality_score: Some(0.8),
        total_artifacts: 5,
        has_ticket_artifacts: true,
        has_code_change_artifacts: true,
        is_last_cycle: false,
        last_sandbox_denied: false,
        sandbox_denied_count: 0,
        last_sandbox_denial_reason: None,
    }
}

#[test]
fn test_finalize_cel_context_qa_configured_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "qa_configured == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_qa_observed_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "qa_observed == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_qa_enabled_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "qa_enabled == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_qa_ran_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "qa_ran == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_qa_skipped_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "qa_skipped == false", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_fix_configured_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "fix_configured == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_fix_enabled_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "fix_enabled == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_fix_ran_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "fix_ran == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_fix_skipped_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "fix_skipped == false", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_fix_success_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "fix_success == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_retest_enabled_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "retest_enabled == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_retest_ran_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "retest_ran == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_retest_success_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "retest_success == false", "unresolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_qa_confidence_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "qa_confidence > 0.8", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_qa_quality_score_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "qa_quality_score > 0.89", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_fix_confidence_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "fix_confidence > 0.69", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_fix_quality_score_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "fix_quality_score > 0.79", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_total_artifacts_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "total_artifacts == 5", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_has_ticket_artifacts_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "has_ticket_artifacts == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_has_code_change_artifacts_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "has_code_change_artifacts == true", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_retest_new_ticket_count_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "retest_new_ticket_count == 0", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_is_last_cycle_variable() {
    let ctx = make_finalize_ctx();
    let rule = make_rule("r1", "is_last_cycle == false", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

// ── resolve_workflow_finalize_outcome tests ────────────────────────

#[test]
fn test_resolve_finalize_outcome_empty_rules() {
    let config = WorkflowFinalizeConfig { rules: vec![] };
    let ctx = make_finalize_ctx();
    let result = resolve_workflow_finalize_outcome(&config, &ctx).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_resolve_finalize_outcome_first_matching_rule_wins() {
    let config = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "qa_failed == true", "unresolved", None),
            make_rule("r2", "qa_failed == true", "resolved", None),
        ],
    };
    let ctx = make_finalize_ctx();
    let outcome = resolve_workflow_finalize_outcome(&config, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(outcome.rule_id, "r1");
    assert_eq!(outcome.status, "unresolved");
}

#[test]
fn test_resolve_finalize_outcome_skips_non_matching_rules() {
    let config = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "qa_failed == false", "skipped", None),
            make_rule("r2", "qa_failed == true", "unresolved", None),
        ],
    };
    let ctx = make_finalize_ctx();
    let outcome = resolve_workflow_finalize_outcome(&config, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(outcome.rule_id, "r2");
    assert_eq!(outcome.status, "unresolved");
}

#[test]
fn test_resolve_finalize_outcome_custom_reason() {
    let config = WorkflowFinalizeConfig {
        rules: vec![make_rule(
            "r1",
            "qa_failed == true",
            "unresolved",
            Some("QA detected failures"),
        )],
    };
    let ctx = make_finalize_ctx();
    let outcome = resolve_workflow_finalize_outcome(&config, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(outcome.reason, "QA detected failures");
}

#[test]
fn test_resolve_finalize_outcome_default_reason() {
    let config = WorkflowFinalizeConfig {
        rules: vec![make_rule("r1", "qa_failed == true", "unresolved", None)],
    };
    let ctx = make_finalize_ctx();
    let outcome = resolve_workflow_finalize_outcome(&config, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(outcome.reason, "finalize rule 'r1' matched");
}

#[test]
fn test_resolve_finalize_outcome_no_rules_match() {
    let config = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "qa_failed == false", "resolved", None),
            make_rule("r2", "active_ticket_count == 0", "resolved", None),
        ],
    };
    let ctx = make_finalize_ctx();
    let result = resolve_workflow_finalize_outcome(&config, &ctx).unwrap();
    assert!(result.is_none());
}

// ── CEL sandbox variable coverage: exercise sandbox fields in prehook context ──

#[test]
fn test_prehook_cel_context_last_sandbox_denied_true() {
    let mut ctx = make_prehook_ctx();
    ctx.last_sandbox_denied = true;
    let result = evaluate_step_prehook_expression("last_sandbox_denied == true", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_sandbox_denied_count_nonzero() {
    let mut ctx = make_prehook_ctx();
    ctx.sandbox_denied_count = 5;
    let result = evaluate_step_prehook_expression("sandbox_denied_count == 5", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_last_sandbox_denial_reason_set() {
    let mut ctx = make_prehook_ctx();
    ctx.last_sandbox_denial_reason = Some("permission denied".to_string());
    let result =
        evaluate_step_prehook_expression("last_sandbox_denial_reason == 'permission denied'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_last_sandbox_denial_reason_none() {
    let ctx = make_prehook_ctx();
    // When None, cel-interpreter registers the value as null (not empty string)
    let result = evaluate_step_prehook_expression("last_sandbox_denial_reason == null", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_cel_context_sandbox_combined_expression() {
    let mut ctx = make_prehook_ctx();
    ctx.last_sandbox_denied = true;
    ctx.sandbox_denied_count = 3;
    ctx.last_sandbox_denial_reason = Some("network blocked".to_string());
    let result = evaluate_step_prehook_expression(
        "last_sandbox_denied && sandbox_denied_count > 2 && last_sandbox_denial_reason == 'network blocked'",
        &ctx,
    );
    assert!(result.unwrap());
}

// ── CEL sandbox variable coverage: exercise sandbox fields in finalize context ──

#[test]
fn test_finalize_cel_context_last_sandbox_denied_true() {
    let mut ctx = make_finalize_ctx();
    ctx.last_sandbox_denied = true;
    let rule = make_rule("r1", "last_sandbox_denied == true", "blocked", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_sandbox_denied_count_nonzero() {
    let mut ctx = make_finalize_ctx();
    ctx.sandbox_denied_count = 7;
    let rule = make_rule("r1", "sandbox_denied_count == 7", "blocked", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_last_sandbox_denial_reason_set() {
    let mut ctx = make_finalize_ctx();
    ctx.last_sandbox_denial_reason = Some("fs write denied".to_string());
    let rule = make_rule(
        "r1",
        "last_sandbox_denial_reason == 'fs write denied'",
        "blocked",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_last_sandbox_denial_reason_none() {
    let ctx = make_finalize_ctx();
    // When None, cel-interpreter registers the value as null (not empty string)
    let rule = make_rule("r1", "last_sandbox_denial_reason == null", "resolved", None);
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_cel_context_sandbox_combined_expression() {
    let mut ctx = make_finalize_ctx();
    ctx.last_sandbox_denied = true;
    ctx.sandbox_denied_count = 2;
    ctx.last_sandbox_denial_reason = Some("process limit".to_string());
    let rule = make_rule(
        "r1",
        "last_sandbox_denied && sandbox_denied_count >= 2",
        "blocked",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

// ── Pipeline variable injection tests ──

#[test]
fn test_prehook_pipeline_var_string() {
    let mut ctx = make_prehook_ctx();
    ctx.vars.insert("my_var".to_string(), "hello".to_string());
    let result = evaluate_step_prehook_expression("my_var == 'hello'", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_pipeline_var_int() {
    let mut ctx = make_prehook_ctx();
    ctx.vars.insert("my_count".to_string(), "42".to_string());
    let result = evaluate_step_prehook_expression("my_count > 10", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_pipeline_var_bool() {
    let mut ctx = make_prehook_ctx();
    ctx.vars
        .insert("feature_on".to_string(), "true".to_string());
    let result = evaluate_step_prehook_expression("feature_on", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_pipeline_var_float() {
    let mut ctx = make_prehook_ctx();
    ctx.vars.insert("score".to_string(), "3.14".to_string());
    let result = evaluate_step_prehook_expression("score > 3.0", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_pipeline_var_json_array_in_operator() {
    let mut ctx = make_prehook_ctx();
    ctx.vars.insert(
        "regression_target_ids".to_string(),
        r#"["docs/qa/test.md","docs/qa/other.md"]"#.to_string(),
    );
    let result = evaluate_step_prehook_expression("qa_file_path in regression_target_ids", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_prehook_pipeline_var_json_array_not_in() {
    let mut ctx = make_prehook_ctx();
    ctx.vars.insert(
        "regression_target_ids".to_string(),
        r#"["docs/qa/other.md"]"#.to_string(),
    );
    let result = evaluate_step_prehook_expression("qa_file_path in regression_target_ids", &ctx);
    assert!(!result.unwrap());
}

#[test]
fn test_prehook_pipeline_var_truncated_skipped() {
    let mut ctx = make_prehook_ctx();
    ctx.vars.insert(
        "big_var".to_string(),
        "partial...\n[truncated — full content at /tmp/spill.txt]".to_string(),
    );
    // big_var should not be in the CEL context — expression referencing it should fail
    let result = evaluate_step_prehook_expression("big_var == 'anything'", &ctx);
    assert!(result.is_err());
}

#[test]
fn test_prehook_pipeline_var_builtin_takes_precedence() {
    let mut ctx = make_prehook_ctx();
    // Try to override built-in `cycle` — built-in should win (cycle == 2 from make_prehook_ctx)
    ctx.vars.insert("cycle".to_string(), "999".to_string());
    let result = evaluate_step_prehook_expression("cycle == 2", &ctx);
    assert!(result.unwrap());
}

// ── validate_agent_command_rules ────────────────────────────────────

#[test]
fn validate_command_rules_empty_is_ok() {
    let result = super::validate_agent_command_rules("ag1", &[]);
    assert!(result.is_ok());
}

#[test]
fn validate_command_rules_valid_cel() {
    use crate::config::AgentCommandRule;
    let rules = vec![AgentCommandRule {
        when: "vars.loop_session_id != \"\"".to_string(),
        command: "claude --resume {loop_session_id} -p \"{prompt}\"".to_string(),
    }];
    let result = super::validate_agent_command_rules("ag1", &rules);
    assert!(result.is_ok());
}

#[test]
fn validate_command_rules_invalid_cel() {
    use crate::config::AgentCommandRule;
    let rules = vec![AgentCommandRule {
        when: "vars.x !!!".to_string(),
        command: "echo \"{prompt}\"".to_string(),
    }];
    let result = super::validate_agent_command_rules("ag1", &rules);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid CEL"));
}

#[test]
fn validate_command_rules_empty_when_rejected() {
    use crate::config::AgentCommandRule;
    let rules = vec![AgentCommandRule {
        when: "  ".to_string(),
        command: "echo \"{prompt}\"".to_string(),
    }];
    let result = super::validate_agent_command_rules("ag1", &rules);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn validate_command_rules_missing_prompt_placeholder() {
    use crate::config::AgentCommandRule;
    let rules = vec![AgentCommandRule {
        when: "true".to_string(),
        command: "echo hello".to_string(),
    }];
    let result = super::validate_agent_command_rules("ag1", &rules);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("{prompt} placeholder")
    );
}

#[test]
fn validate_command_rules_multiple_valid() {
    use crate::config::AgentCommandRule;
    let rules = vec![
        AgentCommandRule {
            when: "vars.mode == \"fast\"".to_string(),
            command: "fast-agent \"{prompt}\"".to_string(),
        },
        AgentCommandRule {
            when: "vars.mode == \"slow\"".to_string(),
            command: "slow-agent \"{prompt}\"".to_string(),
        },
    ];
    let result = super::validate_agent_command_rules("ag1", &rules);
    assert!(result.is_ok());
}

#[test]
fn validate_command_rules_second_rule_invalid_cel() {
    use crate::config::AgentCommandRule;
    let rules = vec![
        AgentCommandRule {
            when: "true".to_string(),
            command: "echo \"{prompt}\"".to_string(),
        },
        AgentCommandRule {
            when: "bad @@@ cel".to_string(),
            command: "echo \"{prompt}\"".to_string(),
        },
    ];
    let result = super::validate_agent_command_rules("ag1", &rules);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("command_rules[1]"),
        "error should reference second rule index, got: {}",
        err
    );
}

#[test]
fn validate_command_rules_second_rule_missing_prompt() {
    use crate::config::AgentCommandRule;
    let rules = vec![
        AgentCommandRule {
            when: "true".to_string(),
            command: "echo \"{prompt}\"".to_string(),
        },
        AgentCommandRule {
            when: "true".to_string(),
            command: "echo hello".to_string(),
        },
    ];
    let result = super::validate_agent_command_rules("ag1", &rules);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("command_rules[1]"),
        "error should reference second rule index, got: {}",
        err
    );
    assert!(err.contains("{prompt} placeholder"));
}

// ── command rule CEL evaluation with pipeline vars ──────────────────

#[test]
fn command_rule_cel_matches_pipeline_var() {
    let mut ctx = make_prehook_ctx();
    ctx.vars
        .insert("loop_session_id".to_string(), "ABC-123".to_string());
    let result = evaluate_step_prehook_expression("loop_session_id != \"\"", &ctx);
    assert!(result.unwrap());
}

#[test]
fn command_rule_cel_empty_var_does_not_match() {
    let mut ctx = make_prehook_ctx();
    ctx.vars
        .insert("loop_session_id".to_string(), String::new());
    let result = evaluate_step_prehook_expression("loop_session_id != \"\"", &ctx);
    assert!(!result.unwrap());
}

#[test]
fn command_rule_cel_missing_var_does_not_match() {
    let ctx = make_prehook_ctx();
    // loop_session_id not in vars → evaluates as empty string
    let result = evaluate_step_prehook_expression("loop_session_id != \"\"", &ctx);
    // Should either return false or error — either way, not a match
    assert!(!result.unwrap_or(false));
}

// ========================================================================
// build_step_prehook_cel_context: Default context and variable accessibility
// ========================================================================

#[test]
fn test_build_step_prehook_cel_context_default_succeeds() {
    let context = StepPrehookContext::default();
    let result = evaluate_step_prehook_expression("cycle == 0", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_step_prehook_cel_context_default_qa_failed_false() {
    let context = StepPrehookContext::default();
    let result = evaluate_step_prehook_expression("qa_failed == false", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_step_prehook_cel_context_default_bool_variables() {
    let context = StepPrehookContext::default();
    // All boolean fields should be accessible and default to false
    let result = evaluate_step_prehook_expression(
        "!qa_failed && !fix_required && !is_last_cycle && !last_sandbox_denied && !fix_has_changes && self_referential_safe",
        &context,
    );
    assert!(result.is_ok());
    // self_referential_safe defaults to true, the rest to false
    // fix_has_changes defaults to None which is treated as false in CEL
    // Check if it evaluates without error -- result depends on default values
    let _ = result.unwrap();
}

#[test]
fn test_build_step_prehook_cel_context_default_int_variables() {
    let context = StepPrehookContext::default();
    let result = evaluate_step_prehook_expression(
        "active_ticket_count == 0 && new_ticket_count == 0 && sandbox_denied_count == 0 && build_errors == 0 && test_failures == 0",
        &context,
    );
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_step_prehook_cel_context_default_string_variables() {
    let context = StepPrehookContext::default();
    let result = evaluate_step_prehook_expression("step == \"\" && task_id == \"\"", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

// ========================================================================
// build_finalize_cel_context: Default context
// ========================================================================

#[test]
fn test_build_finalize_cel_context_default_succeeds() {
    let context = default_item_finalize_context();
    let rule = make_rule("r-default", "cycle == 1", "initial", None);
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_finalize_cel_context_default_bool_variables() {
    let context = ItemFinalizeContext {
        qa_failed: false,
        fix_required: false,
        qa_enabled: false,
        qa_ran: false,
        qa_skipped: false,
        fix_enabled: false,
        fix_ran: false,
        fix_success: false,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r-bools",
        "!qa_failed && !fix_required && !qa_enabled && !qa_ran && !qa_skipped && !fix_enabled && !fix_ran && !fix_success",
        "none",
        None,
    );
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_finalize_cel_context_default_int_variables() {
    let context = default_item_finalize_context();
    let rule = make_rule(
        "r-ints",
        "active_ticket_count == 0 && new_ticket_count == 0 && retest_new_ticket_count == 0 && total_artifacts == 0 && sandbox_denied_count == 0",
        "zero",
        None,
    );
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_finalize_cel_context_default_retest_variables() {
    let context = ItemFinalizeContext {
        retest_enabled: false,
        retest_ran: false,
        retest_success: false,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r-retest",
        "!retest_enabled && !retest_ran && !retest_success",
        "no_retest",
        None,
    );
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_finalize_cel_context_default_artifact_variables() {
    let context = default_item_finalize_context();
    let rule = make_rule(
        "r-artifacts",
        "!has_ticket_artifacts && !has_code_change_artifacts && total_artifacts == 0",
        "no_artifacts",
        None,
    );
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_build_finalize_cel_context_default_qa_observed_variables() {
    let context = ItemFinalizeContext {
        qa_configured: false,
        qa_observed: false,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r-qa-obs",
        "!qa_configured && !qa_observed",
        "unobserved",
        None,
    );
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

// ========================================================================
// Vars type inference paths — comprehensive coverage for context.rs
// ========================================================================

#[test]
fn test_vars_i64_inference() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("count".into(), "42".into());
    let result = evaluate_step_prehook_expression("count == 42", &ctx);
    assert!(result.unwrap(), "i64 var should be injected as integer");
}

#[test]
fn test_vars_f64_inference() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("ratio".into(), "3.14".into());
    let result = evaluate_step_prehook_expression("ratio > 3.0", &ctx);
    assert!(result.unwrap(), "f64 var should be injected as double");
}

#[test]
fn test_vars_bool_inference() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("flag".into(), "true".into());
    let result = evaluate_step_prehook_expression("flag == true", &ctx);
    assert!(result.unwrap(), "bool var should be injected as boolean");
}

#[test]
fn test_vars_bool_false_inference() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("flag".into(), "false".into());
    let result = evaluate_step_prehook_expression("flag == false", &ctx);
    assert!(result.unwrap(), "false bool var should be injected as boolean");
}

#[test]
fn test_vars_string_inference() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("name".into(), "hello".into());
    let result = evaluate_step_prehook_expression("name == \"hello\"", &ctx);
    assert!(result.unwrap(), "string var should be injected as string");
}

#[test]
fn test_vars_json_array_size() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("tags".into(), r#"["a","b"]"#.into());
    let result = evaluate_step_prehook_expression("size(tags) == 2", &ctx);
    assert!(result.unwrap(), "JSON array var should be a list with size 2");
}

#[test]
fn test_vars_json_array_contains() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("tags".into(), r#"["a","b","c"]"#.into());
    let result = evaluate_step_prehook_expression("\"b\" in tags", &ctx);
    assert!(result.unwrap(), "JSON array var should support 'in' operator");
}

#[test]
fn test_vars_truncated_skipped_is_absent() {
    let mut ctx = default_step_prehook_context();
    ctx.vars
        .insert("big".into(), "data [truncated at limit]".into());
    // Truncated var should not be in CEL context — referencing it should error
    let result = evaluate_step_prehook_expression("big == \"anything\"", &ctx);
    assert!(result.is_err(), "truncated var should not be in CEL context");
}

#[test]
fn test_vars_negative_integer() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("offset".into(), "-5".into());
    let result = evaluate_step_prehook_expression("offset == -5", &ctx);
    assert!(result.unwrap(), "negative i64 var should work");
}

#[test]
fn test_vars_negative_float() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("temp".into(), "-2.5".into());
    let result = evaluate_step_prehook_expression("temp < 0.0", &ctx);
    assert!(result.unwrap(), "negative f64 var should work");
}

#[test]
fn test_vars_zero_integer() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("zero".into(), "0".into());
    let result = evaluate_step_prehook_expression("zero == 0", &ctx);
    assert!(result.unwrap(), "zero i64 var should work");
}

#[test]
fn test_vars_empty_json_array() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("empty_list".into(), "[]".into());
    let result = evaluate_step_prehook_expression("size(empty_list) == 0", &ctx);
    assert!(result.unwrap(), "empty JSON array should have size 0");
}

#[test]
fn test_vars_multiple_types_combined() {
    let mut ctx = default_step_prehook_context();
    ctx.vars.insert("count".into(), "42".into());
    ctx.vars.insert("ratio".into(), "3.14".into());
    ctx.vars.insert("enabled".into(), "true".into());
    ctx.vars.insert("label".into(), "prod".into());
    ctx.vars
        .insert("items".into(), r#"["x","y"]"#.into());
    let result = evaluate_step_prehook_expression(
        "count > 10 && ratio > 3.0 && enabled && label == \"prod\" && size(items) == 2",
        &ctx,
    );
    assert!(
        result.unwrap(),
        "multiple vars of different types should all be accessible"
    );
}

// ========================================================================
// Non-default StepPrehookContext field values — coverage for context.rs
// ========================================================================

#[test]
fn test_step_prehook_non_default_cycle_and_max_cycles() {
    let ctx = StepPrehookContext {
        cycle: 5,
        max_cycles: 10,
        is_last_cycle: true,
        ..default_step_prehook_context()
    };
    let result =
        evaluate_step_prehook_expression("cycle == 5 && max_cycles == 10 && is_last_cycle", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_step_prehook_non_default_exit_codes() {
    let ctx = StepPrehookContext {
        qa_exit_code: Some(1),
        fix_exit_code: Some(0),
        ..default_step_prehook_context()
    };
    let result =
        evaluate_step_prehook_expression("qa_exit_code == 1 && fix_exit_code == 0", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_step_prehook_non_default_error_counts() {
    let ctx = StepPrehookContext {
        build_error_count: 3,
        test_failure_count: 2,
        ..default_step_prehook_context()
    };
    let result =
        evaluate_step_prehook_expression("build_errors == 3 && test_failures == 2", &ctx);
    assert!(result.unwrap());
}

#[test]
fn test_step_prehook_self_referential_safe_false_with_scenarios() {
    let ctx = StepPrehookContext {
        self_referential_safe: false,
        self_referential_safe_scenarios: vec!["s1".into(), "s2".into()],
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression(
        "!self_referential_safe && size(self_referential_safe_scenarios) == 2",
        &ctx,
    );
    assert!(result.unwrap());
}

#[test]
fn test_step_prehook_combined_non_default_fields() {
    let ctx = StepPrehookContext {
        cycle: 5,
        max_cycles: 10,
        is_last_cycle: true,
        qa_exit_code: Some(1),
        fix_exit_code: Some(0),
        build_error_count: 3,
        test_failure_count: 2,
        self_referential_safe: false,
        self_referential_safe_scenarios: vec!["s1".into()],
        ..default_step_prehook_context()
    };
    let result = evaluate_step_prehook_expression(
        "cycle == 5 && max_cycles == 10 && is_last_cycle && qa_exit_code == 1 && fix_exit_code == 0 && build_errors == 3 && test_failures == 2 && !self_referential_safe && size(self_referential_safe_scenarios) == 1",
        &ctx,
    );
    assert!(result.unwrap());
}

// ========================================================================
// build_finalize_cel_context: non-default field combinations
// ========================================================================

#[test]
fn test_finalize_context_qa_ran_fix_success_true() {
    let ctx = ItemFinalizeContext {
        qa_ran: true,
        qa_failed: true,
        fix_ran: true,
        fix_success: true,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r1",
        "qa_ran && qa_failed && fix_ran && fix_success",
        "fixed",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_context_optional_confidence_scores() {
    let ctx = ItemFinalizeContext {
        qa_confidence: Some(0.95),
        qa_quality_score: Some(0.88),
        fix_confidence: Some(0.75),
        fix_quality_score: Some(0.60),
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r1",
        "qa_confidence > 0.9 && qa_quality_score > 0.8 && fix_confidence > 0.7 && fix_quality_score > 0.5",
        "high_quality",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_context_none_confidence_is_null() {
    let ctx = ItemFinalizeContext {
        qa_confidence: None,
        fix_confidence: None,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r1",
        "qa_confidence == null && fix_confidence == null",
        "no_scores",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_context_sandbox_denial_with_reason() {
    let ctx = ItemFinalizeContext {
        last_sandbox_denied: true,
        sandbox_denied_count: 5,
        last_sandbox_denial_reason: Some("network access".to_string()),
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r1",
        "last_sandbox_denied && sandbox_denied_count == 5 && last_sandbox_denial_reason == 'network access'",
        "blocked",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_context_artifact_fields_nonzero() {
    let ctx = ItemFinalizeContext {
        total_artifacts: 10,
        has_ticket_artifacts: true,
        has_code_change_artifacts: true,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r1",
        "total_artifacts == 10 && has_ticket_artifacts && has_code_change_artifacts",
        "rich",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_finalize_context_combined_non_default() {
    let ctx = ItemFinalizeContext {
        cycle: 3,
        is_last_cycle: true,
        qa_ran: true,
        qa_failed: false,
        fix_ran: false,
        fix_success: false,
        retest_ran: false,
        retest_success: false,
        qa_confidence: Some(0.99),
        fix_quality_score: Some(0.5),
        total_artifacts: 2,
        has_ticket_artifacts: false,
        has_code_change_artifacts: true,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r1",
        "cycle == 3 && is_last_cycle && qa_ran && !qa_failed && qa_confidence > 0.9 && has_code_change_artifacts",
        "done",
        None,
    );
    assert!(evaluate_finalize_rule_expression(&rule, &ctx).unwrap());
}

#[test]
fn test_build_finalize_cel_context_sandbox_variables() {
    let context = ItemFinalizeContext {
        last_sandbox_denied: true,
        sandbox_denied_count: 3,
        ..default_item_finalize_context()
    };
    let rule = make_rule(
        "r-sandbox",
        "last_sandbox_denied && sandbox_denied_count == 3",
        "denied",
        None,
    );
    let result = evaluate_finalize_rule_expression(&rule, &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

// ========================================================================
// build_convergence_cel_context / evaluate_convergence_expression
// ========================================================================

use super::cel::evaluate_convergence_expression;
use crate::config::ConvergenceContext;

#[test]
fn test_convergence_expression_default_succeeds() {
    let context = ConvergenceContext::default();
    let result = evaluate_convergence_expression("cycle == 0", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_expression_cycle_comparison() {
    let context = ConvergenceContext {
        cycle: 3,
        max_cycles: 5,
        ..Default::default()
    };
    let result = evaluate_convergence_expression("cycle >= 3 && max_cycles == 5", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_expression_self_test_passed() {
    let context = ConvergenceContext {
        self_test_passed: true,
        ..Default::default()
    };
    let result = evaluate_convergence_expression("self_test_passed", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_expression_self_test_not_passed() {
    let context = ConvergenceContext::default();
    let result = evaluate_convergence_expression("self_test_passed", &context);
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn test_convergence_expression_active_tickets() {
    let context = ConvergenceContext {
        active_ticket_count: 5,
        ..Default::default()
    };
    let result =
        evaluate_convergence_expression("active_ticket_count == 0 || self_test_passed", &context);
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn test_convergence_expression_compound() {
    let context = ConvergenceContext {
        cycle: 2,
        max_cycles: 3,
        active_ticket_count: 0,
        self_test_passed: true,
        vars: Default::default(),
    };
    let result = evaluate_convergence_expression(
        "self_test_passed && active_ticket_count == 0 && cycle <= max_cycles",
        &context,
    );
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_expression_invalid_cel() {
    let context = ConvergenceContext::default();
    let result = evaluate_convergence_expression("invalid @#$ expression", &context);
    assert!(result.is_err());
}

#[test]
fn test_convergence_expression_non_bool_result() {
    let context = ConvergenceContext::default();
    let result = evaluate_convergence_expression("cycle + 1", &context);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("must return bool"));
}

// ── Convergence context: pipeline variable injection ──

#[test]
fn test_convergence_var_int() {
    let mut context = ConvergenceContext::default();
    context.vars.insert("retry_limit".to_string(), "5".to_string());
    let result = evaluate_convergence_expression("retry_limit > 3", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_var_float() {
    let mut context = ConvergenceContext::default();
    context.vars.insert("threshold".to_string(), "0.75".to_string());
    let result = evaluate_convergence_expression("threshold > 0.5", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_var_bool() {
    let mut context = ConvergenceContext::default();
    context.vars.insert("force_continue".to_string(), "true".to_string());
    let result = evaluate_convergence_expression("force_continue", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_var_string() {
    let mut context = ConvergenceContext::default();
    context.vars.insert("env".to_string(), "production".to_string());
    let result = evaluate_convergence_expression("env == 'production'", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_convergence_var_combined_with_builtin() {
    let mut context = ConvergenceContext {
        cycle: 2,
        self_test_passed: true,
        ..Default::default()
    };
    context.vars.insert("min_cycles".to_string(), "2".to_string());
    let result =
        evaluate_convergence_expression("self_test_passed && cycle >= min_cycles", &context);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

// ========================================================================
// resolve_workflow_finalize_outcome tests
// ========================================================================

#[test]
fn finalize_outcome_empty_rules_returns_none() {
    let finalize = WorkflowFinalizeConfig { rules: vec![] };
    let ctx = default_item_finalize_context();
    let result = resolve_workflow_finalize_outcome(&finalize, &ctx).unwrap();
    assert!(result.is_none());
}

#[test]
fn finalize_outcome_single_matching_rule() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![make_rule(
            "r1",
            "qa_failed == true",
            "failed",
            Some("qa check failed"),
        )],
    };
    let ctx = ItemFinalizeContext {
        qa_failed: true,
        ..default_item_finalize_context()
    };
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx)
        .unwrap()
        .expect("should match");
    assert_eq!(outcome.rule_id, "r1");
    assert_eq!(outcome.status, "failed");
    assert_eq!(outcome.reason, "qa check failed");
}

#[test]
fn finalize_outcome_multiple_rules_first_matches() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "true", "skipped", Some("always")),
            make_rule("r2", "true", "done", Some("also always")),
        ],
    };
    let ctx = default_item_finalize_context();
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx)
        .unwrap()
        .expect("should match first rule");
    assert_eq!(outcome.rule_id, "r1");
    assert_eq!(outcome.status, "skipped");
}

#[test]
fn finalize_outcome_multiple_rules_second_matches() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "false", "skipped", Some("never")),
            make_rule("r2", "cycle > 0", "done", Some("cycle positive")),
        ],
    };
    let ctx = ItemFinalizeContext {
        cycle: 1,
        ..default_item_finalize_context()
    };
    let outcome = resolve_workflow_finalize_outcome(&finalize, &ctx)
        .unwrap()
        .expect("should match second rule");
    assert_eq!(outcome.rule_id, "r2");
    assert_eq!(outcome.status, "done");
    assert_eq!(outcome.reason, "cycle positive");
}

#[test]
fn finalize_outcome_custom_reason_vs_auto_generated() {
    let ctx = default_item_finalize_context();

    // Rule without reason → auto-generated
    let finalize_auto = WorkflowFinalizeConfig {
        rules: vec![make_rule("auto-reason", "true", "done", None)],
    };
    let outcome_auto = resolve_workflow_finalize_outcome(&finalize_auto, &ctx)
        .unwrap()
        .expect("should match");
    assert_eq!(outcome_auto.reason, "finalize rule 'auto-reason' matched");

    // Rule with custom reason
    let finalize_custom = WorkflowFinalizeConfig {
        rules: vec![make_rule("custom", "true", "done", Some("my reason"))],
    };
    let outcome_custom = resolve_workflow_finalize_outcome(&finalize_custom, &ctx)
        .unwrap()
        .expect("should match");
    assert_eq!(outcome_custom.reason, "my reason");
}

#[test]
fn finalize_outcome_no_rules_match_returns_none() {
    let finalize = WorkflowFinalizeConfig {
        rules: vec![
            make_rule("r1", "false", "skipped", Some("never")),
            make_rule("r2", "qa_failed == true", "failed", Some("qa bad")),
        ],
    };
    let ctx = ItemFinalizeContext {
        qa_failed: false,
        ..default_item_finalize_context()
    };
    let result = resolve_workflow_finalize_outcome(&finalize, &ctx).unwrap();
    assert!(result.is_none());
}
