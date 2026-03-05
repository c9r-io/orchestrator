use crate::config::TaskRuntimeContext;
use crate::events::insert_event;
use crate::prehook::{emit_item_finalize_event, resolve_workflow_finalize_outcome};
use crate::state::InnerState;
use crate::ticket::list_existing_tickets_for_item;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

use super::accumulator::StepExecutionAccumulator;

pub fn finalize_item_execution(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    let item_id = item.id.as_str();

    // Seed active tickets from existing ticket files if no scan step ran
    if acc.active_tickets.is_empty() && !acc.step_ran.contains_key("ticket_scan") {
        acc.active_tickets = list_existing_tickets_for_item(task_ctx, &item.qa_file_path)?;
        acc.new_ticket_count = acc.active_tickets.len() as i64;
    }

    let finalize_context = acc.to_finalize_context(task_id, item, task_ctx);
    if finalize_context.is_last_cycle
        && finalize_context.qa_configured
        && !finalize_context.qa_observed
    {
        acc.item_status = "unresolved".to_string();
        insert_event(
            state,
            task_id,
            Some(item_id),
            "item_validation_missing",
            json!({
                "step": "qa_testing",
                "reason": "configured qa step was neither run nor skipped in final cycle"
            }),
        )?;
    } else if acc.flags.get("execution_failed").copied().unwrap_or(false) {
        acc.item_status = "unresolved".to_string();
    } else if let Some(outcome) = resolve_workflow_finalize_outcome(
        &task_ctx.execution_plan.finalize,
        &finalize_context,
    )? {
        acc.item_status = outcome.status.clone();
        emit_item_finalize_event(state, &finalize_context, &outcome)?;
    }

    let has_ticket_artifacts = !acc.created_ticket_files.is_empty()
        || acc
            .phase_artifacts
            .iter()
            .any(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }));
    if has_ticket_artifacts {
        let ticket_content: Vec<&serde_json::Value> = acc
            .phase_artifacts
            .iter()
            .filter(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }))
            .filter_map(|a| a.content.as_ref())
            .collect();
        let files_json =
            serde_json::to_string(&acc.created_ticket_files).unwrap_or_else(|_| "[]".to_string());
        let content_json =
            serde_json::to_string(&ticket_content).unwrap_or_else(|_| "[]".to_string());
        state
            .db_writer
            .update_task_item_tickets(item_id, &files_json, &content_json)?;
    }

    state
        .db_writer
        .set_task_item_terminal_status(item_id, &acc.item_status)?;
    Ok(())
}
