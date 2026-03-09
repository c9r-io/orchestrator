use crate::OutputFormat;
use orchestrator_proto::{CommandRun, Event, TaskInfoResponse, TaskItem, TaskSummary};
use serde_json::{json, Value};

pub fn print_task_list(tasks: &[TaskSummary], format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            let json: Vec<serde_json::Value> = tasks
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "name": t.name,
                        "status": t.status,
                        "goal": t.goal,
                        "total_items": t.total_items,
                        "finished_items": t.finished_items,
                        "failed_items": t.failed_items,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
        }
        OutputFormat::Yaml => {
            let yaml: Vec<serde_json::Value> = tasks
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "name": t.name,
                        "status": t.status,
                        "goal": t.goal,
                    })
                })
                .collect();
            println!("{}", serde_yml::to_string(&yaml).unwrap_or_default());
        }
        OutputFormat::Table => {
            println!(
                "{:<38} {:<12} {:<10} {:<8} {:<8}",
                "ID", "NAME", "STATUS", "FINISHED", "FAILED"
            );
            println!("{:-<38} {:-<12} {:-<10} {:-<8} {:-<8}", "", "", "", "", "");
            for t in tasks {
                let id_display = if t.id.len() > 8 { &t.id[..8] } else { &t.id };
                let name_display = if t.name.len() > 12 {
                    &t.name[..12]
                } else {
                    &t.name
                };
                println!(
                    "{:<38} {:<12} {:<10} {:<8} {:<8}",
                    id_display, name_display, t.status, t.finished_items, t.failed_items
                );
            }
        }
    }
}

pub fn print_task_detail(resp: &TaskInfoResponse, format: OutputFormat) {
    let Some(ref task) = resp.task else {
        println!("No task data");
        return;
    };

    match format {
        OutputFormat::Json => {
            let json = task_detail_value(task, resp);
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
        }
        OutputFormat::Yaml => {
            let json = task_detail_value(task, resp);
            println!("{}", serde_yml::to_string(&json).unwrap_or_default());
        }
        OutputFormat::Table => {
            println!("Task: {}", task.id);
            println!("  Name: {}", task.name);
            println!("  Status: {}", task.status);
            println!("  Workspace: {}", task.workspace_id);
            println!("  Workflow: {}", task.workflow_id);
            println!(
                "  Progress: {}/{} items",
                task.finished_items, task.total_items
            );
            println!("  Failed: {}", task.failed_items);
            if !task.goal.is_empty() {
                println!("  Goal: {}", task.goal);
            }
            println!("  Items: {}", resp.items.len());
            for item in &resp.items {
                println!(
                    "    - {} [{}] order={} path={}",
                    item.id, item.status, item.order_no, item.qa_file_path
                );
            }
            println!("  Runs: {}", resp.runs.len());
            for run in &resp.runs {
                let exit_code = run
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "running".to_string());
                println!(
                    "    - {} item={} phase={} exit={}",
                    run.id, run.task_item_id, run.phase, exit_code
                );
            }
            println!("  Events: {}", resp.events.len());
            for event in &resp.events {
                let item_id = event.task_item_id.as_deref().unwrap_or("-");
                println!(
                    "    - {} {} item={} at={}",
                    event.id, event.event_type, item_id, event.created_at
                );
            }
        }
    }
}

fn task_detail_value(task: &TaskSummary, resp: &TaskInfoResponse) -> Value {
    json!({
        "task": task_summary_value(task),
        "items": resp.items.iter().map(task_item_value).collect::<Vec<_>>(),
        "runs": resp.runs.iter().map(command_run_value).collect::<Vec<_>>(),
        "events": resp.events.iter().map(event_value).collect::<Vec<_>>(),
    })
}

fn task_summary_value(task: &TaskSummary) -> Value {
    json!({
        "id": task.id,
        "name": task.name,
        "status": task.status,
        "goal": task.goal,
        "workspace_id": task.workspace_id,
        "workflow_id": task.workflow_id,
        "total_items": task.total_items,
        "finished_items": task.finished_items,
        "failed_items": task.failed_items,
    })
}

fn task_item_value(item: &TaskItem) -> Value {
    json!({
        "id": item.id,
        "task_id": item.task_id,
        "order_no": item.order_no,
        "qa_file_path": item.qa_file_path,
        "status": item.status,
        "ticket_files": item.ticket_files,
        "ticket_content_json": item.ticket_content_json,
        "fix_required": item.fix_required,
        "fixed": item.fixed,
        "last_error": item.last_error,
        "started_at": item.started_at,
        "completed_at": item.completed_at,
        "updated_at": item.updated_at,
    })
}

fn command_run_value(run: &CommandRun) -> Value {
    json!({
        "id": run.id,
        "task_item_id": run.task_item_id,
        "phase": run.phase,
        "command": run.command,
        "cwd": run.cwd,
        "workspace_id": run.workspace_id,
        "agent_id": run.agent_id,
        "exit_code": run.exit_code,
        "stdout_path": run.stdout_path,
        "stderr_path": run.stderr_path,
        "started_at": run.started_at,
        "ended_at": run.ended_at,
        "interrupted": run.interrupted,
    })
}

fn event_value(event: &Event) -> Value {
    let payload = serde_json::from_str::<Value>(&event.payload_json)
        .unwrap_or_else(|_| Value::String(event.payload_json.clone()));
    json!({
        "id": event.id,
        "task_id": event.task_id,
        "task_item_id": event.task_item_id,
        "event_type": event.event_type,
        "payload": payload,
        "created_at": event.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::task_detail_value;
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
                parent_task_id: None,
                spawn_reason: None,
                spawn_depth: 0,
            }),
            items: vec![TaskItem {
                id: "item-1".into(),
                task_id: "task-1".into(),
                order_no: 7,
                qa_file_path: "docs/qa/test.md".into(),
                status: "qa_failed".into(),
                ticket_files: vec!["docs/ticket/t1.md".into()],
                ticket_content_json: "{\"severity\":\"high\"}".into(),
                fix_required: true,
                fixed: false,
                last_error: "boom".into(),
                started_at: Some("2026-03-10T00:01:00Z".into()),
                completed_at: None,
                updated_at: "2026-03-10T00:02:00Z".into(),
            }],
            runs: vec![CommandRun {
                id: "run-1".into(),
                task_item_id: "item-1".into(),
                phase: "qa".into(),
                command: "echo hi".into(),
                cwd: "/tmp".into(),
                workspace_id: "ws-1".into(),
                agent_id: "agent-1".into(),
                exit_code: Some(1),
                stdout_path: "/tmp/out.log".into(),
                stderr_path: "/tmp/err.log".into(),
                started_at: "2026-03-10T00:01:00Z".into(),
                ended_at: Some("2026-03-10T00:01:05Z".into()),
                interrupted: false,
            }],
            events: vec![Event {
                id: 42,
                task_id: "task-1".into(),
                task_item_id: Some("item-1".into()),
                event_type: "task_failed".into(),
                payload_json: "{\"reason\":\"boom\"}".into(),
                created_at: "2026-03-10T00:01:06Z".into(),
            }],
        };

        let value = task_detail_value(resp.task.as_ref().unwrap(), &resp);

        assert_eq!(value["items"][0]["id"], "item-1");
        assert_eq!(value["items"][0]["order_no"], 7);
        assert_eq!(value["runs"][0]["id"], "run-1");
        assert_eq!(value["runs"][0]["task_item_id"], "item-1");
        assert_eq!(value["events"][0]["id"], 42);
        assert_eq!(value["events"][0]["payload"]["reason"], "boom");
    }

    #[test]
    fn task_detail_value_preserves_non_json_event_payloads() {
        let resp = TaskInfoResponse {
            task: Some(TaskSummary {
                id: "task-1".into(),
                name: "task-name".into(),
                status: "running".into(),
                started_at: None,
                completed_at: None,
                goal: "".into(),
                project_id: "project-1".into(),
                workspace_id: "ws-1".into(),
                workflow_id: "wf-1".into(),
                target_files: vec![],
                total_items: 0,
                finished_items: 0,
                failed_items: 0,
                created_at: "2026-03-10T00:00:00Z".into(),
                updated_at: "2026-03-10T00:00:00Z".into(),
                parent_task_id: None,
                spawn_reason: None,
                spawn_depth: 0,
            }),
            items: vec![],
            runs: vec![],
            events: vec![Event {
                id: 1,
                task_id: "task-1".into(),
                task_item_id: None,
                event_type: "note".into(),
                payload_json: "not-json".into(),
                created_at: "2026-03-10T00:01:06Z".into(),
            }],
        };

        let value = task_detail_value(resp.task.as_ref().unwrap(), &resp);

        assert_eq!(value["events"][0]["payload"], "not-json");
    }
}
