use crate::config::{ItemFinalizeContext, StepPrehookContext, WorkflowFinalizeRule};
use anyhow::Result;
use cel_interpreter::{Program, Value as CelValue};

use super::context::{build_finalize_cel_context, build_step_prehook_cel_context};

/// Evaluates a step prehook CEL expression against the provided context.
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

/// Evaluates a finalize-rule CEL expression against the provided context.
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
