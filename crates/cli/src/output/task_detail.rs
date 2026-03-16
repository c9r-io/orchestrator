use orchestrator_proto::TaskInfoResponse;

use crate::OutputFormat;

use super::value::task_detail_value;

pub(super) fn print(resp: &TaskInfoResponse, format: OutputFormat) {
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
            // FR-054: Step-level progress breakdown from runs
            if !resp.runs.is_empty() {
                let mut phase_stats: std::collections::BTreeMap<
                    &str,
                    (u32, u32), // (completed, running)
                > = std::collections::BTreeMap::new();
                for run in &resp.runs {
                    let entry = phase_stats.entry(run.phase.as_str()).or_default();
                    if run.exit_code.is_some() {
                        entry.0 += 1;
                    } else {
                        entry.1 += 1;
                    }
                }
                for (phase, (completed, running)) in &phase_stats {
                    if *running > 0 {
                        println!(
                            "    {:<20} {} completed, {} running",
                            format!("{}:", phase),
                            completed,
                            running
                        );
                    } else {
                        println!("    {:<20} {} completed", format!("{}:", phase), completed);
                    }
                }
            }
            println!("  Failed: {}", task.failed_items);
            if !task.goal.is_empty() {
                println!("  Goal: {}", task.goal);
            }
            println!("  Items: {}", resp.items.len());
            for item in &resp.items {
                let blocked_tag = if item.status == "blocked" {
                    " [BLOCKED]"
                } else {
                    ""
                };
                println!(
                    "    - {} [{}]{} order={} path={}",
                    item.id, item.status, blocked_tag, item.order_no, item.qa_file_path
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
            if let Some(graph) = resp.graph_debug.first() {
                println!(
                    "  Graph: cycle={} source={} status={}",
                    graph.cycle, graph.source, graph.status
                );
            } else {
                println!("  Graph: none");
            }
            if !resp.agent_states.is_empty() {
                println!("  Agents:");
                println!(
                    "    {:<20} {:<8} {:<10} {:<10} CAPABILITIES",
                    "NAME", "ENABLED", "STATE", "IN-FLIGHT"
                );
                for a in &resp.agent_states {
                    println!(
                        "    {:<20} {:<8} {:<10} {:<10} {}",
                        a.name,
                        a.enabled,
                        a.lifecycle_state,
                        a.in_flight_items,
                        a.capabilities.join(", ")
                    );
                }
            }
        }
    }
}
