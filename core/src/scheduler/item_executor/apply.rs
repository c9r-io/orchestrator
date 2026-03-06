use crate::config::{OnFailureAction, OnSuccessAction, PostAction, TaskExecutionStep, TaskRuntimeContext};
use crate::events::insert_event;
use crate::state::InnerState;
use crate::ticket::{create_ticket_for_qa_failure, scan_active_tickets_for_task_items};
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

use super::accumulator::StepExecutionAccumulator;
use super::dispatch::is_execution_hard_failure;

/// Apply step results: capture outputs, status transitions, post-actions,
/// artifact collection, confidence/quality scores, and event emission.
/// Returns `true` if the caller should return early (terminal state).
#[allow(clippy::too_many_arguments)]
pub(super) async fn apply_step_results(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    step: &TaskExecutionStep,
    task_ctx: &TaskRuntimeContext,
    task_item_paths: &[String],
    qa_file_path: &str,
    result: &crate::dto::RunResult,
    acc: &mut StepExecutionAccumulator,
) -> Result<bool> {
    // 3. Capture outputs
    acc.exit_codes.insert(step.id.clone(), result.exit_code);
    acc.apply_captures(&step.behavior.captures, &step.id, result);
    acc.step_ran.insert(step.id.clone(), true);

    // 4. Status transitions
    if result.is_success() {
        if let OnSuccessAction::SetStatus { status } = &step.behavior.on_success {
            acc.item_status = status.clone();
        }
    } else {
        match &step.behavior.on_failure {
            OnFailureAction::Continue => {}
            OnFailureAction::SetStatus { status } => {
                acc.item_status = status.clone();
            }
            OnFailureAction::EarlyReturn { status } => {
                acc.item_status = status.clone();
                acc.terminal = true;
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step": phase, "step_id": step.id, "step_scope": step.resolved_scope(), "early_return": true, "exit_code": result.exit_code, "success": false}),
                )
                .await?;
                return Ok(true);
            }
        }
    }

    // 5. Post-actions
    for action in &step.behavior.post_actions {
        match action {
            PostAction::CreateTicket if !result.is_success() => {
                if let Some(exit_code) = acc.exit_codes.get(&step.id) {
                    let task_name = state.task_repo.load_task_name(task_id).await?
                        .unwrap_or_else(|| task_id.to_string());
                    match create_ticket_for_qa_failure(
                        &task_ctx.workspace_root,
                        &task_ctx.ticket_dir,
                        &task_name,
                        qa_file_path,
                        *exit_code,
                        &result.stdout_path,
                        &result.stderr_path,
                    ) {
                        Ok(Some(ticket_path)) => {
                            acc.created_ticket_files.push(ticket_path.clone());
                            insert_event(
                                state,
                                task_id,
                                Some(item_id),
                                "ticket_created",
                                json!({"path": ticket_path, "qa_file": qa_file_path}),
                            )
                            .await?;
                        }
                        Ok(None) => {}
                        Err(e) => warn!(error = %e, "failed to auto-create ticket"),
                    }
                }
            }
            PostAction::ScanTickets => {
                let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
                acc.active_tickets =
                    tickets.get(qa_file_path).cloned().unwrap_or_default();
                acc.new_ticket_count = acc.active_tickets.len() as i64;
            }
            _ => {}
        }
    }

    // 6. Collect artifacts
    if step.behavior.collect_artifacts {
        let step_artifacts = result
            .output
            .as_ref()
            .map(|o| o.artifacts.clone())
            .unwrap_or_default();
        if !step_artifacts.is_empty() {
            insert_event(
                state,
                task_id,
                Some(item_id),
                "artifacts_parsed",
                json!({"step": phase, "count": step_artifacts.len()}),
            )
            .await?;
            acc.phase_artifacts.extend(step_artifacts);
        }
    }

    // Also check for ticket artifacts that may seed active_tickets
    if acc.active_tickets.is_empty() {
        let ticket_artifact_count = acc
            .phase_artifacts
            .iter()
            .filter(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }))
            .count();
        if ticket_artifact_count > 0 {
            acc.active_tickets = (0..ticket_artifact_count)
                .map(|idx| format!("artifact://ticket/{}", idx))
                .collect();
            acc.new_ticket_count = acc.active_tickets.len() as i64;
        }
    }

    let confidence = result.output.as_ref().map(|o| o.confidence).unwrap_or(0.0);
    let quality = result
        .output
        .as_ref()
        .map(|o| o.quality_score)
        .unwrap_or(0.0);

    match phase {
        "qa" | "qa_testing" => {
            acc.qa_confidence = Some(confidence);
            acc.qa_quality_score = Some(quality);
        }
        "fix" | "ticket_fix" => {
            acc.fix_confidence = Some(confidence);
            acc.fix_quality_score = Some(quality);
        }
        _ => {}
    }

    insert_event(
        state,
        task_id,
        Some(item_id),
        "step_finished",
        json!({
                "step": phase,
                "step_id": step.id,
                "step_scope": step.resolved_scope(),
                "agent_id": result.agent_id,
                "run_id": result.run_id,
                "exit_code": result.exit_code,
            "success": result.is_success(),
            "timed_out": result.timed_out,
            "duration_ms": result.duration_ms,
            "build_errors": acc.pipeline_vars.build_errors.len(),
            "test_failures": acc.pipeline_vars.test_failures.len(),
            "confidence": confidence,
            "quality_score": quality,
            "validation_status": result.validation_status,
        }),
    )
    .await?;

    if is_execution_hard_failure(result) {
        acc.item_status = "unresolved".to_string();
        acc.flags.insert("execution_failed".to_string(), true);
        acc.terminal = true;
        return Ok(true);
    }

    Ok(false)
}
