use orchestrator_proto::{CommandRun, Event, TaskGraphDebugBundle, TaskItem, TaskSummary};

pub(super) fn summary_to_proto(t: &agent_orchestrator::dto::TaskSummary) -> TaskSummary {
    TaskSummary {
        id: t.id.clone(),
        name: t.name.clone(),
        status: t.status.clone(),
        started_at: t.started_at.clone(),
        completed_at: t.completed_at.clone(),
        goal: t.goal.clone(),
        project_id: t.project_id.clone(),
        workspace_id: t.workspace_id.clone(),
        workflow_id: t.workflow_id.clone(),
        target_files: t.target_files.clone(),
        total_items: t.total_items,
        finished_items: t.finished_items,
        failed_items: t.failed_items,
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        parent_task_id: t.parent_task_id.clone(),
        spawn_reason: t.spawn_reason.clone(),
        spawn_depth: t.spawn_depth,
    }
}

pub(super) fn item_to_proto(i: agent_orchestrator::dto::TaskItemDto) -> TaskItem {
    TaskItem {
        id: i.id,
        task_id: i.task_id,
        order_no: i.order_no,
        qa_file_path: i.qa_file_path,
        status: i.status,
        ticket_files: i.ticket_files,
        ticket_content_json: serde_json::to_string(&i.ticket_content).unwrap_or_default(),
        fix_required: i.fix_required,
        fixed: i.fixed,
        last_error: i.last_error,
        started_at: i.started_at,
        completed_at: i.completed_at,
        updated_at: i.updated_at,
    }
}

pub(super) fn run_to_proto(r: agent_orchestrator::dto::CommandRunDto) -> CommandRun {
    CommandRun {
        id: r.id,
        task_item_id: r.task_item_id,
        phase: r.phase,
        command: r.command,
        cwd: r.cwd,
        workspace_id: r.workspace_id,
        agent_id: r.agent_id,
        exit_code: r.exit_code,
        stdout_path: r.stdout_path,
        stderr_path: r.stderr_path,
        started_at: r.started_at,
        ended_at: r.ended_at,
        interrupted: r.interrupted,
    }
}

pub(super) fn event_to_proto(e: agent_orchestrator::dto::EventDto) -> Event {
    Event {
        id: e.id,
        task_id: e.task_id,
        task_item_id: e.task_item_id,
        event_type: e.event_type,
        payload_json: serde_json::to_string(&e.payload).unwrap_or_default(),
        created_at: e.created_at,
    }
}

pub(super) fn graph_debug_to_proto(
    bundle: agent_orchestrator::dto::TaskGraphDebugBundle,
) -> TaskGraphDebugBundle {
    TaskGraphDebugBundle {
        graph_run_id: bundle.graph_run_id,
        cycle: bundle.cycle,
        source: bundle.source,
        status: bundle.status,
        fallback_mode: bundle.fallback_mode,
        planner_failure_class: bundle.planner_failure_class,
        planner_failure_message: bundle.planner_failure_message,
        effective_graph_json: bundle.effective_graph_json,
        planner_raw_output_json: bundle.planner_raw_output_json,
        normalized_plan_json: bundle.normalized_plan_json,
        execution_replay_json: bundle.execution_replay_json,
        created_at: bundle.created_at,
        updated_at: bundle.updated_at,
    }
}
