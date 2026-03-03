use crate::cli::{OutputFormat, QaCommands, QaProjectCommands};
use crate::config::{ProjectConfig, WorkspaceConfig};
use crate::config_load::{persist_config_and_reload, read_active_config};
use anyhow::{Context, Result};
use serde_json::json;

use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_qa(&self, cmd: &QaCommands) -> Result<i32> {
        match cmd {
            QaCommands::Project(project_cmd) => self.handle_qa_project(project_cmd),
            QaCommands::Doctor { output } => self.handle_qa_doctor(*output),
        }
    }

    fn handle_qa_project(&self, cmd: &QaProjectCommands) -> Result<i32> {
        match cmd {
            QaProjectCommands::Create {
                project_id,
                from_workspace,
                workflow,
                workspace,
                root_path,
                qa_target,
                ticket_dir,
                force,
            } => {
                let mut config = {
                    let active = read_active_config(&self.state)?;
                    active.config.clone()
                };

                let source_workspace = config
                    .workspaces
                    .get(from_workspace)
                    .with_context(|| format!("source workspace not found: {}", from_workspace))?
                    .clone();
                let workflow_id = workflow
                    .clone()
                    .unwrap_or_else(|| config.defaults.workflow.clone());
                let source_workflow = config
                    .workflows
                    .get(&workflow_id)
                    .with_context(|| format!("workflow not found: {}", workflow_id))?
                    .clone();

                let workspace_id = workspace
                    .clone()
                    .unwrap_or_else(|| format!("{}-ws", project_id));
                let resolved_root_path = root_path
                    .clone()
                    .unwrap_or_else(|| format!("workspace/{}", project_id));
                let resolved_qa_targets = if qa_target.is_empty() {
                    source_workspace.qa_targets
                } else {
                    qa_target.clone()
                };

                let new_workspace = WorkspaceConfig {
                    root_path: resolved_root_path.clone(),
                    qa_targets: resolved_qa_targets,
                    ticket_dir: ticket_dir.clone(),
                    self_referential: false,
                };

                let project = config
                    .projects
                    .entry(project_id.clone())
                    .or_insert_with(|| ProjectConfig {
                        description: Some("qa isolated project".to_string()),
                        workspaces: std::collections::HashMap::new(),
                        agents: std::collections::HashMap::new(),
                        workflows: std::collections::HashMap::new(),
                    });

                if !*force && !project.workspaces.is_empty() {
                    anyhow::bail!(
                        "project '{}' already exists; pass --force to overwrite project workspace/workflow",
                        project_id
                    );
                }

                project
                    .workspaces
                    .insert(workspace_id.clone(), new_workspace);
                project
                    .workflows
                    .insert(workflow_id.clone(), source_workflow);

                let workspace_root = self.state.app_root.join(&resolved_root_path);
                std::fs::create_dir_all(&workspace_root).with_context(|| {
                    format!(
                        "failed to create workspace root for project '{}': {}",
                        project_id,
                        workspace_root.display()
                    )
                })?;
                if let Some(ws) = project.workspaces.get(&workspace_id) {
                    for target in &ws.qa_targets {
                        std::fs::create_dir_all(workspace_root.join(target)).with_context(
                            || {
                                format!(
                                    "failed to create qa target dir for project '{}': {}",
                                    project_id, target
                                )
                            },
                        )?;
                    }
                    std::fs::create_dir_all(workspace_root.join(&ws.ticket_dir)).with_context(
                        || {
                            format!(
                                "failed to create ticket dir for project '{}': {}",
                                project_id, ws.ticket_dir
                            )
                        },
                    )?;
                }

                let yaml = serde_yaml::to_string(&config)
                    .context("failed to serialize configuration after qa project create")?;
                persist_config_and_reload(&self.state, config, yaml, "qa-project-create")?;

                println!(
                    "qa project created: project={} workspace={} workflow={}",
                    project_id, workspace_id, workflow_id
                );
                Ok(0)
            }
            QaProjectCommands::Reset {
                project_id,
                keep_config,
                force,
            } => {
                if !force {
                    println!(
                        "Use --force to confirm qa project reset for '{}' (sqlite DB file is preserved)",
                        project_id
                    );
                    return Ok(0);
                }

                let removed = crate::db::reset_project_data(&self.state, project_id)?;

                // Clean auto-generated ticket files from project workspaces
                let mut tickets_cleaned = 0u32;
                {
                    let active = read_active_config(&self.state)?;
                    if let Some(project) = active.config.projects.get(project_id) {
                        for ws in project.workspaces.values() {
                            let ticket_path =
                                std::path::Path::new(&ws.root_path).join(&ws.ticket_dir);
                            if ticket_path.is_dir() {
                                if let Ok(entries) = std::fs::read_dir(&ticket_path) {
                                    for entry in entries.flatten() {
                                        let fname = entry.file_name();
                                        let name = fname.to_string_lossy();
                                        if name.starts_with("auto_")
                                            && name.ends_with(".md")
                                            && std::fs::remove_file(entry.path()).is_ok()
                                        {
                                            tickets_cleaned += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if !keep_config {
                    let mut config = {
                        let active = read_active_config(&self.state)?;
                        active.config.clone()
                    };
                    config.projects.remove(project_id);
                    let yaml = serde_yaml::to_string(&config).context(
                        "failed to serialize configuration after qa project config cleanup",
                    )?;
                    persist_config_and_reload(&self.state, config, yaml, "qa-project-reset")?;
                }

                println!(
                    "qa project reset completed: project={} tasks={} items={} runs={} events={} tickets_cleaned={} config_kept={}",
                    project_id,
                    removed.tasks,
                    removed.task_items,
                    removed.command_runs,
                    removed.events,
                    tickets_cleaned,
                    keep_config
                );
                Ok(0)
            }
        }
    }

    fn handle_qa_doctor(&self, format: OutputFormat) -> Result<i32> {
        let conn = crate::db::open_conn(&self.state.db_path)?;
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .unwrap_or_default();
        let busy_timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
            .unwrap_or(0);
        let total_task_metrics: i64 =
            conn.query_row("SELECT COUNT(*) FROM task_execution_metrics", [], |row| {
                row.get(0)
            })?;
        let completed_task_metrics: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_execution_metrics WHERE status = 'completed'",
            [],
            |row| row.get(0),
        )?;
        let recent24_task_metrics: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_execution_metrics WHERE datetime(created_at) >= datetime('now', '-1 day')",
            [],
            |row| row.get(0),
        )?;
        let completion_rate = if total_task_metrics > 0 {
            (completed_task_metrics as f64) / (total_task_metrics as f64)
        } else {
            0.0
        };

        let active = read_active_config(&self.state)?;
        let checks = json!({
            "sqlite": {
                "journal_mode": journal_mode,
                "busy_timeout_ms": busy_timeout_ms,
                "pool_max_size": self.state.database.pool_max_size(),
                "pool_min_idle": self.state.database.pool_min_idle(),
                "pool_connection_timeout_ms": self.state.database.pool_connection_timeout_ms(),
            },
            "observability": {
                "task_execution_metrics_total": total_task_metrics,
                "task_execution_metrics_last_24h": recent24_task_metrics,
                "task_completion_rate": completion_rate,
            },
            "config": {
                "default_project": active.default_project_id,
                "project_count": active.config.projects.len(),
            },
            "recommendations": [
                "Use unique qa project id per scenario run",
                "Use `orchestrator qa project reset <project> --keep-config --force` between reruns",
            ]
        });

        match format {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&checks)?),
            OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&checks)?),
            OutputFormat::Table => {
                println!("QA Doctor");
                println!("---------");
                println!("sqlite.journal_mode: {}", checks["sqlite"]["journal_mode"]);
                println!(
                    "sqlite.busy_timeout_ms: {}",
                    checks["sqlite"]["busy_timeout_ms"]
                );
                println!(
                    "sqlite.pool_max_size: {}",
                    checks["sqlite"]["pool_max_size"]
                );
                println!(
                    "sqlite.pool_min_idle: {}",
                    checks["sqlite"]["pool_min_idle"]
                );
                println!(
                    "sqlite.pool_connection_timeout_ms: {}",
                    checks["sqlite"]["pool_connection_timeout_ms"]
                );
                println!(
                    "config.default_project: {}",
                    checks["config"]["default_project"]
                        .as_str()
                        .unwrap_or_default()
                );
                println!(
                    "config.project_count: {}",
                    checks["config"]["project_count"].as_u64().unwrap_or(0)
                );
                println!(
                    "observability.task_execution_metrics_total: {}",
                    checks["observability"]["task_execution_metrics_total"]
                );
                println!(
                    "observability.task_execution_metrics_last_24h: {}",
                    checks["observability"]["task_execution_metrics_last_24h"]
                );
                println!(
                    "observability.task_completion_rate: {:.3}",
                    checks["observability"]["task_completion_rate"]
                        .as_f64()
                        .unwrap_or(0.0)
                );
            }
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::CliHandler;
    use crate::cli::{Cli, Commands, QaCommands, QaProjectCommands, TaskCommands};
    use crate::config_load::read_active_config;
    use crate::db::open_conn;
    use crate::test_utils::TestState;

    #[test]
    fn qa_project_create_then_reset_keep_config_cleans_only_project_data() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let create_project = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Create {
                project_id: "qa-isolated".to_string(),
                from_workspace: "default".to_string(),
                workflow: Some("basic".to_string()),
                workspace: Some("qa-isolated-ws".to_string()),
                root_path: Some("workspace/qa-isolated".to_string()),
                qa_target: vec![],
                ticket_dir: "docs/ticket".to_string(),
                force: true,
            })),
            verbose: false,
            log_level: None,
            log_format: None,
        };
        assert_eq!(
            handler
                .execute(&create_project)
                .expect("qa project create should succeed"),
            0
        );
        let qa_file = fixture
            .temp_root()
            .join("workspace/qa-isolated/docs/qa/sample.md");
        std::fs::write(&qa_file, "# sample\n").expect("qa sample file should be writable");

        let create_task = Cli {
            command: Commands::Task(TaskCommands::Create {
                name: Some("qa-proj-task".to_string()),
                goal: Some("verify reset".to_string()),
                project: Some("qa-isolated".to_string()),
                workspace: Some("qa-isolated-ws".to_string()),
                workflow: Some("basic".to_string()),
                target_file: vec![],
                no_start: true,
                detach: false,
            }),
            verbose: false,
            log_level: None,
            log_format: None,
        };
        assert_eq!(
            handler
                .execute(&create_task)
                .expect("task create should succeed"),
            0
        );

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let before_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = 'qa-isolated'",
                [],
                |row| row.get(0),
            )
            .expect("count before reset");
        assert!(before_count >= 1);

        let reset = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Reset {
                project_id: "qa-isolated".to_string(),
                keep_config: true,
                force: true,
            })),
            verbose: false,
            log_level: None,
            log_format: None,
        };
        assert_eq!(handler.execute(&reset).expect("qa reset should succeed"), 0);

        let after_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = 'qa-isolated'",
                [],
                |row| row.get(0),
            )
            .expect("count after reset");
        assert_eq!(after_count, 0);
        drop(conn);
        assert!(state.db_path.exists());

        let active = read_active_config(&state).expect("config should be readable");
        assert!(active.config.projects.contains_key("qa-isolated"));
    }

    #[test]
    fn qa_project_reset_without_keep_config_removes_project_entry() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let create = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Create {
                project_id: "qa-drop".to_string(),
                from_workspace: "default".to_string(),
                workflow: Some("basic".to_string()),
                workspace: None,
                root_path: None,
                qa_target: vec![],
                ticket_dir: "docs/ticket".to_string(),
                force: true,
            })),
            verbose: false,
            log_level: None,
            log_format: None,
        };
        handler
            .execute(&create)
            .expect("qa project create should succeed");

        let reset = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Reset {
                project_id: "qa-drop".to_string(),
                keep_config: false,
                force: true,
            })),
            verbose: false,
            log_level: None,
            log_format: None,
        };
        handler
            .execute(&reset)
            .expect("qa project reset should succeed");

        let active = read_active_config(&state).expect("config should be readable");
        assert!(!active.config.projects.contains_key("qa-drop"));
    }
}
