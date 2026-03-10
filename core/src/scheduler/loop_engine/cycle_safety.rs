use crate::config::InvariantCheckPoint;
use crate::config::OnViolation;
use crate::events::insert_event;
use crate::scheduler::invariant::{
    evaluate_invariants, has_halting_violation, has_rollback_violation,
};
use crate::scheduler::safety::{
    create_checkpoint, restore_binary_snapshot, rollback_to_checkpoint, snapshot_binary,
};
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

/// Create a git-tag checkpoint at the start of a cycle, with optional binary snapshot.
pub(super) async fn create_cycle_checkpoint(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
) -> Result<()> {
    if !matches!(
        task_ctx.safety.checkpoint_strategy,
        crate::config::CheckpointStrategy::GitTag
    ) {
        return Ok(());
    }

    let ws_path = Path::new(&task_ctx.workspace_root);
    match create_checkpoint(ws_path, task_id, task_ctx.current_cycle).await {
        Ok(tag) => {
            insert_event(
                state,
                task_id,
                None,
                "checkpoint_created",
                json!({"cycle": task_ctx.current_cycle, "tag": tag}),
            )
            .await?;

            if should_snapshot_binary(task_ctx.safety.binary_snapshot, task_ctx.self_referential) {
                match snapshot_binary(&task_ctx.workspace_root, task_id, task_ctx.current_cycle)
                    .await
                {
                    Ok(path) => {
                        insert_event(
                            state,
                            task_id,
                            None,
                            "binary_snapshot_created",
                            json!({"cycle": task_ctx.current_cycle, "path": path.display().to_string()}),
                        )
                        .await?;
                    }
                    Err(e) => {
                        warn!(
                            cycle = task_ctx.current_cycle,
                            error = %e,
                            "failed to create binary snapshot"
                        );
                    }
                }
            }
        }
        Err(e) => {
            warn!(
                cycle = task_ctx.current_cycle,
                error = %e,
                "failed to create checkpoint"
            );
        }
    }

    Ok(())
}

/// Check invariants at a given checkpoint. Returns:
/// - `None` if all pass or only warnings
/// - `Some("halt")` if a Halt violation is found
/// - `Some("rollback")` if a Rollback violation is found
pub(super) async fn check_invariants(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
    checkpoint: InvariantCheckPoint,
) -> Result<Option<&'static str>> {
    if task_ctx.pinned_invariants.is_empty() {
        return Ok(None);
    }

    let results = evaluate_invariants(
        &task_ctx.pinned_invariants,
        checkpoint,
        &task_ctx.workspace_root,
    )?;

    if results.is_empty() {
        return Ok(None);
    }

    // Emit events for each result
    for r in &results {
        let event_type = if r.passed {
            "invariant_passed"
        } else {
            "invariant_violated"
        };
        insert_event(
            state,
            task_id,
            None,
            event_type,
            json!({
                "invariant": r.name,
                "checkpoint": format!("{:?}", checkpoint),
                "passed": r.passed,
                "message": r.message,
                "on_violation": format!("{:?}", r.on_violation),
            }),
        )
        .await?;
        if !r.passed && r.on_violation == OnViolation::Warn {
            warn!(
                invariant = %r.name,
                message = %r.message,
                "invariant warning at {:?}",
                checkpoint
            );
        }
    }

    if has_halting_violation(&results) {
        return Ok(Some("halt"));
    }
    if has_rollback_violation(&results) {
        return Ok(Some("rollback"));
    }

    Ok(None)
}

/// Detect consecutive failures and perform git rollback with optional binary recovery.
pub(super) async fn apply_auto_rollback_if_needed(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut crate::config::TaskRuntimeContext,
) -> Result<()> {
    if !(task_ctx.safety.auto_rollback
        && task_ctx.consecutive_failures >= task_ctx.safety.max_consecutive_failures
        && matches!(
            task_ctx.safety.checkpoint_strategy,
            crate::config::CheckpointStrategy::GitTag
        ))
    {
        return Ok(());
    }

    let rollback_cycle = task_ctx
        .current_cycle
        .saturating_sub(task_ctx.consecutive_failures);
    let rollback_tag = format!("checkpoint/{}/{}", task_id, rollback_cycle.max(1));
    let ws_path = Path::new(&task_ctx.workspace_root);
    match rollback_to_checkpoint(ws_path, &rollback_tag).await {
        Ok(()) => {
            insert_event(
                state,
                task_id,
                None,
                "auto_rollback",
                json!({
                    "cycle": task_ctx.current_cycle,
                    "rollback_to": rollback_tag,
                    "consecutive_failures": task_ctx.consecutive_failures,
                }),
            )
            .await?;
            state.emit_event(
                task_id,
                None,
                "auto_rollback",
                json!({"rollback_to": rollback_tag}),
            );

            if should_snapshot_binary(task_ctx.safety.binary_snapshot, task_ctx.self_referential) {
                match restore_binary_snapshot(&task_ctx.workspace_root).await {
                    Ok(()) => {
                        insert_event(
                            state,
                            task_id,
                            None,
                            "binary_snapshot_restored",
                            json!({"cycle": task_ctx.current_cycle}),
                        )
                        .await?;
                    }
                    Err(e) => warn!(error = %e, "failed to restore binary snapshot"),
                }
            }

            task_ctx.consecutive_failures = 0;
        }
        Err(e) => {
            warn!(error = %e, "auto-rollback failed");
            insert_event(
                state,
                task_id,
                None,
                "auto_rollback_failed",
                json!({"error": e.to_string()}),
            )
            .await?;
        }
    }

    Ok(())
}

pub(super) fn should_snapshot_binary(binary_snapshot: bool, self_referential: bool) -> bool {
    binary_snapshot && self_referential
}

/// Pure function: determine if auto-rollback should trigger.
#[cfg(test)]
pub(crate) fn should_auto_rollback(
    auto_rollback: bool,
    consecutive_failures: u64,
    max_consecutive_failures: u64,
    checkpoint_strategy: &crate::config::CheckpointStrategy,
) -> bool {
    auto_rollback
        && consecutive_failures >= max_consecutive_failures
        && matches!(
            checkpoint_strategy,
            crate::config::CheckpointStrategy::GitTag
        )
}

/// Pure function: compute the rollback tag for a given state.
#[cfg(test)]
pub(crate) fn compute_rollback_tag(
    task_id: &str,
    current_cycle: u64,
    consecutive_failures: u64,
) -> String {
    let rollback_cycle = current_cycle.saturating_sub(consecutive_failures);
    format!("checkpoint/{}/{}", task_id, rollback_cycle.max(1))
}
