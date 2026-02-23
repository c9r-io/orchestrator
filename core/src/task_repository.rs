use crate::config_load::now_ts;
use crate::db::open_conn;
use crate::dto::{CommandRunDto, EventDto, TaskItemDto, TaskItemRow, TaskSummary};
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

pub trait TaskRepository {
    fn resolve_task_id(&self, task_id_or_prefix: &str) -> Result<String>;
    fn load_task_summary(&self, task_id: &str) -> Result<TaskSummary>;
    fn load_task_detail_rows(
        &self,
        task_id: &str,
    ) -> Result<(Vec<TaskItemDto>, Vec<CommandRunDto>, Vec<EventDto>)>;
    fn load_task_item_counts(&self, task_id: &str) -> Result<(i64, i64, i64)>;
    fn list_task_ids_ordered_by_created_desc(&self) -> Result<Vec<String>>;
    fn find_latest_resumable_task_id(&self, include_pending: bool) -> Result<Option<String>>;
    fn load_task_runtime_row(&self, task_id: &str) -> Result<TaskRuntimeRow>;
    fn first_task_item_id(&self, task_id: &str) -> Result<Option<String>>;
    fn count_unresolved_items(&self, task_id: &str) -> Result<i64>;
    fn list_task_items_for_cycle(&self, task_id: &str) -> Result<Vec<TaskItemRow>>;
    fn load_task_status(&self, task_id: &str) -> Result<Option<String>>;
    fn set_task_status(&self, task_id: &str, status: &str, set_completed: bool) -> Result<()>;
    fn prepare_task_for_start_batch(&self, task_id: &str) -> Result<()>;
    fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()>;
    fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()>;
    fn load_task_name(&self, task_id: &str) -> Result<Option<String>>;
    fn list_task_log_runs(&self, task_id: &str, limit: usize) -> Result<Vec<TaskLogRunRow>>;
    fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>>;
    fn insert_command_run(&self, run: &NewCommandRun) -> Result<()>;
}

pub struct SqliteTaskRepository {
    db_path: PathBuf,
}

pub struct TaskRuntimeRow {
    pub workspace_id: String,
    pub workflow_id: String,
    pub workspace_root_raw: String,
    pub ticket_dir: String,
    pub execution_plan_json: String,
    pub current_cycle: i64,
    pub init_done: i64,
}

pub struct TaskLogRunRow {
    pub run_id: String,
    pub phase: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub started_at: Option<String>,
}

pub struct NewCommandRun {
    pub id: String,
    pub task_item_id: String,
    pub phase: String,
    pub command: String,
    pub cwd: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub exit_code: i64,
    pub stdout_path: String,
    pub stderr_path: String,
    pub started_at: String,
    pub ended_at: String,
    pub interrupted: i64,
    pub output_json: String,
    pub artifacts_json: String,
    pub confidence: Option<f32>,
    pub quality_score: Option<f32>,
    pub validation_status: String,
}

impl SqliteTaskRepository {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }
}

impl TaskRepository for SqliteTaskRepository {
    fn resolve_task_id(&self, task_id_or_prefix: &str) -> Result<String> {
        let conn = open_conn(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT id FROM tasks WHERE id = ?1")?;
        let exact_match: Option<String> = stmt
            .query_row(params![task_id_or_prefix], |row| row.get(0))
            .optional()?;
        if let Some(id) = exact_match {
            return Ok(id);
        }

        let pattern = format!("{}%", task_id_or_prefix);
        let mut stmt = conn.prepare("SELECT id FROM tasks WHERE id LIKE ?1")?;
        let matches: Vec<String> = stmt
            .query_map(params![pattern], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        match matches.len() {
            1 => Ok(matches.into_iter().next().unwrap()),
            0 => anyhow::bail!("task not found: {}", task_id_or_prefix),
            _ => anyhow::bail!(
                "multiple tasks match prefix '{}': {:?}",
                task_id_or_prefix,
                matches
            ),
        }
    }

    fn load_task_summary(&self, task_id: &str) -> Result<TaskSummary> {
        let conn = open_conn(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, status, started_at, completed_at, goal, target_files_json, project_id, workspace_id, workflow_id, created_at, updated_at FROM tasks WHERE id = ?1",
        )?;
        stmt.query_row(params![task_id], |row| {
            let target_raw: String = row.get("target_files_json")?;
            let target_files = serde_json::from_str::<Vec<String>>(&target_raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    target_raw.len(),
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(TaskSummary {
                id: row.get("id")?,
                name: row.get("name")?,
                status: row.get("status")?,
                started_at: row.get("started_at")?,
                completed_at: row.get("completed_at")?,
                goal: row.get("goal")?,
                project_id: row.get("project_id")?,
                workspace_id: row.get("workspace_id")?,
                workflow_id: row.get("workflow_id")?,
                target_files,
                total_items: 0,
                finished_items: 0,
                failed_items: 0,
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
            })
        })
        .with_context(|| format!("load task summary for task_id={task_id}"))
    }

    fn load_task_detail_rows(
        &self,
        task_id: &str,
    ) -> Result<(Vec<TaskItemDto>, Vec<CommandRunDto>, Vec<EventDto>)> {
        let conn = open_conn(&self.db_path)?;
        let mut items_stmt = conn.prepare(
            "SELECT id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at FROM task_items WHERE task_id = ?1 ORDER BY order_no",
        )?;
        let items = items_stmt
            .query_map(params![task_id], |row| {
                let ticket_files_raw: String = row.get(5)?;
                let ticket_content_raw: String = row.get(6)?;
                let ticket_files: Vec<String> =
                    serde_json::from_str(&ticket_files_raw).unwrap_or_default();
                let ticket_content: Vec<Value> =
                    serde_json::from_str(&ticket_content_raw).unwrap_or_default();
                Ok(TaskItemDto {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    order_no: row.get(2)?,
                    qa_file_path: row.get(3)?,
                    status: row.get(4)?,
                    ticket_files,
                    ticket_content,
                    fix_required: row.get::<_, i64>(7)? == 1,
                    fixed: row.get::<_, i64>(8)? == 1,
                    last_error: row.get(9)?,
                    started_at: row.get(10)?,
                    completed_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut runs_stmt = conn.prepare(
            "SELECT cr.id, cr.task_item_id, cr.phase, cr.command, cr.cwd, cr.workspace_id, cr.agent_id, cr.exit_code, cr.stdout_path, cr.stderr_path, cr.started_at, cr.ended_at, cr.interrupted
             FROM command_runs cr
             JOIN task_items ti ON ti.id = cr.task_item_id
             WHERE ti.task_id = ?1
             ORDER BY cr.started_at DESC
             LIMIT 120",
        )?;
        let runs = runs_stmt
            .query_map(params![task_id], |row| {
                Ok(CommandRunDto {
                    id: row.get(0)?,
                    task_item_id: row.get(1)?,
                    phase: row.get(2)?,
                    command: row.get(3)?,
                    cwd: row.get(4)?,
                    workspace_id: row.get(5)?,
                    agent_id: row.get(6)?,
                    exit_code: row.get(7)?,
                    stdout_path: row.get(8)?,
                    stderr_path: row.get(9)?,
                    started_at: row.get(10)?,
                    ended_at: row.get(11)?,
                    interrupted: row.get::<_, i64>(12)? == 1,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut events_stmt = conn.prepare(
            "SELECT id, task_id, task_item_id, event_type, payload_json, created_at FROM events WHERE task_id = ?1 ORDER BY id DESC LIMIT 200",
        )?;
        let events = events_stmt
            .query_map(params![task_id], |row| {
                let payload_raw: String = row.get(4)?;
                let payload = serde_json::from_str(&payload_raw)
                    .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                Ok(EventDto {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    task_item_id: row.get(2)?,
                    event_type: row.get(3)?,
                    payload,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok((items, runs, events))
    }

    fn load_task_item_counts(&self, task_id: &str) -> Result<(i64, i64, i64)> {
        let conn = open_conn(&self.db_path)?;
        conn.query_row(
            "SELECT COUNT(*), SUM(CASE WHEN status IN ('qa_passed','fixed','verified','skipped','unresolved') THEN 1 ELSE 0 END), SUM(CASE WHEN status IN ('qa_failed','unresolved') THEN 1 ELSE 0 END) FROM task_items WHERE task_id = ?1",
            params![task_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                ))
            },
        )
        .with_context(|| format!("load task item counts for task_id={task_id}"))
    }

    fn list_task_ids_ordered_by_created_desc(&self) -> Result<Vec<String>> {
        let conn = open_conn(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT id FROM tasks ORDER BY created_at DESC")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    fn find_latest_resumable_task_id(&self, include_pending: bool) -> Result<Option<String>> {
        let conn = open_conn(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT id, status FROM tasks ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (id, status) = row?;
            let resumable = matches!(status.as_str(), "running" | "interrupted" | "paused")
                || (include_pending && status == "pending");
            if resumable {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    fn load_task_runtime_row(&self, task_id: &str) -> Result<TaskRuntimeRow> {
        let conn = open_conn(&self.db_path)?;
        let row = conn.query_row(
            "SELECT workspace_id, workflow_id, workspace_root, ticket_dir, execution_plan_json, current_cycle, init_done FROM tasks WHERE id = ?1",
            params![task_id],
            |row| {
                Ok(TaskRuntimeRow {
                    workspace_id: row.get(0)?,
                    workflow_id: row.get(1)?,
                    workspace_root_raw: row.get(2)?,
                    ticket_dir: row.get(3)?,
                    execution_plan_json: row.get(4)?,
                    current_cycle: row.get(5)?,
                    init_done: row.get(6)?,
                })
            },
        )?;
        Ok(row)
    }

    fn first_task_item_id(&self, task_id: &str) -> Result<Option<String>> {
        let conn = open_conn(&self.db_path)?;
        conn.query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()
        .context("query first task item")
    }

    fn count_unresolved_items(&self, task_id: &str) -> Result<i64> {
        let conn = open_conn(&self.db_path)?;
        conn.query_row(
            "SELECT COUNT(*) FROM task_items WHERE task_id = ?1 AND status IN ('unresolved','qa_failed')",
            params![task_id],
            |row| row.get(0),
        )
        .context("count unresolved items")
    }

    fn list_task_items_for_cycle(&self, task_id: &str) -> Result<Vec<TaskItemRow>> {
        let conn = open_conn(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, qa_file_path
             FROM task_items
             WHERE task_id = ?1
             ORDER BY order_no",
        )?;
        let rows = stmt
            .query_map(params![task_id], |row| {
                Ok(TaskItemRow {
                    id: row.get(0)?,
                    qa_file_path: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn set_task_status(&self, task_id: &str, status: &str, set_completed: bool) -> Result<()> {
        let conn = open_conn(&self.db_path)?;
        let now = now_ts();
        if set_completed {
            conn.execute(
                "UPDATE tasks SET status = ?2, completed_at = ?3, updated_at = ?4 WHERE id = ?1",
                params![task_id, status, now.clone(), now],
            )?;
        } else if matches!(status, "pending" | "running" | "paused" | "interrupted") {
            conn.execute(
                "UPDATE tasks SET status = ?2, completed_at = NULL, updated_at = ?3 WHERE id = ?1",
                params![task_id, status, now],
            )?;
        } else {
            conn.execute(
                "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![task_id, status, now],
            )?;
        }
        Ok(())
    }

    fn load_task_status(&self, task_id: &str) -> Result<Option<String>> {
        let conn = open_conn(&self.db_path)?;
        conn.query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .with_context(|| format!("load task status for task_id={task_id}"))
    }

    fn prepare_task_for_start_batch(&self, task_id: &str) -> Result<()> {
        let conn = open_conn(&self.db_path)?;
        let tx = conn.unchecked_transaction()?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?;

        if status.is_none() {
            anyhow::bail!("task not found: {}", task_id);
        }

        if matches!(status.as_deref(), Some("failed")) {
            tx.execute(
                "UPDATE task_items SET status='pending', ticket_files_json='[]', ticket_content_json='[]', fix_required=0, fixed=0, last_error='', completed_at=NULL, updated_at=?2 WHERE task_id=?1 AND status='unresolved'",
                params![task_id, now_ts()],
            )?;
        }

        tx.execute(
            "UPDATE tasks SET status = 'running', completed_at = NULL, updated_at = ?2 WHERE id = ?1",
            params![task_id, now_ts()],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()> {
        let conn = open_conn(&self.db_path)?;
        conn.execute(
            "UPDATE tasks SET current_cycle = ?2, init_done = ?3, updated_at = ?4 WHERE id = ?1",
            params![
                task_id,
                current_cycle as i64,
                if init_done { 1 } else { 0 },
                now_ts()
            ],
        )?;
        Ok(())
    }

    fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()> {
        let conn = open_conn(&self.db_path)?;
        conn.execute(
            "UPDATE task_items SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![task_item_id, status, now_ts()],
        )?;
        Ok(())
    }

    fn load_task_name(&self, task_id: &str) -> Result<Option<String>> {
        let conn = open_conn(&self.db_path)?;
        conn.query_row(
            "SELECT name FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .with_context(|| format!("load task name for task_id={task_id}"))
    }

    fn list_task_log_runs(&self, task_id: &str, limit: usize) -> Result<Vec<TaskLogRunRow>> {
        let conn = open_conn(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT cr.id, cr.phase, cr.stdout_path, cr.stderr_path, cr.started_at
             FROM command_runs cr
             JOIN task_items ti ON ti.id = cr.task_item_id
             WHERE ti.task_id = ?1
             ORDER BY cr.started_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![task_id, limit as i64], |row| {
                Ok(TaskLogRunRow {
                    run_id: row.get(0)?,
                    phase: row.get(1)?,
                    stdout_path: row.get(2)?,
                    stderr_path: row.get(3)?,
                    started_at: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>> {
        let conn = open_conn(&self.db_path)?;
        let exists = conn
            .query_row(
                "SELECT 1 FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if exists.is_none() {
            anyhow::bail!("task not found: {}", task_id);
        }

        let mut log_paths = HashSet::new();
        let mut runs_stmt = conn.prepare(
            "SELECT cr.stdout_path, cr.stderr_path
             FROM command_runs cr
             JOIN task_items ti ON ti.id = cr.task_item_id
             WHERE ti.task_id = ?1",
        )?;
        for row in runs_stmt.query_map(params![task_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (stdout_path, stderr_path) = row?;
            if !stdout_path.trim().is_empty() {
                log_paths.insert(stdout_path);
            }
            if !stderr_path.trim().is_empty() {
                log_paths.insert(stderr_path);
            }
        }

        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM events WHERE task_id = ?1", params![task_id])?;
        tx.execute(
            "DELETE FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)",
            params![task_id],
        )?;
        tx.execute(
            "DELETE FROM task_items WHERE task_id = ?1",
            params![task_id],
        )?;
        tx.execute("DELETE FROM tasks WHERE id = ?1", params![task_id])?;
        tx.commit()?;
        Ok(log_paths.into_iter().collect())
    }

    fn insert_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let conn = open_conn(&self.db_path)?;
        conn.execute(
            "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                run.id,
                run.task_item_id,
                run.phase,
                run.command,
                run.cwd,
                run.workspace_id,
                run.agent_id,
                run.exit_code,
                run.stdout_path,
                run.stderr_path,
                run.output_json,
                run.artifacts_json,
                run.confidence,
                run.quality_score,
                run.validation_status,
                run.started_at,
                run.ended_at,
                run.interrupted
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::now_ts;
    use crate::db::open_conn;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;
    use rusqlite::params;

    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/repo_test.md");
        std::fs::write(&qa_file, "# repository test\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("repo-test".to_string()),
                goal: Some("repo-test-goal".to_string()),
                ..Default::default()
            },
        )
        .expect("task should be created");
        (state, created.id)
    }

    #[test]
    fn resolve_task_id_supports_exact_and_prefix() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = SqliteTaskRepository::new(state.db_path.clone());

        let exact = repo
            .resolve_task_id(&task_id)
            .expect("exact id must resolve");
        assert_eq!(exact, task_id);

        let prefix = &task_id[0..8];
        let by_prefix = repo
            .resolve_task_id(prefix)
            .expect("single prefix match must resolve");
        assert_eq!(by_prefix, task_id);
    }

    #[test]
    fn load_task_summary_and_counts_are_consistent() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = SqliteTaskRepository::new(state.db_path.clone());

        let summary = repo
            .load_task_summary(&task_id)
            .expect("summary should load from repo");
        assert_eq!(summary.id, task_id);
        assert!(!summary.created_at.is_empty());
        assert!(!summary.updated_at.is_empty());

        let (total, finished, failed) = repo
            .load_task_item_counts(&summary.id)
            .expect("item counts should load");
        assert!(total >= 1);
        assert_eq!(finished, 0);
        assert_eq!(failed, 0);
    }

    #[test]
    fn prepare_task_for_start_batch_resets_unresolved_items() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET status='failed' WHERE id = ?1",
            params![task_id.clone()],
        )
        .expect("mark task failed");
        conn.execute(
            "UPDATE task_items SET status='unresolved', fix_required=1, fixed=1, last_error='x' WHERE task_id = ?1",
            params![task_id.clone()],
        )
        .expect("mark unresolved");

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.prepare_task_for_start_batch(&task_id)
            .expect("prepare should succeed");

        let task_status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![task_id.clone()],
                |row| row.get(0),
            )
            .expect("task status query");
        assert_eq!(task_status, "running");

        let reset_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_items WHERE task_id=?1 AND status='pending' AND fix_required=0 AND fixed=0",
                params![task_id],
                |row| row.get(0),
            )
            .expect("task_items query");
        assert!(reset_count >= 1);
    }

    #[test]
    fn insert_and_list_task_log_runs_work() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let item_id: String = conn
            .query_row(
                "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
                params![task_id.clone()],
                |row| row.get(0),
            )
            .expect("task item exists");

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        let run = NewCommandRun {
            id: "run-test-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo test".to_string(),
            cwd: state.app_root.to_string_lossy().to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: "/tmp/stdout.log".to_string(),
            stderr_path: "/tmp/stderr.log".to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
        };
        repo.insert_command_run(&run).expect("insert command run");

        let runs = repo
            .list_task_log_runs(&task_id, 10)
            .expect("list task log runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, "run-test-1");
        assert_eq!(runs[0].phase, "qa");
    }

    #[test]
    fn delete_task_and_collect_log_paths_cleans_data() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let item_id: String = conn
            .query_row(
                "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
                params![task_id.clone()],
                |row| row.get(0),
            )
            .expect("task item exists");
        let stdout_path = state.logs_dir.join("repo_test.stdout");
        let stderr_path = state.logs_dir.join("repo_test.stderr");
        std::fs::write(&stdout_path, "stdout").expect("seed stdout log");
        std::fs::write(&stderr_path, "stderr").expect("seed stderr log");

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        let run = NewCommandRun {
            id: "run-test-delete".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo test".to_string(),
            cwd: state.app_root.to_string_lossy().to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
        };
        repo.insert_command_run(&run).expect("insert command run");

        let paths = repo
            .delete_task_and_collect_log_paths(&task_id)
            .expect("delete task");
        assert_eq!(paths.len(), 2);

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("count tasks");
        assert_eq!(remaining, 0);
    }
}
