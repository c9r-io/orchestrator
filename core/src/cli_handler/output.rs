use crate::cli::OutputFormat;
use crate::config::WorkspaceConfig;
use crate::dto::{TaskDetail, TaskSummary};
use anyhow::Result;
use std::path::Path;

use super::CliHandler;

impl CliHandler {
    pub(super) fn print_resource_rows<F>(
        &self,
        kind: &str,
        rows: Vec<serde_json::Value>,
        format: OutputFormat,
        table_row: F,
    ) -> Result<i32>
    where
        F: Fn(&serde_json::Value) -> String,
    {
        match format {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
            OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&rows)?),
            OutputFormat::Table => {
                println!("{kind} LIST");
                println!("{}", "-".repeat(kind.len() + 5));
                for row in &rows {
                    println!("{}", table_row(row));
                }
            }
        }
        Ok(0)
    }

    pub(super) fn print_tasks(
        &self,
        tasks: &[TaskSummary],
        format: OutputFormat,
        _verbose: bool,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(tasks)?;
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(tasks)?;
                println!("{}", yaml);
            }
            OutputFormat::Table => {
                println!(
                    "{:<38} {:<12} {:<10} {:<8} {:<8}",
                    "ID", "NAME", "STATUS", "FINISHED", "FAILED"
                );
                println!("{:-<38} {:-<12} {:-<10} {:-<8} {:-<8}", "", "", "", "", "");
                for t in tasks {
                    println!(
                        "{:<38} {:<12} {:<10} {:<8} {:<8}",
                        &t.id[..8],
                        &t.name[..std::cmp::min(12, t.name.len())],
                        t.status,
                        t.finished_items,
                        t.failed_items
                    );
                }
            }
        }
        Ok(0)
    }

    pub(super) fn print_task_detail(
        &self,
        detail: &TaskDetail,
        format: OutputFormat,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(detail)?);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(detail)?);
            }
            OutputFormat::Table => {
                let t = &detail.task;
                println!("Task: {}", t.id);
                println!("  Name: {}", t.name);
                println!("  Status: {}", t.status);
                println!("  Workspace: {}", t.workspace_id);
                println!("  Workflow: {}", t.workflow_id);
                println!("  Progress: {}/{} items", t.finished_items, t.total_items);
                println!("  Failed: {}", t.failed_items);
                if !t.goal.is_empty() {
                    println!("  Goal: {}", t.goal);
                }
                if !t.target_files.is_empty() {
                    println!("  Target Files: {:?}", t.target_files);
                }
            }
        }
        Ok(0)
    }

    pub(super) fn print_workspaces(
        &self,
        ids: &[String],
        workspaces: &std::collections::HashMap<String, WorkspaceConfig>,
        format: OutputFormat,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(workspaces)?;
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(workspaces)?);
            }
            OutputFormat::Table => {
                println!("{:<20} {:<40}", "ID", "ROOT PATH");
                println!("{:-<20} {:-<40}", "", "");
                for id in ids {
                    if let Some(ws) = workspaces.get(id) {
                        let root_path = Path::new(&ws.root_path);
                        let absolute_path = if root_path.is_absolute() {
                            root_path.to_path_buf()
                        } else {
                            self.state
                                .app_root
                                .join(&ws.root_path)
                                .canonicalize()
                                .unwrap_or_else(|_| self.state.app_root.join(&ws.root_path))
                        };
                        println!("{:<20} {:<40}", id, absolute_path.display());
                    }
                }
            }
        }
        Ok(0)
    }

    pub(super) fn print_workspace_detail(
        &self,
        id: &str,
        ws: &WorkspaceConfig,
        format: OutputFormat,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(ws)?;
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(ws)?);
            }
            OutputFormat::Table => {
                println!("Workspace: {}", id);
                println!("  Root Path: {}", ws.root_path);
                println!("  QA Targets: {:?}", ws.qa_targets);
                println!("  Ticket Dir: {}", ws.ticket_dir);
            }
        }
        Ok(0)
    }
}
