use orchestrator_proto::{TaskTimelineResponse, TimelineDelta, TimelineEntry};

use crate::OutputFormat;

pub fn print_response(response: &TaskTimelineResponse, format: OutputFormat) {
    match format {
        OutputFormat::Table => {
            if response.entries.is_empty() {
                println!("No timeline entries found.");
            } else {
                print_table_header();
                for entry in &response.entries {
                    print_table_entry(entry);
                }
            }
            if response.has_more {
                println!(
                    "\nMore entries available. Continue with --cursor {}",
                    response.next_cursor.as_deref().unwrap_or_default()
                );
            }
        }
        OutputFormat::Json => print_serialized(&response_value(response), false),
        OutputFormat::Yaml => print_serialized(&response_value(response), true),
    }
}

pub fn print_delta(delta: &TimelineDelta, format: OutputFormat) {
    match format {
        OutputFormat::Table => {
            if delta.kind == "reset_required" {
                println!(
                    "Timeline changed too quickly; refresh required at event {}.",
                    delta.snapshot_max_event_id
                );
            } else if let Some(entry) = &delta.entry {
                print_table_entry(entry);
            }
        }
        OutputFormat::Json => print_serialized(&delta_value(delta), false),
        OutputFormat::Yaml => print_serialized(&delta_value(delta), true),
    }
}

fn print_table_header() {
    println!(
        "{:<20} {:<12} {:<12} {:<28} SUMMARY",
        "OCCURRED", "CATEGORY", "STATUS", "TITLE"
    );
}

fn print_table_entry(entry: &TimelineEntry) {
    println!(
        "{:<20} {:<12} {:<12} {:<28} {}{}",
        truncate(&entry.occurred_at, 20),
        truncate(&entry.category, 12),
        truncate(entry.status.as_deref().unwrap_or("-"), 12),
        truncate(&entry.title, 28),
        entry.summary,
        evidence_suffix(entry),
    );
}

fn evidence_suffix(entry: &TimelineEntry) -> String {
    if entry.evidence.is_empty() {
        String::new()
    } else {
        format!(" [{} evidence]", entry.evidence.len())
    }
}

fn truncate(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        return value.to_owned();
    }
    value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn response_value(response: &TaskTimelineResponse) -> serde_json::Value {
    serde_json::json!({
        "entries": response.entries.iter().map(entry_value).collect::<Vec<_>>(),
        "next_cursor": response.next_cursor,
        "has_more": response.has_more,
        "snapshot_max_event_id": response.snapshot_max_event_id,
        "projection_version": response.projection_version,
    })
}

fn delta_value(delta: &TimelineDelta) -> serde_json::Value {
    serde_json::json!({
        "kind": delta.kind,
        "entry": delta.entry.as_ref().map(entry_value),
        "snapshot_max_event_id": delta.snapshot_max_event_id,
    })
}

fn entry_value(entry: &TimelineEntry) -> serde_json::Value {
    serde_json::json!({
        "id": entry.id,
        "task_id": entry.task_id,
        "occurred_at": entry.occurred_at,
        "category": entry.category,
        "title": entry.title,
        "summary": entry.summary,
        "status": entry.status,
        "actor": entry.actor.as_ref().map(|actor| serde_json::json!({
            "type": actor.actor_type,
            "id": actor.actor_id,
        })),
        "step_id": entry.step_id,
        "task_item_id": entry.task_item_id,
        "command_run_id": entry.command_run_id,
        "session_id": entry.session_id,
        "checkpoint_id": entry.checkpoint_id,
        "source_event_id": entry.source_event_id,
        "evidence": entry.evidence.iter().map(|evidence| serde_json::json!({
            "kind": evidence.kind,
            "label": evidence.label,
            "uri": evidence.uri,
            "content_type": evidence.content_type,
            "digest": evidence.digest,
            "redacted": evidence.redacted,
        })).collect::<Vec<_>>(),
        "raw_event_ids": entry.raw_event_ids,
        "projection_version": entry.projection_version,
    })
}

fn print_serialized(value: &serde_json::Value, yaml: bool) {
    if yaml {
        println!("{}", serde_yaml::to_string(value).unwrap_or_default());
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_respects_utf8_boundaries() {
        assert_eq!(truncate("时间线条目", 4), "时间线…");
    }
}
