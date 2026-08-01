use anyhow::Result;
use orchestrator_proto::{AttentionDelta, AttentionItem, AttentionListResponse};

use crate::OutputFormat;

use super::render;

pub(crate) fn attention_item_value(item: &AttentionItem) -> serde_json::Value {
    serde_json::json!({
        "id": item.id,
        "project_id": item.project_id,
        "task_id": item.task_id,
        "task_item_id": item.task_item_id,
        "step_id": item.step_id,
        "session_id": item.session_id,
        "kind": item.kind,
        "severity": item.severity,
        "state": item.state,
        "title": item.title,
        "summary": item.summary,
        "assignee": item.assignee,
        "occurrence_count": item.occurrence_count,
        "reopen_count": item.reopen_count,
        "version": item.version,
        "created_at": item.created_at,
        "updated_at": item.updated_at,
        "snoozed_until": item.snoozed_until,
        "actions": item.actions.iter().map(|action| serde_json::json!({
            "id": action.id,
            "label": action.label,
            "required_role": action.required_role,
            "confirmation": action.confirmation,
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn print_list(response: &AttentionListResponse, format: OutputFormat) -> Result<()> {
    let projected = serde_json::json!({
        "items": response.items.iter().map(attention_item_value).collect::<Vec<_>>(),
        "latest_change_id": response.latest_change_id,
    });
    match format.encoding() {
        Some(encoding) => render::emit(&projected, encoding),
        None => {
            if response.items.is_empty() {
                println!("No attention items found.");
                return Ok(());
            }
            println!(
                "{:<22} {:<13} {:<12} {:<18} TITLE",
                "ID", "SEVERITY", "STATE", "KIND"
            );
            for item in &response.items {
                println!(
                    "{:<22} {:<13} {:<12} {:<18} {}",
                    item.id, item.severity, item.state, item.kind, item.title
                );
            }
            println!("\n{} item(s)", response.items.len());
            Ok(())
        }
    }
}

pub(crate) fn print_item(item: &AttentionItem, format: OutputFormat) -> Result<()> {
    let projected = attention_item_value(item);
    match format.encoding() {
        Some(encoding) => render::emit(&projected, encoding),
        None => {
            print!("{}", render::kv_table(&projected));
            Ok(())
        }
    }
}

pub(crate) fn print_delta(delta: &AttentionDelta, format: crate::StreamFormat) -> Result<()> {
    let projected = serde_json::json!({
        "kind": delta.kind,
        "change_id": delta.change_id,
        "item": delta.item.as_ref().map(attention_item_value),
    });
    render::emit(&projected, format.encoding())
}
