use crate::config::{
    ItemFinalizeContext, StepHookEngine, StepPrehookConfig, StepPrehookContext,
    WorkflowFinalizeConfig, WorkflowFinalizeRule,
};
use anyhow::Result;
use cel_interpreter::{Context as CelContext, Program, Value as CelValue};

pub fn validate_step_prehook(
    prehook: &StepPrehookConfig,
    workflow_id: &str,
    step_type: &str,
) -> Result<()> {
    let expression = prehook.when.trim();
    if expression.is_empty() {
        anyhow::bail!(
            "workflow '{}' step '{}' prehook.when cannot be empty",
            workflow_id,
            step_type
        );
    }
    match prehook.engine {
        StepHookEngine::Cel => {
            let compiled =
                std::panic::catch_unwind(|| Program::compile(expression)).map_err(|_| {
                    anyhow::anyhow!(
                        "workflow '{}' step '{}' prehook.when caused CEL parser panic",
                        workflow_id,
                        step_type
                    )
                })?;
            compiled.map_err(|err| {
                anyhow::anyhow!(
                    "workflow '{}' step '{}' prehook.when is invalid CEL: {}",
                    workflow_id,
                    step_type,
                    err
                )
            })?;
        }
    }
    Ok(())
}

pub fn validate_workflow_finalize_rule(
    rule: &WorkflowFinalizeRule,
    workflow_id: &str,
) -> Result<()> {
    if rule.id.trim().is_empty() {
        anyhow::bail!("workflow '{}' has finalize rule with empty id", workflow_id);
    }
    if rule.status.trim().is_empty() {
        anyhow::bail!(
            "workflow '{}' finalize rule '{}' has empty status",
            workflow_id,
            rule.id
        );
    }
    let expression = rule.when.trim();
    if expression.is_empty() {
        anyhow::bail!(
            "workflow '{}' finalize rule '{}' has empty when",
            workflow_id,
            rule.id
        );
    }
    match rule.engine {
        StepHookEngine::Cel => {
            let compiled =
                std::panic::catch_unwind(|| Program::compile(expression)).map_err(|_| {
                    anyhow::anyhow!(
                        "workflow '{}' finalize rule '{}' caused CEL parser panic",
                        workflow_id,
                        rule.id
                    )
                })?;
            compiled.map_err(|err| {
                anyhow::anyhow!(
                    "workflow '{}' finalize rule '{}' invalid CEL: {}",
                    workflow_id,
                    rule.id,
                    err
                )
            })?;
        }
    }
    Ok(())
}

fn build_step_prehook_cel_context(context: &StepPrehookContext) -> Result<CelContext<'_>> {
    let mut cel_context = CelContext::default();
    cel_context
        .add_variable("context", context.clone())
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("task_id", context.task_id.clone())
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("task_item_id", context.task_item_id.clone())
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("cycle", context.cycle as i64)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("max_cycles", context.max_cycles as i64)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("is_last_cycle", context.is_last_cycle)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("step", context.step.clone())
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("qa_file_path", context.qa_file_path.clone())
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("item_status", context.item_status.clone())
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("task_status", context.task_status.clone())
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("qa_exit_code", context.qa_exit_code)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("fix_exit_code", context.fix_exit_code)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("retest_exit_code", context.retest_exit_code)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("active_ticket_count", context.active_ticket_count)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("new_ticket_count", context.new_ticket_count)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("qa_failed", context.qa_failed)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("fix_required", context.fix_required)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("build_errors", context.build_error_count)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("test_failures", context.test_failure_count)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("build_exit_code", context.build_exit_code)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("test_exit_code", context.test_exit_code)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    Ok(cel_context)
}

pub fn evaluate_step_prehook_expression(
    expression: &str,
    context: &StepPrehookContext,
) -> Result<bool> {
    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
        .map_err(|_| anyhow::anyhow!("step '{}' prehook compilation panicked", context.step))?;
    let program = compiled.map_err(|err| {
        anyhow::anyhow!(
            "step '{}' prehook compilation failed: {}",
            context.step,
            err
        )
    })?;
    let cel_context = build_step_prehook_cel_context(context)?;
    let value = program.execute(&cel_context).map_err(|err| {
        anyhow::anyhow!("step '{}' prehook execution failed: {}", context.step, err)
    })?;
    match value {
        CelValue::Bool(v) => Ok(v),
        other => {
            anyhow::bail!(
                "step '{}' prehook must return bool, got {:?}",
                context.step,
                other
            );
        }
    }
}

fn build_finalize_cel_context(context: &ItemFinalizeContext) -> Result<CelContext<'_>> {
    let mut cel_context = CelContext::default();
    cel_context
        .add_variable("context", context.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("task_id", context.task_id.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("task_item_id", context.task_item_id.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("cycle", context.cycle as i64)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_file_path", context.qa_file_path.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("item_status", context.item_status.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("task_status", context.task_status.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_exit_code", context.qa_exit_code)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_exit_code", context.fix_exit_code)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("retest_exit_code", context.retest_exit_code)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("active_ticket_count", context.active_ticket_count)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("new_ticket_count", context.new_ticket_count)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("retest_new_ticket_count", context.retest_new_ticket_count)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_failed", context.qa_failed)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_required", context.fix_required)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_configured", context.qa_configured)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_observed", context.qa_observed)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_enabled", context.qa_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_ran", context.qa_ran)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_skipped", context.qa_skipped)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_configured", context.fix_configured)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_enabled", context.fix_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_ran", context.fix_ran)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_skipped", context.fix_skipped)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_success", context.fix_success)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("retest_enabled", context.retest_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("retest_ran", context.retest_ran)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("retest_success", context.retest_success)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("is_last_cycle", context.is_last_cycle)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    Ok(cel_context)
}

pub fn evaluate_finalize_rule_expression(
    rule: &WorkflowFinalizeRule,
    context: &ItemFinalizeContext,
) -> Result<bool> {
    let expression = rule.when.trim();
    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
        .map_err(|_| anyhow::anyhow!("finalize rule '{}' compilation panicked", rule.id))?;
    let program = compiled.map_err(|err| {
        anyhow::anyhow!("finalize rule '{}' compilation failed: {}", rule.id, err)
    })?;
    let cel_context = build_finalize_cel_context(context)?;
    let value = program
        .execute(&cel_context)
        .map_err(|err| anyhow::anyhow!("finalize rule '{}' execution failed: {}", rule.id, err))?;
    match value {
        CelValue::Bool(v) => Ok(v),
        other => anyhow::bail!(
            "finalize rule '{}' must return bool, got {:?}",
            rule.id,
            other
        ),
    }
}

pub fn resolve_workflow_finalize_outcome(
    finalize: &WorkflowFinalizeConfig,
    context: &ItemFinalizeContext,
) -> Result<Option<crate::config::WorkflowFinalizeOutcome>> {
    for rule in &finalize.rules {
        let matched = evaluate_finalize_rule_expression(rule, context)?;
        if !matched {
            continue;
        }
        return Ok(Some(crate::config::WorkflowFinalizeOutcome {
            rule_id: rule.id.clone(),
            status: rule.status.clone(),
            reason: rule
                .reason
                .clone()
                .unwrap_or_else(|| format!("finalize rule '{}' matched", rule.id)),
        }));
    }
    Ok(None)
}

pub fn evaluate_step_prehook(
    state: &crate::state::InnerState,
    prehook: Option<&StepPrehookConfig>,
    context: &StepPrehookContext,
) -> Result<bool> {
    let Some(prehook) = prehook else {
        return Ok(true);
    };
    let expression = prehook.when.trim();

    let should_run = evaluate_step_prehook_expression(expression, context)?;

    if should_run {
        emit_step_prehook_event(
            state,
            context,
            expression,
            prehook
                .reason
                .as_deref()
                .unwrap_or("prehook evaluated to true"),
            "run",
        )?;
    } else {
        emit_step_prehook_event(
            state,
            context,
            expression,
            prehook
                .reason
                .as_deref()
                .unwrap_or("prehook evaluated to false"),
            "skip",
        )?;
    }

    Ok(should_run)
}

pub fn emit_step_prehook_event(
    state: &crate::state::InnerState,
    context: &StepPrehookContext,
    expression: &str,
    reason: &str,
    decision: &str,
) -> Result<()> {
    let payload = serde_json::json!({
        "step": context.step,
        "decision": decision,
        "reason": reason,
        "engine": "cel",
        "when": expression,
        "context": {
            "cycle": context.cycle,
            "item_status": context.item_status,
            "qa_exit_code": context.qa_exit_code,
            "fix_exit_code": context.fix_exit_code,
            "retest_exit_code": context.retest_exit_code,
            "active_ticket_count": context.active_ticket_count,
            "new_ticket_count": context.new_ticket_count,
            "qa_failed": context.qa_failed,
            "fix_required": context.fix_required
        }
    });
    crate::events::insert_event(
        state,
        &context.task_id,
        Some(&context.task_item_id),
        "step_prehook_evaluated",
        payload.clone(),
    )?;
    state.emit_event(
        &context.task_id,
        Some(&context.task_item_id),
        "step_prehook_evaluated",
        payload,
    );
    Ok(())
}

pub fn emit_item_finalize_event(
    state: &crate::state::InnerState,
    context: &ItemFinalizeContext,
    outcome: &crate::config::WorkflowFinalizeOutcome,
) -> Result<()> {
    let payload = serde_json::json!({
        "rule_id": outcome.rule_id,
        "status": outcome.status,
        "reason": outcome.reason,
        "context": context
    });
    crate::events::insert_event(
        state,
        &context.task_id,
        Some(&context.task_item_id),
        "item_finalize_evaluated",
        payload.clone(),
    )?;
    state.emit_event(
        &context.task_id,
        Some(&context.task_item_id),
        "item_finalize_evaluated",
        payload,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{StepHookEngine, StepPrehookContext, WorkflowFinalizeRule};

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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("prehook.when cannot be empty"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty id"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty status"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty when"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("prehook.when cannot be empty"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty id"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty status"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("empty when"));
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("must return bool"));
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
        assert!(result
            .expect("finalize without rules should resolve")
            .is_none());
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
        assert!(result
            .expect("finalize without matches should resolve")
            .is_none());
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
        assert!(outcome.is_none(), "rule should not match when fix_skipped is true");
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("must return bool"));
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
}
