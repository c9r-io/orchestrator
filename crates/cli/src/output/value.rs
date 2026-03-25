use orchestrator_proto::{
    AgentStatus, CommandRun, Event, TaskGraphDebugBundle, TaskInfoResponse, TaskItem, TaskSummary,
};
use serde_json::{Value, json};

pub(super) fn task_detail_value(task: &TaskSummary, resp: &TaskInfoResponse) -> Value {
    // FR-054: Aggregate step-level progress from runs
    let mut phase_stats: std::collections::BTreeMap<&str, (u32, u32)> =
        std::collections::BTreeMap::new();
    for run in &resp.runs {
        let entry = phase_stats.entry(run.phase.as_str()).or_default();
        if run.exit_code.is_some() {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    let step_progress: Vec<Value> = phase_stats
        .iter()
        .map(|(phase, (completed, running))| {
            json!({
                "phase": phase,
                "completed": completed,
                "running": running,
            })
        })
        .collect();

    json!({
        "task": task_summary_value(task),
        "step_progress": step_progress,
        "items": resp.items.iter().map(task_item_value).collect::<Vec<_>>(),
        "runs": resp.runs.iter().map(command_run_value).collect::<Vec<_>>(),
        "events": resp.events.iter().map(event_value).collect::<Vec<_>>(),
        "graph_debug": resp
            .graph_debug
            .iter()
            .map(task_graph_debug_value)
            .collect::<Vec<_>>(),
        "agent_states": resp
            .agent_states
            .iter()
            .map(agent_status_value)
            .collect::<Vec<_>>(),
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
        "created_at": task.created_at,
        "updated_at": task.updated_at,
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

pub(super) fn agent_status_value(a: &AgentStatus) -> Value {
    json!({
        "name": a.name,
        "enabled": a.enabled,
        "lifecycle_state": a.lifecycle_state,
        "in_flight_items": a.in_flight_items,
        "capabilities": a.capabilities,
        "drain_requested_at": a.drain_requested_at,
        "is_healthy": a.is_healthy,
        "diseased_until": a.diseased_until,
        "consecutive_errors": a.consecutive_errors,
    })
}

pub(super) fn task_graph_debug_value(bundle: &TaskGraphDebugBundle) -> Value {
    json!({
        "graph_run_id": bundle.graph_run_id,
        "cycle": bundle.cycle,
        "source": bundle.source,
        "status": bundle.status,
        "fallback_mode": bundle.fallback_mode,
        "planner_failure_class": bundle.planner_failure_class,
        "planner_failure_message": bundle.planner_failure_message,
        "effective_graph": serde_json::from_str::<Value>(&bundle.effective_graph_json)
            .unwrap_or_else(|_| Value::String(bundle.effective_graph_json.clone())),
        "planner_raw_output": bundle.planner_raw_output_json.as_ref().map(|json| {
            serde_json::from_str::<Value>(json).unwrap_or_else(|_| Value::String(json.clone()))
        }),
        "normalized_plan": bundle.normalized_plan_json.as_ref().map(|json| {
            serde_json::from_str::<Value>(json).unwrap_or_else(|_| Value::String(json.clone()))
        }),
        "execution_replay": bundle.execution_replay_json.as_ref().map(|json| {
            serde_json::from_str::<Value>(json).unwrap_or_else(|_| Value::String(json.clone()))
        }),
        "created_at": bundle.created_at,
        "updated_at": bundle.updated_at,
    })
}
