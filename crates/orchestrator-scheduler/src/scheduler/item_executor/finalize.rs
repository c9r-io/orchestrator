use agent_orchestrator::config::TaskRuntimeContext;
use agent_orchestrator::events::insert_event;
use agent_orchestrator::prehook::{emit_item_finalize_event, resolve_workflow_finalize_outcome};
use agent_orchestrator::state::InnerState;
use agent_orchestrator::ticket::list_existing_tickets_for_item;
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use super::accumulator::StepExecutionAccumulator;

pub async fn finalize_item_execution(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &agent_orchestrator::dto::TaskItemRow,
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
        )
        .await?;
    } else if acc.flags.get("execution_failed").copied().unwrap_or(false) {
        acc.item_status = "unresolved".to_string();
    } else if let Some(outcome) =
        resolve_workflow_finalize_outcome(&task_ctx.execution_plan.finalize, &finalize_context)?
    {
        acc.item_status = outcome.status.clone();
        emit_item_finalize_event(state, &finalize_context, &outcome).await?;
    }

    let has_ticket_artifacts = !acc.created_ticket_files.is_empty()
        || acc.phase_artifacts.iter().any(|a| {
            matches!(
                a.kind,
                agent_orchestrator::collab::ArtifactKind::Ticket { .. }
            )
        });
    if has_ticket_artifacts {
        let ticket_content: Vec<&serde_json::Value> = acc
            .phase_artifacts
            .iter()
            .filter(|a| {
                matches!(
                    a.kind,
                    agent_orchestrator::collab::ArtifactKind::Ticket { .. }
                )
            })
            .filter_map(|a| a.content.as_ref())
            .collect();
        let files_json =
            serde_json::to_string(&acc.created_ticket_files).unwrap_or_else(|_| "[]".to_string());
        let content_json =
            serde_json::to_string(&ticket_content).unwrap_or_else(|_| "[]".to_string());
        state
            .db_writer
            .update_task_item_tickets(item_id, &files_json, &content_json)
            .await?;
    }

    persist_item_pipeline_vars(
        state,
        item_id,
        item.dynamic_vars_json.as_deref(),
        &acc.pipeline_vars.vars,
    )
    .await;

    state
        .db_writer
        .set_task_item_terminal_status(item_id, &acc.item_status)
        .await?;
    Ok(())
}

/// Merge item's accumulated pipeline_vars into its dynamic_vars_json and persist.
pub async fn persist_item_pipeline_vars(
    state: &Arc<InnerState>,
    item_id: &str,
    existing_dynamic_vars_json: Option<&str>,
    pipeline_vars: &HashMap<String, String>,
) {
    if pipeline_vars.is_empty() {
        return;
    }
    // Start from existing dynamic_vars, overlay pipeline_vars on top
    let mut merged: HashMap<String, String> = existing_dynamic_vars_json
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    merged.extend(pipeline_vars.iter().map(|(k, v)| (k.clone(), v.clone())));
    if let Ok(json) = serde_json::to_string(&merged) {
        let _ = state
            .db_writer
            .update_task_item_pipeline_vars(item_id, &json)
            .await;
    }
}
