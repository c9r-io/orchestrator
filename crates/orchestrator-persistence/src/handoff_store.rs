//! The handoff and resume tables: `handoff_snapshots`, `resume_plans` and
//! `resume_executions`, plus the reads that feed a task's state version.
//!
//! `core::handoff` owns the projection — what a briefing contains, how a
//! workspace is digested, how a state version is hashed, which resume modes a
//! boundary supports, and what an expired or stale plan means. This module owns
//! the statements.
//!
//! **The reason this file exists is not the reference count.** In core, every
//! one of these operations ran inside a `writer().call` closure, and two of them
//! called a state-version helper that shells out to `git` three times and reads
//! every untracked file in the workspace. `reserve_execution` did it *inside its
//! transaction*. The SQLite write lock was held for the duration of an external
//! process tree. Splitting here is what takes it out (FR-130 B14).
//!
//! JSON columns cross the boundary as text. The briefing, the consequence
//! preview and the boundary snapshot are `core` types; parsing them here would
//! mean reporting a `serde_json` failure as a column-conversion failure against
//! an invented column index, which is the shape B10 and B11 removed elsewhere.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::async_database::{AsyncDatabase, flatten_err};

/// The task columns a state version is computed from, plus the workspace root
/// the caller must digest separately.
#[derive(Debug, Clone)]
pub struct StateVersionInputs {
    /// Task status.
    pub status: String,
    /// Current workflow cycle.
    pub current_cycle: i64,
    /// Whether initialization has completed.
    pub init_done: i64,
    /// Pipeline variables as stored, unparsed.
    pub pipeline_vars_json: Option<String>,
    /// Execution plan as stored, unparsed.
    pub execution_plan_json: Option<String>,
    /// Last update timestamp.
    pub updated_at: String,
    /// Highest event id recorded for the task.
    pub event_cursor: i64,
    /// Workspace root, for the caller's filesystem digest.
    pub workspace_root: String,
}

/// The task header a snapshot records, read once with the cursor bound.
#[derive(Debug, Clone)]
pub struct SnapshotInputs {
    /// Owning project.
    pub project_id: String,
    /// Task goal text.
    pub goal: String,
    /// Task status.
    pub status: String,
    /// Current workflow cycle.
    pub current_cycle: i64,
    /// Highest event id available to snapshot.
    pub max_cursor: i64,
    /// Inputs for the state version, read in the same snapshot.
    pub state_version: StateVersionInputs,
}

/// One event as a briefing projection sees it.
#[derive(Debug, Clone)]
pub struct BriefingEvent {
    /// Event type name.
    pub event_type: String,
    /// Raw JSON payload, unparsed.
    pub payload_json: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// A handoff snapshot to be recorded, with its briefing already projected.
#[derive(Debug, Clone)]
pub struct NewSnapshot {
    /// Snapshot identifier.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Owning task.
    pub task_id: String,
    /// Event cursor the snapshot was taken at.
    pub source_event_cursor: i64,
    /// Projection version that produced the briefing.
    pub projection_version: i64,
    /// The briefing, already serialized.
    pub briefing_json: String,
    /// Hash over the projection inputs.
    pub content_hash: String,
    /// Task state version at projection time.
    pub state_version: String,
    /// Who asked for the snapshot.
    pub generated_by: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// One `handoff_snapshots` row, briefing unparsed.
#[derive(Debug, Clone)]
pub struct SnapshotRow {
    /// Snapshot identifier.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Owning task.
    pub task_id: String,
    /// Event cursor the snapshot was taken at.
    pub source_event_cursor: i64,
    /// Projection version that produced the briefing.
    pub projection_version: i64,
    /// The briefing as stored, for the caller to parse.
    pub briefing_json: String,
    /// Hash over the projection inputs.
    pub content_hash: String,
    /// Task state version at projection time.
    pub state_version: String,
    /// Who asked for the snapshot.
    pub generated_by: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// Everything a boundary listing reads, in one consistent snapshot.
#[derive(Debug, Clone)]
pub struct BoundaryInputs {
    /// Owning project.
    pub project_id: String,
    /// Current workflow cycle.
    pub current_cycle: i64,
    /// Execution plan as stored, unparsed.
    pub execution_plan_json: String,
    /// Inputs for the state version, read in the same snapshot.
    pub state_version: StateVersionInputs,
    /// The lowest-ordered failed item, when there is one.
    pub failed_item_id: Option<String>,
    /// The most recent command run and its provider session, when there is one.
    pub latest_run: Option<(String, Option<String>)>,
}

/// A resume plan to be recorded, with its consequence preview already built.
#[derive(Debug, Clone)]
pub struct NewResumePlan {
    /// Plan identifier.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Owning task.
    pub task_id: String,
    /// Attention item the plan answers, when there is one.
    pub attention_item_id: Option<String>,
    /// Boundary the plan resumes from.
    pub boundary_id: String,
    /// Resume mode, already rendered.
    pub mode: String,
    /// State version the plan was built against.
    pub expected_state_version: String,
    /// Side-effect class of the boundary, already labelled.
    pub side_effect_class: String,
    /// Whether replaying the boundary is safe.
    pub replay_safe: bool,
    /// Whether an elevated confirmation is required to execute.
    pub elevated_confirmation_required: bool,
    /// Consequence preview, already serialized.
    pub consequence_json: String,
    /// Boundary snapshot, already serialized.
    pub execution_input_json: String,
    /// Command run the plan would resume, when there is one.
    pub provider_command_run_id: Option<String>,
    /// Expiry timestamp.
    pub expires_at: String,
    /// Who created the plan.
    pub created_by: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// One `resume_plans` row, JSON columns unparsed.
#[derive(Debug, Clone)]
pub struct ResumePlanRow {
    /// Owning task.
    pub task_id: String,
    /// Resume mode, as stored.
    pub mode: String,
    /// State version the plan was built against.
    pub expected_state_version: String,
    /// Consequence preview as stored.
    pub consequence_json: String,
    /// Boundary snapshot as stored.
    pub execution_input_json: String,
    /// Whether an elevated confirmation is required.
    pub elevated_confirmation_required: i64,
    /// Expiry timestamp.
    pub expires_at: String,
    /// Lifecycle status.
    pub status: String,
}

/// A reservation to be taken against a plan, already authorized by the caller.
#[derive(Debug, Clone)]
pub struct NewExecution {
    /// Execution identifier.
    pub id: String,
    /// Plan being executed.
    pub plan_id: String,
    /// Operator taking the reservation.
    pub actor: String,
    /// Operator-supplied reason.
    pub operator_reason: String,
    /// Retry identity.
    pub idempotency_key: String,
    /// Hash over the authorized request.
    pub request_hash: String,
    /// The state version the caller verified before calling. Re-checked inside
    /// the transaction as the fence that replaces the caller's own read.
    pub verified_state_version: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// What [`reserve_execution`] found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reservation {
    /// This retry identity already holds a reservation.
    Existing {
        /// Execution identifier of the prior reservation.
        id: String,
        /// Its current status.
        status: String,
    },
    /// The reservation was taken; the caller owns the side effect.
    Reserved {
        /// Execution identifier just created.
        id: String,
    },
    /// The plan left `planned`, or its state version moved, between the
    /// caller's checks and this transaction.
    PlanMoved,
}

/// Reads the columns a task's state version is computed from.
pub async fn state_version_inputs(
    db: &AsyncDatabase,
    task_id: String,
) -> Result<Option<StateVersionInputs>> {
    db.reader()
        .call(move |conn| {
            state_version_inputs_blocking(conn, &task_id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads a task's header and its highest event id, for a snapshot projection.
///
/// One reader closure, so the header, the cursor ceiling and the state-version
/// columns describe the same instant. The workspace digest deliberately does not
/// happen here — see the module note.
pub async fn snapshot_inputs(
    db: &AsyncDatabase,
    task_id: String,
) -> Result<Option<SnapshotInputs>> {
    db.reader()
        .call(move |conn| {
            snapshot_inputs_blocking(conn, &task_id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads a task's events up to and including a cursor, oldest first.
pub async fn events_up_to(
    db: &AsyncDatabase,
    task_id: String,
    cursor: i64,
) -> Result<Vec<BriefingEvent>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT event_type, payload_json, created_at FROM events
                 WHERE task_id=?1 AND id<=?2 ORDER BY id ASC",
            )?;
            let rows = stmt
                .query_map(params![task_id, cursor], |row| {
                    Ok(BriefingEvent {
                        event_type: row.get(0)?,
                        payload_json: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(flatten_err)
}

/// Returns the snapshot already recorded for this task, cursor and content
/// hash, or records this one.
///
/// Find and insert are one transaction: the identity of a snapshot is
/// `(task_id, cursor, content_hash)`, and two callers projecting the same task
/// at the same cursor must converge on one row rather than write two.
pub async fn find_or_insert_snapshot(
    db: &AsyncDatabase,
    snapshot: NewSnapshot,
) -> Result<SnapshotRow> {
    db.writer()
        .call(move |conn| {
            find_or_insert_snapshot_blocking(conn, &snapshot)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads one snapshot by identifier.
pub async fn read_snapshot(db: &AsyncDatabase, id: String) -> Result<Option<SnapshotRow>> {
    db.reader()
        .call(move |conn| {
            read_snapshot_blocking(conn, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads everything a boundary listing needs, in one consistent snapshot.
pub async fn boundary_inputs(
    db: &AsyncDatabase,
    task_id: String,
) -> Result<Option<BoundaryInputs>> {
    db.reader()
        .call(move |conn| {
            boundary_inputs_blocking(conn, &task_id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Records a resume plan.
pub async fn insert_plan(db: &AsyncDatabase, plan: NewResumePlan) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "INSERT INTO resume_plans
                 (id, project_id, task_id, attention_item_id, boundary_id, mode,
                  expected_state_version, side_effect_class, replay_safe,
                  elevated_confirmation_required, consequence_json, execution_input_json,
                  provider_command_run_id, status, expires_at, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                         'planned', ?14, ?15, ?16)",
                params![
                    plan.id,
                    plan.project_id,
                    plan.task_id,
                    plan.attention_item_id,
                    plan.boundary_id,
                    plan.mode,
                    plan.expected_state_version,
                    plan.side_effect_class,
                    i64::from(plan.replay_safe),
                    i64::from(plan.elevated_confirmation_required),
                    plan.consequence_json,
                    plan.execution_input_json,
                    plan.provider_command_run_id,
                    plan.expires_at,
                    plan.created_by,
                    plan.created_at,
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Reads one resume plan by identifier.
pub async fn read_plan(db: &AsyncDatabase, id: String) -> Result<Option<ResumePlanRow>> {
    db.reader()
        .call(move |conn| {
            read_plan_blocking(conn, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Takes a reservation against a plan, or reports why it could not.
///
/// One transaction. The plan is flipped to `executing` behind a fence on both
/// `status='planned'` **and** `expected_state_version`, and the reservation row
/// is only written when that fence held. The version condition is here because
/// the caller now computes the current state version *before* this call — it has
/// to, since computing it runs `git` — and this is what makes that safe: if the
/// plan moved in between, nothing is written and the caller is told.
///
/// The statement it replaces did not check its own result. It read the status,
/// inserted the execution row, and then ran
/// `UPDATE … WHERE status='planned'` without looking at how many rows changed,
/// so two callers racing could both come away believing they owned the
/// execution.
pub async fn reserve_execution(db: &AsyncDatabase, execution: NewExecution) -> Result<Reservation> {
    db.writer()
        .call(move |conn| {
            reserve_execution_blocking(conn, &execution)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Closes out an owned execution reservation and its plan, in one transaction.
///
/// Reports `false` when no `executing` reservation matched — a completion for
/// something that was never reserved, or was already closed.
pub async fn complete_execution(
    db: &AsyncDatabase,
    execution_id: String,
    status: String,
    child_task_id: Option<String>,
    error_code: Option<String>,
    now: String,
) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            complete_execution_blocking(
                conn,
                &execution_id,
                &status,
                child_task_id.as_deref(),
                error_code.as_deref(),
                &now,
            )
            .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

fn state_version_inputs_blocking(
    conn: &Connection,
    task_id: &str,
) -> Result<Option<StateVersionInputs>> {
    conn.query_row(
        "SELECT status, current_cycle, init_done, pipeline_vars_json, execution_plan_json,
                updated_at, (SELECT COALESCE(MAX(id), 0) FROM events WHERE task_id=tasks.id),
                workspace_root
         FROM tasks WHERE id=?1",
        [task_id],
        |row| {
            Ok(StateVersionInputs {
                status: row.get(0)?,
                current_cycle: row.get(1)?,
                init_done: row.get(2)?,
                pipeline_vars_json: row.get(3)?,
                execution_plan_json: row.get(4)?,
                updated_at: row.get(5)?,
                event_cursor: row.get(6)?,
                workspace_root: row.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn snapshot_inputs_blocking(conn: &Connection, task_id: &str) -> Result<Option<SnapshotInputs>> {
    let header = conn
        .query_row(
            "SELECT project_id, goal, status, current_cycle FROM tasks WHERE id=?1",
            [task_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((project_id, goal, status, current_cycle)) = header else {
        return Ok(None);
    };
    let max_cursor: i64 = conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM events WHERE task_id=?1",
        [task_id],
        |row| row.get(0),
    )?;
    let state_version =
        state_version_inputs_blocking(conn, task_id)?.context("task disappeared between reads")?;
    Ok(Some(SnapshotInputs {
        project_id,
        goal,
        status,
        current_cycle,
        max_cursor,
        state_version,
    }))
}

fn find_or_insert_snapshot_blocking(
    conn: &Connection,
    snapshot: &NewSnapshot,
) -> Result<SnapshotRow> {
    let tx = conn.unchecked_transaction()?;
    if let Some(existing) = tx
        .query_row(
            "SELECT id FROM handoff_snapshots
             WHERE task_id=?1 AND source_event_cursor=?2 AND content_hash=?3",
            params![
                snapshot.task_id,
                snapshot.source_event_cursor,
                snapshot.content_hash
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        tx.commit()?;
        return read_snapshot_blocking(conn, &existing)?.context("handoff snapshot disappeared");
    }
    tx.execute(
        "INSERT INTO handoff_snapshots
         (id, project_id, task_id, source_event_cursor, projection_version, briefing_json,
          content_hash, state_version, generated_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            snapshot.id,
            snapshot.project_id,
            snapshot.task_id,
            snapshot.source_event_cursor,
            snapshot.projection_version,
            snapshot.briefing_json,
            snapshot.content_hash,
            snapshot.state_version,
            snapshot.generated_by,
            snapshot.created_at,
        ],
    )?;
    tx.commit()?;
    read_snapshot_blocking(conn, &snapshot.id)?.context("failed to persist handoff snapshot")
}

fn read_snapshot_blocking(conn: &Connection, id: &str) -> Result<Option<SnapshotRow>> {
    conn.query_row(
        "SELECT id, project_id, task_id, source_event_cursor, projection_version, briefing_json,
                content_hash, state_version, generated_by, created_at
         FROM handoff_snapshots WHERE id=?1",
        [id],
        |row| {
            Ok(SnapshotRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_id: row.get(2)?,
                source_event_cursor: row.get(3)?,
                projection_version: row.get(4)?,
                briefing_json: row.get(5)?,
                content_hash: row.get(6)?,
                state_version: row.get(7)?,
                generated_by: row.get(8)?,
                created_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn boundary_inputs_blocking(conn: &Connection, task_id: &str) -> Result<Option<BoundaryInputs>> {
    let header = conn
        .query_row(
            "SELECT project_id, current_cycle, execution_plan_json FROM tasks WHERE id=?1",
            [task_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((project_id, current_cycle, execution_plan_json)) = header else {
        return Ok(None);
    };
    let state_version =
        state_version_inputs_blocking(conn, task_id)?.context("task disappeared between reads")?;
    let failed_item_id: Option<String> = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id=?1 AND status='failed'
             ORDER BY order_no ASC LIMIT 1",
            [task_id],
            |row| row.get(0),
        )
        .optional()?;
    let latest_run: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT cr.id, cr.session_id FROM command_runs cr
             JOIN task_items ti ON ti.id=cr.task_item_id
             WHERE ti.task_id=?1 ORDER BY cr.started_at DESC LIMIT 1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(Some(BoundaryInputs {
        project_id,
        current_cycle,
        execution_plan_json,
        state_version,
        failed_item_id,
        latest_run,
    }))
}

fn read_plan_blocking(conn: &Connection, id: &str) -> Result<Option<ResumePlanRow>> {
    conn.query_row(
        "SELECT task_id, mode, expected_state_version, consequence_json,
                execution_input_json, elevated_confirmation_required, expires_at, status
         FROM resume_plans WHERE id=?1",
        [id],
        |row| {
            Ok(ResumePlanRow {
                task_id: row.get(0)?,
                mode: row.get(1)?,
                expected_state_version: row.get(2)?,
                consequence_json: row.get(3)?,
                execution_input_json: row.get(4)?,
                elevated_confirmation_required: row.get(5)?,
                expires_at: row.get(6)?,
                status: row.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn reserve_execution_blocking(conn: &Connection, execution: &NewExecution) -> Result<Reservation> {
    let tx = conn.unchecked_transaction()?;
    if let Some((id, status)) = tx
        .query_row(
            "SELECT id, status FROM resume_executions WHERE plan_id=?1 AND idempotency_key=?2",
            params![execution.plan_id, execution.idempotency_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        tx.commit()?;
        return Ok(Reservation::Existing { id, status });
    }
    let claimed = tx.execute(
        "UPDATE resume_plans SET status='executing'
         WHERE id=?1 AND status='planned' AND expected_state_version=?2",
        params![execution.plan_id, execution.verified_state_version],
    )?;
    if claimed != 1 {
        return Ok(Reservation::PlanMoved);
    }
    tx.execute(
        "INSERT INTO resume_executions
         (id, plan_id, actor, operator_reason, idempotency_key, request_hash, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'executing', ?7)",
        params![
            execution.id,
            execution.plan_id,
            execution.actor,
            execution.operator_reason,
            execution.idempotency_key,
            execution.request_hash,
            execution.created_at,
        ],
    )?;
    tx.commit()?;
    Ok(Reservation::Reserved {
        id: execution.id.clone(),
    })
}

fn complete_execution_blocking(
    conn: &Connection,
    execution_id: &str,
    status: &str,
    child_task_id: Option<&str>,
    error_code: Option<&str>,
    now: &str,
) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let plan_id: Option<String> = tx
        .query_row(
            "SELECT plan_id FROM resume_executions WHERE id=?1 AND status='executing'",
            [execution_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(plan_id) = plan_id else {
        return Ok(false);
    };
    tx.execute(
        "UPDATE resume_executions SET status=?1, child_task_id=?2, error_code=?3, completed_at=?4
         WHERE id=?5",
        params![status, child_task_id, error_code, now, execution_id],
    )?;
    tx.execute(
        "UPDATE resume_plans SET status=?1, executed_at=?2 WHERE id=?3",
        params![status, now, plan_id],
    )?;
    tx.commit()?;
    Ok(true)
}
