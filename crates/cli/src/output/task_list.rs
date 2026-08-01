use anyhow::Result;
use orchestrator_proto::TaskSummary;

use crate::OutputFormat;

use super::{render, value};

pub(super) fn print(tasks: &[TaskSummary], format: OutputFormat) -> Result<()> {
    let projected = serde_json::Value::Array(tasks.iter().map(value::task_summary_value).collect());
    match format.encoding() {
        Some(encoding) => render::emit(&projected, encoding),
        None => {
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
            Ok(())
        }
    }
}
