use crate::config::{
    ItemFinalizeContext, StepHookEngine, StepPrehookConfig, StepPrehookContext,
    WorkflowFinalizeConfig, WorkflowFinalizeOutcome, WorkflowFinalizeRule,
};
use crate::dynamic_orchestration::PrehookDecision;
use anyhow::Result;
use cel_interpreter::{Context as CelContext, Program, Value as CelValue};
use serde_json::json;
use std::path::Path;

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
        .add_variable("qa_enabled", context.qa_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_ran", context.qa_ran)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_skipped", context.qa_skipped)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_enabled", context.fix_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_ran", context.fix_ran)
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

pub fn simulate_prehook_impl(
    payload: crate::dto::SimulatePrehookPayload,
) -> Result<crate::dto::SimulatePrehookResult> {
    let expression = payload.expression.trim().to_string();
    if expression.is_empty() {
        anyhow::bail!("prehook expression cannot be empty");
    }
    let step_name = payload
        .step
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("simulation")
        .to_string();
    let context = StepPrehookContext {
        task_id: "simulation".to_string(),
        task_item_id: "simulation".to_string(),
        cycle: if payload.context.cycle < 0 {
            0
        } else {
            payload.context.cycle as u32
        },
        step: step_name,
        qa_file_path: "simulation.md".to_string(),
        item_status: "pending".to_string(),
        task_status: "running".to_string(),
        qa_exit_code: payload.context.qa_exit_code,
        fix_exit_code: payload.context.fix_exit_code,
        retest_exit_code: payload.context.retest_exit_code,
        active_ticket_count: payload.context.active_ticket_count,
        new_ticket_count: payload.context.new_ticket_count,
        qa_failed: payload.context.qa_failed,
        fix_required: payload.context.fix_required,
        qa_confidence: None,
        qa_quality_score: None,
        fix_has_changes: None,
        upstream_artifacts: vec![],
    };
    let result = evaluate_step_prehook_expression(&expression, &context)?;
    Ok(crate::dto::SimulatePrehookResult { result, expression })
}

pub fn evaluate_step_prehook(
    state: &crate::state::InnerState,
    app: Option<&tauri::AppHandle>,
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
            app,
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
            app,
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
    app: Option<&tauri::AppHandle>,
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
    if let Some(app_handle) = app {
        crate::events::emit_event(
            app_handle,
            &context.task_id,
            Some(&context.task_item_id),
            "step_prehook_evaluated",
            payload,
        );
    }
    Ok(())
}

pub fn emit_item_finalize_event(
    state: &crate::state::InnerState,
    app: Option<&tauri::AppHandle>,
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
    if let Some(app_handle) = app {
        crate::events::emit_event(
            app_handle,
            &context.task_id,
            Some(&context.task_item_id),
            "item_finalize_evaluated",
            payload,
        );
    }
    Ok(())
}
