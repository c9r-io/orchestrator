use orchestrator_proto::{AttentionDelta, AttentionItem, AttentionListResponse};

use crate::OutputFormat;

fn value(item: &AttentionItem) -> serde_json::Value {
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

pub(crate) fn print_list(response: &AttentionListResponse, format: OutputFormat) {
    if format == OutputFormat::Table {
        if response.items.is_empty() {
            println!("No attention items found.");
            return;
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
        return;
    }
    let values = response.items.iter().map(value).collect::<Vec<_>>();
    print_value(
        &serde_json::json!({
            "items": values,
            "latest_change_id": response.latest_change_id,
        }),
        format,
    );
}

pub(crate) fn print_item(item: &AttentionItem, format: OutputFormat) {
    print_value(&value(item), format);
}

pub(crate) fn print_delta(delta: &AttentionDelta, format: OutputFormat) {
    print_value(
        &serde_json::json!({
            "kind": delta.kind,
            "change_id": delta.change_id,
            "item": delta.item.as_ref().map(value),
        }),
        format,
    );
}

fn print_value(value: &serde_json::Value, format: OutputFormat) {
    match format {
        OutputFormat::Json | OutputFormat::Table => {
            println!("{}", serde_json::to_string(value).unwrap_or_default());
        }
        OutputFormat::Yaml => {
            print!("{}", serde_yaml::to_string(value).unwrap_or_default());
        }
    }
}
