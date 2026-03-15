use crate::config::{ConvergenceContext, ItemFinalizeContext, StepPrehookContext};
use anyhow::Result;
use cel_interpreter::Context as CelContext;

pub(super) fn build_step_prehook_cel_context(
    context: &StepPrehookContext,
) -> Result<CelContext<'_>> {
    let mut cel_context = CelContext::default();
    let err_msg_prefix = format!("step '{}' prehook context build failed", context.step);
    // Inject user-defined pipeline variables first so built-in variables take
    // precedence when names collide (built-ins are added below and overwrite).
    for (key, val) in &context.vars {
        // Skip spilled / truncated values — they are too large for CEL evaluation.
        if val.contains("[truncated") {
            continue;
        }
        // Try JSON array → CEL list<string>
        if val.starts_with('[') {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(val) {
                cel_context
                    .add_variable(key.as_str(), arr)
                    .map_err(|e| anyhow::anyhow!("{}: {}", err_msg_prefix, e))?;
                continue;
            }
        }
        // Type inference: i64 → f64 → bool → string
        if let Ok(i) = val.parse::<i64>() {
            cel_context
                .add_variable(key.as_str(), i)
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg_prefix, e))?;
        } else if let Ok(f) = val.parse::<f64>() {
            cel_context
                .add_variable(key.as_str(), f)
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg_prefix, e))?;
        } else if let Ok(b) = val.parse::<bool>() {
            cel_context
                .add_variable(key.as_str(), b)
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg_prefix, e))?;
        } else {
            cel_context
                .add_variable(key.as_str(), val.clone())
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg_prefix, e))?;
        }
    }
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
        .add_variable("last_sandbox_denied", context.last_sandbox_denied)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("sandbox_denied_count", context.sandbox_denied_count as i64)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable(
            "last_sandbox_denial_reason",
            context.last_sandbox_denial_reason.clone(),
        )
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
        .add_variable("qa_confidence", context.qa_confidence)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("qa_quality_score", context.qa_quality_score)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("fix_has_changes", context.fix_has_changes)
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
    cel_context
        .add_variable("self_test_exit_code", context.self_test_exit_code)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    cel_context
        .add_variable("self_referential_safe", context.self_referential_safe)
        .map_err(|err| {
            anyhow::anyhow!(
                "step '{}' prehook context build failed: {}",
                context.step,
                err
            )
        })?;
    Ok(cel_context)
}

pub(super) fn build_finalize_cel_context(context: &ItemFinalizeContext) -> Result<CelContext<'_>> {
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
        .add_variable("last_sandbox_denied", context.last_sandbox_denied)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("sandbox_denied_count", context.sandbox_denied_count as i64)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable(
            "last_sandbox_denial_reason",
            context.last_sandbox_denial_reason.clone(),
        )
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
        .add_variable("qa_confidence", context.qa_confidence)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("qa_quality_score", context.qa_quality_score)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_confidence", context.fix_confidence)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("fix_quality_score", context.fix_quality_score)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("total_artifacts", context.total_artifacts)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("has_ticket_artifacts", context.has_ticket_artifacts)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable(
            "has_code_change_artifacts",
            context.has_code_change_artifacts,
        )
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    cel_context
        .add_variable("is_last_cycle", context.is_last_cycle)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {}", err))?;
    Ok(cel_context)
}

pub(super) fn build_convergence_cel_context(
    context: &ConvergenceContext,
) -> Result<CelContext<'_>> {
    let mut cel_context = CelContext::default();
    let err_msg = "convergence context build failed";
    cel_context
        .add_variable("cycle", context.cycle as i64)
        .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
    cel_context
        .add_variable("active_ticket_count", context.active_ticket_count)
        .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
    cel_context
        .add_variable("self_test_passed", context.self_test_passed)
        .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
    cel_context
        .add_variable("max_cycles", context.max_cycles as i64)
        .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
    // Inject user-defined pipeline variables. Try parsing as i64/f64/bool first.
    for (key, val) in &context.vars {
        if let Ok(i) = val.parse::<i64>() {
            cel_context
                .add_variable(key.as_str(), i)
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
        } else if let Ok(f) = val.parse::<f64>() {
            cel_context
                .add_variable(key.as_str(), f)
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
        } else if let Ok(b) = val.parse::<bool>() {
            cel_context
                .add_variable(key.as_str(), b)
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
        } else {
            cel_context
                .add_variable(key.as_str(), val.clone())
                .map_err(|e| anyhow::anyhow!("{}: {}", err_msg, e))?;
        }
    }
    Ok(cel_context)
}
