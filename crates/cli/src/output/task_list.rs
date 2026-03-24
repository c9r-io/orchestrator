use orchestrator_proto::TaskSummary;

use crate::OutputFormat;

pub(super) fn print(tasks: &[TaskSummary], format: OutputFormat) {
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
            println!("{}", serde_yaml::to_string(&yaml).unwrap_or_default());
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
