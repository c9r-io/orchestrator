use orchestrator_proto::{CommandRun, Event, TaskInfoResponse, TaskItem, TaskSummary};
use serde_json::{json, Value};

pub(super) fn task_detail_value(task: &TaskSummary, resp: &TaskInfoResponse) -> Value {
    json!({
        "task": task_summary_value(task),
        "items": resp.items.iter().map(task_item_value).collect::<Vec<_>>(),
        "runs": resp.runs.iter().map(command_run_value).collect::<Vec<_>>(),
        "events": resp.events.iter().map(event_value).collect::<Vec<_>>(),
    })
}

pub(super) fn task_summary_value(task: &TaskSummary) -> Value {
    json!({
        "id": task.id,
        "name": task.name,
        "status": task.status,
        "goal": task.goal,
        "project_id": task.project_id,
        "workspace_id": task.workspace_id,
        "workflow_id": task.workflow_id,
        "total_items": task.total_items,
        "finished_items": task.finished_items,
        "failed_items": task.failed_items,
        "parent_task_id": task.parent_task_id,
        "spawn_reason": task.spawn_reason,
        "spawn_depth": task.spawn_depth,
    })
}

pub(super) fn task_item_value(item: &TaskItem) -> Value {
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

pub(super) fn command_run_value(run: &CommandRun) -> Value {
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

pub(super) fn event_value(event: &Event) -> Value {
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
