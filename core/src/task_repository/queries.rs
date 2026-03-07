use crate::dto::{CommandRunDto, EventDto, TaskItemDto, TaskItemRow, TaskSummary};
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

use super::types::{TaskLogRunRow, TaskRuntimeRow};
use rusqlite::Connection;

pub fn resolve_task_id(conn: &Connection, task_id_or_prefix: &str) -> Result<String> {
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
        1 => matches
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("single task match disappeared unexpectedly")),
        0 => anyhow::bail!("task not found: {}", task_id_or_prefix),
        _ => anyhow::bail!(
            "multiple tasks match prefix '{}': {:?}",
            task_id_or_prefix,
            matches
        ),
    }
}

pub fn load_task_summary(conn: &Connection, task_id: &str) -> Result<TaskSummary> {
    let mut stmt = conn.prepare(
        "SELECT id, name, status, started_at, completed_at, goal, target_files_json, project_id, workspace_id, workflow_id, created_at, updated_at, parent_task_id, spawn_reason, spawn_depth FROM tasks WHERE id = ?1",
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
            parent_task_id: row.get("parent_task_id")?,
            spawn_reason: row.get("spawn_reason")?,
            spawn_depth: row.get::<_, Option<i64>>("spawn_depth")?.unwrap_or(0),
        })
    })
    .with_context(|| format!("load task summary for task_id={task_id}"))
}

pub fn load_task_detail_rows(
    conn: &Connection,
    task_id: &str,
) -> Result<(Vec<TaskItemDto>, Vec<CommandRunDto>, Vec<EventDto>)> {
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

pub fn load_task_item_counts(conn: &Connection, task_id: &str) -> Result<(i64, i64, i64)> {
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

pub fn list_task_ids_ordered_by_created_desc(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM tasks ORDER BY created_at DESC")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

pub fn find_latest_resumable_task_id(
    conn: &Connection,
    include_pending: bool,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT id, status FROM tasks ORDER BY updated_at DESC")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    for row in rows {
        let (id, status) = row?;
        let resumable = matches!(
            status.as_str(),
            "running" | "interrupted" | "paused" | "restart_pending"
        ) || (include_pending && status == "pending");
        if resumable {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub fn load_task_runtime_row(conn: &Connection, task_id: &str) -> Result<TaskRuntimeRow> {
    let row = conn.query_row(
        "SELECT workspace_id, workflow_id, workspace_root, ticket_dir, execution_plan_json, current_cycle, init_done, COALESCE(goal,''), COALESCE(project_id,''), pipeline_vars_json, COALESCE(spawn_depth,0) FROM tasks WHERE id = ?1",
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
                goal: row.get(7)?,
                project_id: row.get(8)?,
                pipeline_vars_json: row.get(9)?,
                spawn_depth: row.get(10)?,
            })
        },
    )?;
    Ok(row)
}

pub fn first_task_item_id(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
        params![task_id],
        |row| row.get(0),
    )
    .optional()
    .context("query first task item")
}

pub fn count_unresolved_items(conn: &Connection, task_id: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM task_items WHERE task_id = ?1 AND status IN ('unresolved','qa_failed')",
        params![task_id],
        |row| row.get(0),
    )
    .context("count unresolved items")
}

pub fn list_task_items_for_cycle(conn: &Connection, task_id: &str) -> Result<Vec<TaskItemRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, qa_file_path, dynamic_vars_json, label, source
         FROM task_items
         WHERE task_id = ?1
         ORDER BY order_no",
    )?;
    let rows = stmt
        .query_map(params![task_id], |row| {
            Ok(TaskItemRow {
                id: row.get(0)?,
                qa_file_path: row.get(1)?,
                dynamic_vars_json: row.get(2)?,
                label: row.get(3)?,
                source: row
                    .get::<_, Option<String>>(4)?
                    .unwrap_or_else(|| "static".to_string()),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn load_task_status(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT status FROM tasks WHERE id = ?1",
        params![task_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .with_context(|| format!("load task status for task_id={task_id}"))
}

pub fn load_task_name(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT name FROM tasks WHERE id = ?1",
        params![task_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .with_context(|| format!("load task name for task_id={task_id}"))
}

pub fn list_task_log_runs(
    conn: &Connection,
    task_id: &str,
    limit: usize,
) -> Result<Vec<TaskLogRunRow>> {
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
