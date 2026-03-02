use crate::cli::ConfigLifecycleCommands;
use crate::config_load::{query_heal_log_entries, HealLogEntry};
use anyhow::Result;

use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_config_lifecycle(&self, cmd: &ConfigLifecycleCommands) -> Result<i32> {
        match cmd {
            ConfigLifecycleCommands::HealLog { limit, json } => {
                self.handle_heal_log(*limit, *json)
            }
            ConfigLifecycleCommands::BackfillEvents => self.handle_backfill_events(),
        }
    }

    fn handle_heal_log(&self, limit: usize, json: bool) -> Result<i32> {
        let entries = query_heal_log_entries(&self.state.db_path, limit)?;

        if json {
            println!("{}", serde_json::to_string_pretty(&entries)?);
            return Ok(0);
        }

        if entries.is_empty() {
            println!("config heal-log — no self-heal events recorded");
            return Ok(0);
        }

        println!("config heal-log — recent self-heal events\n");
        render_heal_log_table(&entries);
        Ok(0)
    }

    fn handle_backfill_events(&self) -> Result<i32> {
        let stats = crate::events_backfill::backfill_event_step_scope(&self.state.db_path)?;
        println!(
            "scanned {} events, updated {}, skipped {} (already had step_scope)",
            stats.scanned, stats.updated, stats.skipped
        );
        Ok(0)
    }
}

fn render_heal_log_table(entries: &[HealLogEntry]) {
    let mut current_version: Option<i64> = None;

    for (idx, entry) in entries.iter().enumerate() {
        if current_version != Some(entry.version) {
            if idx > 0 {
                println!();
            }
            println!(
                "version {} | {} | triggered by: \"{}\"",
                entry.version, entry.created_at, entry.original_error
            );
            current_version = Some(entry.version);
        }
        println!(
            "  {}/{}  {}",
            entry.workflow_id, entry.step_id, entry.rule
        );
        println!("      {}", entry.detail);
    }
}
