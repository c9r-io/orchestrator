mod task_detail;
mod task_list;
mod value;

use orchestrator_proto::{Event, TaskInfoResponse, TaskItem, TaskSummary};

use crate::OutputFormat;

/// Render a list of task summaries in the requested output format.
pub fn print_task_list(tasks: &[TaskSummary], format: OutputFormat) {
    task_list::print(tasks, format);
}

/// Render a task detail payload in the requested output format.
pub fn print_task_detail(resp: &TaskInfoResponse, format: OutputFormat) {
    task_detail::print(resp, format);
}

/// Render task items in the requested output format.
pub fn print_task_items(items: &[TaskItem], format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            let arr: Vec<serde_json::Value> = items
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "id": item.id,
                        "label": item.qa_file_path,
                        "status": item.status,
                        "order_no": item.order_no,
                        "fix_required": item.fix_required,
                        "fixed": item.fixed,
                        "last_error": item.last_error,
                        "started_at": item.started_at,
                        "completed_at": item.completed_at,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        }
        OutputFormat::Yaml => {
            let arr: Vec<serde_json::Value> = items
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "id": item.id,
                        "label": item.qa_file_path,
                        "status": item.status,
                        "order_no": item.order_no,
                    })
                })
                .collect();
            println!("{}", serde_yaml::to_string(&arr).unwrap_or_default());
        }
        OutputFormat::Table => {
            if items.is_empty() {
                println!("No items found.");
                return;
            }
            println!(
                "{:<8} {:<40} {:<12} {:<8}",
                "ORDER", "LABEL", "STATUS", "FIXED"
            );
            for item in items {
                let label = if item.qa_file_path.len() > 38 {
                    format!("..{}", &item.qa_file_path[item.qa_file_path.len() - 36..])
                } else {
                    item.qa_file_path.clone()
                };
                println!(
                    "{:<8} {:<40} {:<12} {:<8}",
                    item.order_no,
                    label,
                    item.status,
                    if item.fixed { "yes" } else { "no" },
                );
            }
            println!("\n{} item(s)", items.len());
        }
    }
}

/// Render an event list in the requested output format.
pub fn print_event_list(events: &[Event], format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            let arr: Vec<serde_json::Value> = events
                .iter()
                .map(|e| {
                    let payload: serde_json::Value =
                        serde_json::from_str(&e.payload_json).unwrap_or(serde_json::Value::Null);
                    serde_json::json!({
                        "id": e.id,
                        "event_type": e.event_type,
                        "task_item_id": e.task_item_id,
                        "payload": payload,
                        "created_at": e.created_at,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        }
        OutputFormat::Yaml => {
            let arr: Vec<serde_json::Value> = events
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "event_type": e.event_type,
                        "payload_json": e.payload_json,
                        "created_at": e.created_at,
                    })
                })
                .collect();
            println!("{}", serde_yaml::to_string(&arr).unwrap_or_default());
        }
        OutputFormat::Table => {
            if events.is_empty() {
                println!("No events found.");
                return;
            }
            println!("{:<8} {:<28} {:<60} CREATED", "ID", "TYPE", "PAYLOAD");
            for e in events {
                let payload = if e.payload_json.len() > 58 {
                    format!("{}...", &e.payload_json[..55])
                } else {
                    e.payload_json.clone()
                };
                println!(
                    "{:<8} {:<28} {:<60} {}",
                    e.id, e.event_type, payload, e.created_at
                );
            }
            println!("\n{} event(s)", events.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::value::task_detail_value;
    use orchestrator_proto::{CommandRun, Event, TaskInfoResponse, TaskItem, TaskSummary};

    #[test]
    fn task_detail_value_includes_item_run_and_event_details() {
        let resp = TaskInfoResponse {
            task: Some(TaskSummary {
                id: "task-1".into(),
                name: "task-name".into(),
                status: "failed".into(),
                started_at: None,
                completed_at: None,
                goal: "goal".into(),
                project_id: "project-1".into(),
                workspace_id: "ws-1".into(),
                workflow_id: "wf-1".into(),
                target_files: vec![],
                total_items: 1,
                finished_items: 0,
                failed_items: 1,
                created_at: "2026-03-10T00:00:00Z".into(),
                updated_at: "2026-03-10T00:00:00Z".into(),
                parent_task_id: Some("task-0".into()),
                spawn_reason: Some("retry".into()),
                spawn_depth: 1,
            }),
            items: vec![TaskItem {
                id: "item-1".into(),
                task_id: "task-1".into(),
                order_no: 7,
                qa_file_path: "docs/qa/case.md".into(),
                status: "failed".into(),
                ticket_files: vec!["docs/ticket/bug.md".into()],
                ticket_content_json: "{\"severity\":\"high\"}".into(),
                fix_required: true,
                fixed: false,
                last_error: "boom".into(),
                started_at: Some("2026-03-10T00:01:00Z".into()),
                completed_at: Some("2026-03-10T00:02:00Z".into()),
                updated_at: "2026-03-10T00:02:00Z".into(),
            }],
            runs: vec![CommandRun {
                id: "run-1".into(),
                task_item_id: "item-1".into(),
                phase: "qa".into(),
                command: "qa-doc-gen".into(),
                cwd: "/tmp/workspace".into(),
                workspace_id: "ws-1".into(),
                agent_id: "agent-1".into(),
                exit_code: Some(1),
                stdout_path: "/tmp/out.log".into(),
                stderr_path: "/tmp/err.log".into(),
                started_at: "2026-03-10T00:01:00Z".into(),
                ended_at: Some("2026-03-10T00:02:00Z".into()),
                interrupted: false,
            }],
            events: vec![Event {
                id: 1,
                task_id: "task-1".into(),
                task_item_id: Some("item-1".into()),
                event_type: "task_failed".into(),
                payload_json: "{\"reason\":\"timeout\"}".into(),
                created_at: "2026-03-10T00:03:00Z".into(),
            }],
            graph_debug: vec![],
            agent_states: vec![],
        };

        let task = resp.task.as_ref().expect("task");
        let json = task_detail_value(task, &resp);

        assert_eq!(json["task"]["project_id"], "project-1");
        assert_eq!(json["task"]["parent_task_id"], "task-0");
        assert_eq!(json["task"]["spawn_reason"], "retry");
        assert_eq!(json["task"]["spawn_depth"], 1);
        assert_eq!(json["items"][0]["ticket_files"][0], "docs/ticket/bug.md");
        assert_eq!(json["items"][0]["last_error"], "boom");
        assert_eq!(json["runs"][0]["agent_id"], "agent-1");
        assert_eq!(json["events"][0]["payload"]["reason"], "timeout");
    }
}
