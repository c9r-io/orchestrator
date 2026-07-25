use crate::config::{ConvergenceContext, ItemFinalizeContext, StepPrehookContext};
use anyhow::Result;
use cel_interpreter::Context as CelContext;

/// Bind compatibility variables before explicit governance fields.
///
/// Production coordination consumers are forbidden by the governance ledger,
/// but this compatibility surface remains until command rules and public
/// initial/item input bindings receive a separately governed migration.
fn bind_compatibility_vars<'a>(
    cel_context: &mut CelContext<'a>,
    vars: &'a std::collections::HashMap<String, String>,
    err_prefix: &str,
) -> Result<()> {
    for (key, value) in vars {
        if value.contains("[truncated") {
            continue;
        }
        if value.starts_with('[')
            && let Ok(items) = serde_json::from_str::<Vec<String>>(value)
        {
            cel_context
                .add_variable(key.as_str(), items)
                .map_err(|error| anyhow::anyhow!("{err_prefix}: {error}"))?;
            continue;
        }
        if let Ok(integer) = value.parse::<i64>() {
            cel_context
                .add_variable(key.as_str(), integer)
                .map_err(|error| anyhow::anyhow!("{err_prefix}: {error}"))?;
        } else if let Ok(float) = value.parse::<f64>() {
            cel_context
                .add_variable(key.as_str(), float)
                .map_err(|error| anyhow::anyhow!("{err_prefix}: {error}"))?;
        } else if let Ok(boolean) = value.parse::<bool>() {
            cel_context
                .add_variable(key.as_str(), boolean)
                .map_err(|error| anyhow::anyhow!("{err_prefix}: {error}"))?;
        } else {
            cel_context
                .add_variable(key.as_str(), value.clone())
                .map_err(|error| anyhow::anyhow!("{err_prefix}: {error}"))?;
        }
    }
    Ok(())
}

pub(super) fn build_step_prehook_cel_context(
    context: &StepPrehookContext,
) -> Result<CelContext<'_>> {
    let mut cel_context = CelContext::default();
    let err_prefix = format!("step '{}' prehook context build failed", context.step);
    bind_compatibility_vars(&mut cel_context, &context.vars, &err_prefix)?;
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
        .add_variable(
            "api_publishable",
            context
                .vars
                .get("api_publishable")
                .is_some_and(|value| value == "true"),
        )
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
    cel_context
        .add_variable(
            "self_referential_safe_scenarios",
            context.self_referential_safe_scenarios.clone(),
        )
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
    bind_compatibility_vars(
        &mut cel_context,
        &context.vars,
        "finalize context build failed",
    )?;
    cel_context
        .add_variable("context", context.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("task_id", context.task_id.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("task_item_id", context.task_item_id.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("cycle", context.cycle as i64)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_file_path", context.qa_file_path.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("item_status", context.item_status.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("task_status", context.task_status.clone())
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_exit_code", context.qa_exit_code)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_exit_code", context.fix_exit_code)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("retest_exit_code", context.retest_exit_code)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("active_ticket_count", context.active_ticket_count)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("new_ticket_count", context.new_ticket_count)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("retest_new_ticket_count", context.retest_new_ticket_count)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_failed", context.qa_failed)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_required", context.fix_required)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("last_sandbox_denied", context.last_sandbox_denied)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("sandbox_denied_count", context.sandbox_denied_count as i64)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable(
            "last_sandbox_denial_reason",
            context.last_sandbox_denial_reason.clone(),
        )
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_configured", context.qa_configured)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_observed", context.qa_observed)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_enabled", context.qa_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_ran", context.qa_ran)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_skipped", context.qa_skipped)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_configured", context.fix_configured)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_enabled", context.fix_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_ran", context.fix_ran)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_skipped", context.fix_skipped)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_success", context.fix_success)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("retest_enabled", context.retest_enabled)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("retest_ran", context.retest_ran)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("retest_success", context.retest_success)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_confidence", context.qa_confidence)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("qa_quality_score", context.qa_quality_score)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_confidence", context.fix_confidence)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("fix_quality_score", context.fix_quality_score)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("total_artifacts", context.total_artifacts)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("has_ticket_artifacts", context.has_ticket_artifacts)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable(
            "has_code_change_artifacts",
            context.has_code_change_artifacts,
        )
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    cel_context
        .add_variable("is_last_cycle", context.is_last_cycle)
        .map_err(|err| anyhow::anyhow!("finalize context build failed: {err}"))?;
    Ok(cel_context)
}

pub(super) fn build_convergence_cel_context(
    context: &ConvergenceContext,
) -> Result<CelContext<'_>> {
    let mut cel_context = CelContext::default();
    let err_msg = "convergence context build failed";
    bind_compatibility_vars(&mut cel_context, &context.vars, err_msg)?;
    cel_context
        .add_variable("cycle", context.cycle as i64)
        .map_err(|e| anyhow::anyhow!("{err_msg}: {e}"))?;
    cel_context
        .add_variable("active_ticket_count", context.active_ticket_count)
        .map_err(|e| anyhow::anyhow!("{err_msg}: {e}"))?;
    cel_context
        .add_variable("self_test_passed", context.self_test_passed)
        .map_err(|e| anyhow::anyhow!("{err_msg}: {e}"))?;
    cel_context
        .add_variable("max_cycles", context.max_cycles as i64)
        .map_err(|e| anyhow::anyhow!("{err_msg}: {e}"))?;
    cel_context
        .add_variable(
            "tools_called",
            context
                .vars
                .get("tools_called")
                .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
                .unwrap_or_default(),
        )
        .map_err(|e| anyhow::anyhow!("{err_msg}: {e}"))?;
    cel_context
        .add_variable(
            "tool_error_count",
            context
                .vars
                .get("tool_error_count")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0),
        )
        .map_err(|e| anyhow::anyhow!("{err_msg}: {e}"))?;
    Ok(cel_context)
}
