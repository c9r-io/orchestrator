use crate::config_load::now_ts;
use anyhow::Result;
use rusqlite::{params, Connection};

use super::command_run::NewCommandRun;
use super::types::DbEventRecord;

/// In-flight command run record: (run_id, item_id, phase, pid).
pub type InflightRunRecord = (String, String, String, Option<i64>);

pub(crate) fn extract_event_promoted_fields(
    payload_json: &str,
) -> (Option<String>, Option<String>, Option<i64>) {
    let value: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(value) => value,
        Err(_) => return (None, None, None),
    };
    let step = value["step"]
        .as_str()
        .or_else(|| value["phase"].as_str())
        .map(str::to_owned);
    let step_scope = value["step_scope"].as_str().map(str::to_owned);
    let cycle = value["cycle"].as_i64();
    (step, step_scope, cycle)
}

pub fn insert_event(conn: &Connection, event: &DbEventRecord) -> Result<()> {
    let (step, step_scope, cycle) = extract_event_promoted_fields(&event.payload_json);
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at, step, step_scope, cycle)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event.task_id,
            event.task_item_id,
            event.event_type,
            event.payload_json,
            now_ts(),
            step,
            step_scope,
            cycle
        ],
    )?;
    Ok(())
}

pub fn update_command_run(conn: &Connection, run: &NewCommandRun) -> Result<()> {
    conn.execute(
        "UPDATE command_runs
         SET exit_code = ?2,
             ended_at = ?3,
             interrupted = ?4,
             output_json = ?5,
             artifacts_json = ?6,
             confidence = ?7,
             quality_score = ?8,
             validation_status = ?9,
             session_id = ?10,
             machine_output_source = ?11,
             output_json_path = ?12
         WHERE id = ?1",
        params![
            run.id,
            run.exit_code,
            run.ended_at,
            run.interrupted,
            run.output_json,
            run.artifacts_json,
            run.confidence,
            run.quality_score,
            run.validation_status,
            run.session_id,
            run.machine_output_source,
            run.output_json_path
        ],
    )?;
    Ok(())
}

pub fn update_command_run_with_events(
    conn: &Connection,
    run: &NewCommandRun,
    events: &[DbEventRecord],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    update_command_run(&tx, run)?;
    for event in events {
        insert_event(&tx, event)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn persist_phase_result_with_events(
    conn: &Connection,
    run: &NewCommandRun,
    events: &[DbEventRecord],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO command_runs (
             id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code,
             stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score,
             validation_status, started_at, ended_at, interrupted, session_id,
             machine_output_source, output_json_path
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
             ?9, ?10, ?11, ?12, ?13, ?14,
             ?15, ?16, ?17, ?18, ?19,
             ?20, ?21
         )",
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
            run.interrupted,
            run.session_id,
            run.machine_output_source,
            run.output_json_path
        ],
    )?;
    for event in events {
        insert_event(&tx, event)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn update_command_run_pid(conn: &Connection, run_id: &str, pid: i64) -> Result<()> {
    conn.execute(
        "UPDATE command_runs SET pid = ?2 WHERE id = ?1",
        params![run_id, pid],
    )?;
    Ok(())
}

pub fn find_active_child_pids(conn: &Connection, task_id: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT cr.pid FROM command_runs cr
         JOIN task_items ti ON cr.task_item_id = ti.id
         WHERE ti.task_id = ?1 AND cr.exit_code = -1 AND cr.pid IS NOT NULL",
    )?;
    let pids = stmt
        .query_map(params![task_id], |row| row.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(pids)
}

pub fn update_task_pipeline_vars(
    conn: &Connection,
    task_id: &str,
    pipeline_vars_json: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET pipeline_vars_json = ?2, updated_at = ?3 WHERE id = ?1",
        params![task_id, pipeline_vars_json, now_ts()],
    )?;
    Ok(())
}

/// Record of a completed command run for a pending item (used by FR-038 compensation).
#[derive(Debug, Clone)]
pub struct CompletedRunRecord {
    /// The task item this run belongs to.
    pub task_item_id: String,
    /// The workflow phase (e.g., `qa_testing`).
    pub phase: String,
    /// Process exit code.
    pub exit_code: i64,
    /// Optional confidence score from the agent.
    pub confidence: Option<f64>,
    /// Optional quality score from the agent.
    pub quality_score: Option<f64>,
}

/// Returns in-flight command runs (exit_code = -1, not yet ended) for a task.
pub fn find_inflight_command_runs_for_task(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<InflightRunRecord>> {
    let mut stmt = conn.prepare(
        "SELECT cr.id, cr.task_item_id, cr.phase, cr.pid
         FROM command_runs cr
         JOIN task_items ti ON cr.task_item_id = ti.id
         WHERE ti.task_id = ?1 AND cr.exit_code = -1 AND (cr.ended_at IS NULL OR cr.ended_at = '')",
    )?;
    let rows = stmt
        .query_map(params![task_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Returns completed command runs whose parent items are still `pending`.
/// Used by FR-038 post-recovery finalize compensation.
pub fn find_completed_runs_for_pending_items(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<CompletedRunRecord>> {
    let mut stmt = conn.prepare(
        "SELECT cr.task_item_id, cr.phase, cr.exit_code, cr.confidence, cr.quality_score
         FROM task_items ti
         JOIN command_runs cr ON cr.task_item_id = ti.id
         WHERE ti.task_id = ?1 AND ti.status = 'pending'
           AND cr.ended_at IS NOT NULL AND cr.ended_at != '' AND cr.exit_code != -1
         ORDER BY ti.id, cr.started_at",
    )?;
    let rows = stmt
        .query_map(params![task_id], |row| {
            Ok(CompletedRunRecord {
                task_item_id: row.get(0)?,
                phase: row.get(1)?,
                exit_code: row.get(2)?,
                confidence: row.get(3)?,
                quality_score: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// FR-052: Counts recent heartbeat events for the given item IDs since a cutoff timestamp.
pub fn count_recent_heartbeats_for_items(
    conn: &Connection,
    task_id: &str,
    item_ids: &[String],
    cutoff_ts: &str,
) -> Result<i64> {
    if item_ids.is_empty() {
        return Ok(0);
    }
    // Build dynamic IN clause — rusqlite doesn't support array binding.
    let placeholders: Vec<String> = (0..item_ids.len()).map(|i| format!("?{}", i + 3)).collect();
    let sql = format!(
        "SELECT COUNT(*) FROM events
         WHERE task_id = ?1 AND event_type = 'step_heartbeat'
           AND task_item_id IN ({})
           AND created_at >= ?2",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(task_id.to_owned()));
    param_values.push(Box::new(cutoff_ts.to_owned()));
    for id in item_ids {
        param_values.push(Box::new(id.clone()));
    }
    let refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();
    let count: i64 = stmt.query_row(refs.as_slice(), |row| row.get(0))?;
    Ok(count)
}

pub fn update_task_item_tickets(
    conn: &Connection,
    task_item_id: &str,
    ticket_files_json: &str,
    ticket_content_json: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE task_items
         SET ticket_files_json = ?2,
             ticket_content_json = ?3,
             updated_at = ?4
         WHERE id = ?1",
        params![
            task_item_id,
            ticket_files_json,
            ticket_content_json,
            now_ts()
        ],
    )?;
    Ok(())
}
