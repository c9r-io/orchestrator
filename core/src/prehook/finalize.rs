use crate::config::{
    ItemFinalizeContext, StepPrehookConfig, StepPrehookContext, WorkflowFinalizeConfig,
};
use anyhow::Result;

use super::cel::{evaluate_finalize_rule_expression, evaluate_step_prehook_expression};

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
