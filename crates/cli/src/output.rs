use crate::OutputFormat;
use orchestrator_proto::{TaskSummary, TaskInfoResponse};

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
            println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
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
            let json = serde_json::json!({
                "task": {
                    "id": task.id,
                    "name": task.name,
                    "status": task.status,
                    "goal": task.goal,
                    "workspace_id": task.workspace_id,
                    "workflow_id": task.workflow_id,
                    "total_items": task.total_items,
                    "finished_items": task.finished_items,
                    "failed_items": task.failed_items,
                },
                "items": resp.items.len(),
                "runs": resp.runs.len(),
                "events": resp.events.len(),
            });
            println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
        }
        OutputFormat::Yaml => {
            let json = serde_json::json!({
                "id": task.id,
                "name": task.name,
                "status": task.status,
                "goal": task.goal,
            });
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
        }
    }
}
