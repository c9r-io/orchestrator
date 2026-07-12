//! Immutable task handoffs and fail-closed logical resume planning.
//!
//! A handoff is a deterministic projection of persisted orchestration state. A resume plan is a
//! separate, expiring intent record: callers must reserve execution with the same state version
//! before any scheduler or workspace mutation is attempted.

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config::SideEffectClass;
use crate::config_load::now_ts;
use anyhow::{Result, anyhow, bail};
use chrono::{Duration, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;
use uuid::Uuid;

const PROJECTION_VERSION: i64 = 1;
const MAX_LIST_ENTRIES: usize = 50;
const MAX_TEXT_CHARS: usize = 2_000;

/// Deterministic, bounded briefing content suitable for a handoff panel or a new agent prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffBriefing {
    /// Original task goal.
    pub goal: String,
    /// Current persisted task status and cycle.
    pub current_state: Value,
    /// Most recent successful semantic event, if present.
    pub last_success: Option<Value>,
    /// Most recent failure evidence, if present.
    pub failure: Option<Value>,
    /// Bounded structured test evidence.
    pub test_evidence: Vec<Value>,
    /// Changed file paths found in structured event evidence.
    pub changed_files: Vec<String>,
    /// Persisted constraints safe to expose to another session.
    pub constraints: Vec<String>,
    /// Persisted decisions safe to expose to another session.
    pub decisions: Vec<String>,
    /// Open questions extracted from structured evidence.
    pub open_questions: Vec<String>,
    /// Deterministic next-action recommendations.
    pub recommendations: Vec<String>,
}

/// Immutable persisted handoff snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffSnapshot {
    /// Opaque snapshot identifier.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Source task.
    pub task_id: String,
    /// Highest included event identifier.
    pub source_event_cursor: i64,
    /// Projection schema version.
    pub projection_version: i64,
    /// Deterministic structured briefing.
    pub briefing: HandoffBriefing,
    /// SHA-256 of canonical structured content and cursor.
    pub content_hash: String,
    /// Optimistic task-state fingerprint captured at generation.
    pub state_version: String,
    /// Trusted actor that generated the snapshot.
    pub generated_by: String,
    /// Persistence timestamp; excluded from the content hash.
    pub created_at: String,
}

/// A logical execution boundary. Checkpoint identifiers are references only and never imply a
/// destructive source-control rollback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeBoundary {
    /// Stable boundary identifier derived from task, cycle, step and item.
    pub id: String,
    /// Source task.
    pub task_id: String,
    /// Loop cycle at which replay would start.
    pub cycle: i64,
    /// Step at which replay would start.
    pub step_id: Option<String>,
    /// Optional task-item scope.
    pub task_item_id: Option<String>,
    /// Optional latest command-run reference.
    pub command_run_id: Option<String>,
    /// Optional provider session presence. The opaque token is never returned.
    pub provider_session_available: bool,
    /// Optional source-control checkpoint reference. This is not a rollback instruction.
    pub checkpoint_id: Option<String>,
    /// Strongest declared side effect at the boundary.
    pub side_effect_class: SideEffectClass,
    /// Whether replay is safe without elevated confirmation.
    pub replay_safe: bool,
    /// Human-readable reason for offering the boundary.
    pub reason: String,
    /// Captured optimistic task-state fingerprint.
    pub state_version: String,
}

/// Explicit resume operation selected during planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeMode {
    /// Continue normal scheduling from the persisted task state.
    ContinueTask,
    /// Retry one failed item at its current logical boundary.
    RetryItem,
    /// Create a correlated child task beginning at a logical step boundary.
    RestartFromBoundary,
    /// Ask the provider adapter to reuse an existing provider session.
    ResumeProviderSession,
}

impl ResumeMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ContinueTask => "continue_task",
            Self::RetryItem => "retry_item",
            Self::RestartFromBoundary => "restart_from_boundary",
            Self::ResumeProviderSession => "resume_provider_session",
        }
    }
}

/// Persisted consequence preview produced before mutating execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumePlan {
    /// Opaque plan identifier required by execute.
    pub id: String,
    /// Source task.
    pub task_id: String,
    /// Selected boundary.
    pub boundary: ResumeBoundary,
    /// Selected operation.
    pub mode: ResumeMode,
    /// Expected state fingerprint checked again during execute.
    pub expected_state_version: String,
    /// Deterministic consequence preview.
    pub consequence: Value,
    /// Whether elevated confirmation is required.
    pub elevated_confirmation_required: bool,
    /// Plan expiration timestamp.
    pub expires_at: String,
    /// Plan lifecycle status.
    pub status: String,
}

/// Result of atomically reserving a plan execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeExecutionReservation {
    /// Durable execution identifier.
    pub id: String,
    /// Plan being executed.
    pub plan_id: String,
    /// True only for the caller that owns subsequent scheduler side effects.
    pub should_execute: bool,
    /// Current durable execution status.
    pub status: String,
}

/// SQLite-backed handoff and resume repository.
#[derive(Clone)]
pub struct AsyncHandoffRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncHandoffRepository {
    /// Creates a repository sharing the daemon database connections.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Generates or returns an immutable deterministic snapshot at the requested event cursor.
    pub async fn generate_snapshot(
        &self,
        task_id: &str,
        requested_cursor: Option<i64>,
        actor: &str,
    ) -> Result<HandoffSnapshot> {
        let task_id = task_id.to_owned();
        let actor = actor.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                generate_snapshot(conn, &task_id, requested_cursor, &actor).map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns one immutable snapshot.
    pub async fn get_snapshot(&self, id: &str) -> Result<Option<HandoffSnapshot>> {
        let id = id.to_owned();
        self.db
            .reader()
            .call(move |conn| read_snapshot(conn, &id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Lists logical resume boundaries without changing task or workspace state.
    pub async fn list_boundaries(&self, task_id: &str) -> Result<Vec<ResumeBoundary>> {
        let task_id = task_id.to_owned();
        self.db
            .reader()
            .call(move |conn| list_boundaries(conn, &task_id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Persists an expiring consequence preview. No task or workspace mutation occurs.
    pub async fn create_plan(
        &self,
        task_id: &str,
        boundary_id: &str,
        mode: ResumeMode,
        actor: &str,
        attention_item_id: Option<&str>,
    ) -> Result<ResumePlan> {
        let task_id = task_id.to_owned();
        let boundary_id = boundary_id.to_owned();
        let actor = actor.to_owned();
        let attention_item_id = attention_item_id.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                create_plan(
                    conn,
                    &task_id,
                    &boundary_id,
                    mode,
                    &actor,
                    attention_item_id.as_deref(),
                )
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Atomically reserves a plan after stale-state, expiry, policy and confirmation checks.
    /// The caller may perform scheduler side effects only when `should_execute` is true.
    pub async fn reserve_execution(
        &self,
        plan_id: &str,
        expected_state_version: &str,
        idempotency_key: &str,
        actor: &str,
        operator_reason: &str,
        elevated_confirmation: bool,
        elevated_policy_enabled: bool,
    ) -> Result<ResumeExecutionReservation> {
        let plan_id = plan_id.to_owned();
        let expected = expected_state_version.to_owned();
        let key = idempotency_key.to_owned();
        let actor = actor.to_owned();
        let reason = operator_reason.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                reserve_execution(
                    conn,
                    &plan_id,
                    &expected,
                    &key,
                    &actor,
                    &reason,
                    elevated_confirmation,
                    elevated_policy_enabled,
                )
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Completes an owned execution reservation after the scheduler mutation finishes.
    pub async fn complete_execution(
        &self,
        execution_id: &str,
        child_task_id: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<()> {
        let execution_id = execution_id.to_owned();
        let child_task_id = child_task_id.map(str::to_owned);
        let error_code = error_code.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                let tx = conn.unchecked_transaction()?;
                let status = if error_code.is_some() { "failed" } else { "succeeded" };
                let plan_id: String = tx.query_row(
                    "SELECT plan_id FROM resume_executions WHERE id=?1 AND status='executing'",
                    [&execution_id],
                    |row| row.get(0),
                )?;
                tx.execute(
                    "UPDATE resume_executions SET status=?1, child_task_id=?2, error_code=?3, completed_at=?4 WHERE id=?5",
                    params![status, child_task_id, error_code, now_ts(), execution_id],
                )?;
                tx.execute(
                    "UPDATE resume_plans SET status=?1, executed_at=?2 WHERE id=?3",
                    params![status, now_ts(), plan_id],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }
}

fn generate_snapshot(
    conn: &Connection,
    task_id: &str,
    requested_cursor: Option<i64>,
    actor: &str,
) -> Result<HandoffSnapshot> {
    let (project_id, goal, status, cycle): (String, String, String, i64) = conn
        .query_row(
            "SELECT project_id, goal, status, current_cycle FROM tasks WHERE id=?1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| anyhow!("task not found: {task_id}"))?;
    let max_cursor: i64 = conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM events WHERE task_id=?1",
        [task_id],
        |row| row.get(0),
    )?;
    let cursor = requested_cursor.unwrap_or(max_cursor);
    if cursor > max_cursor || cursor < 0 {
        bail!("invalid handoff cursor {cursor}; latest is {max_cursor}");
    }

    let mut stmt = conn.prepare(
        "SELECT event_type, payload_json, created_at FROM events
         WHERE task_id=?1 AND id<=?2 ORDER BY id ASC",
    )?;
    let events = stmt
        .query_map(params![task_id, cursor], |row| {
            let payload: String = row.get(1)?;
            Ok((
                row.get::<_, String>(0)?,
                serde_json::from_str::<Value>(&payload).unwrap_or_default(),
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let briefing = project_briefing(goal, status, cycle, &events);
    let state_version = task_state_version(conn, task_id)?;
    let hash_input = json!({
        "task_id": task_id,
        "cursor": cursor,
        "projection_version": PROJECTION_VERSION,
        "briefing": briefing,
    });
    let content_hash = hash_value(&hash_input)?;

    if let Some(existing_id) = conn
        .query_row(
            "SELECT id FROM handoff_snapshots WHERE task_id=?1 AND source_event_cursor=?2 AND content_hash=?3",
            params![task_id, cursor, content_hash],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return read_snapshot(conn, &existing_id)?
            .ok_or_else(|| anyhow!("handoff snapshot disappeared"));
    }

    let id = Uuid::new_v4().to_string();
    let created_at = now_ts();
    conn.execute(
        "INSERT INTO handoff_snapshots
         (id, project_id, task_id, source_event_cursor, projection_version, briefing_json,
          content_hash, state_version, generated_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            project_id,
            task_id,
            cursor,
            PROJECTION_VERSION,
            serde_json::to_string(&briefing)?,
            content_hash,
            state_version,
            actor,
            created_at,
        ],
    )?;
    read_snapshot(conn, &id)?.ok_or_else(|| anyhow!("failed to persist handoff snapshot"))
}

fn read_snapshot(conn: &Connection, id: &str) -> Result<Option<HandoffSnapshot>> {
    conn.query_row(
        "SELECT id, project_id, task_id, source_event_cursor, projection_version, briefing_json,
                content_hash, state_version, generated_by, created_at
         FROM handoff_snapshots WHERE id=?1",
        [id],
        |row| {
            let raw: String = row.get(5)?;
            let briefing = serde_json::from_str(&raw).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    raw.len(),
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
            Ok(HandoffSnapshot {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_id: row.get(2)?,
                source_event_cursor: row.get(3)?,
                projection_version: row.get(4)?,
                briefing,
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

fn project_briefing(
    goal: String,
    status: String,
    cycle: i64,
    events: &[(String, Value, String)],
) -> HandoffBriefing {
    let mut last_success = None;
    let mut failure = None;
    let mut tests = Vec::new();
    let mut changed_files = BTreeSet::new();
    let mut constraints = BTreeSet::new();
    let mut decisions = BTreeSet::new();
    let mut questions = BTreeSet::new();

    for (event_type, payload, created_at) in events {
        let normalized = event_type.to_ascii_lowercase();
        let evidence = json!({
            "event_type": event_type,
            "created_at": created_at,
            "summary": bounded_payload(payload),
        });
        if normalized.contains("success") || normalized.contains("completed") {
            last_success = Some(evidence.clone());
        }
        if normalized.contains("fail") || normalized.contains("error") {
            failure = Some(evidence.clone());
        }
        if normalized.contains("test") || normalized.contains("qa") || normalized.contains("lint") {
            tests.push(evidence);
        }
        collect_string_values(payload, &["changed_files", "files"], &mut changed_files);
        collect_string_values(payload, &["constraints"], &mut constraints);
        collect_string_values(payload, &["decisions"], &mut decisions);
        collect_string_values(payload, &["open_questions", "questions"], &mut questions);
    }

    let recommendations = if failure.is_some() {
        vec![
            "Review the failure evidence and choose a listed logical resume boundary.".to_string(),
            "Generate a consequence preview before executing any retry or restart.".to_string(),
        ]
    } else if status == "completed" {
        vec!["Review the final evidence before closing the task.".to_string()]
    } else {
        vec!["Continue from the latest persisted orchestration state.".to_string()]
    };

    HandoffBriefing {
        goal: truncate_text(&goal),
        current_state: json!({"status": status, "cycle": cycle}),
        last_success,
        failure,
        test_evidence: tests.into_iter().rev().take(MAX_LIST_ENTRIES).collect(),
        changed_files: changed_files.into_iter().take(MAX_LIST_ENTRIES).collect(),
        constraints: constraints.into_iter().take(MAX_LIST_ENTRIES).collect(),
        decisions: decisions.into_iter().take(MAX_LIST_ENTRIES).collect(),
        open_questions: questions.into_iter().take(MAX_LIST_ENTRIES).collect(),
        recommendations,
    }
}

fn collect_string_values(value: &Value, keys: &[&str], output: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                if keys.contains(&key.as_str()) {
                    match nested {
                        Value::String(text) => {
                            output.insert(truncate_text(text));
                        }
                        Value::Array(values) => {
                            for value in values {
                                if let Some(text) = value.as_str() {
                                    output.insert(truncate_text(text));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                collect_string_values(nested, keys, output);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_string_values(nested, keys, output);
            }
        }
        _ => {}
    }
}

fn bounded_payload(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(truncate_text(text)),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(MAX_LIST_ENTRIES)
                .map(bounded_payload)
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .filter(|(key, _)| !is_sensitive_key(key))
                .take(MAX_LIST_ENTRIES)
                .map(|(key, value)| (key.clone(), bounded_payload(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["token", "secret", "password", "authorization", "cookie"]
        .iter()
        .any(|pattern| key.contains(pattern))
}

fn truncate_text(value: &str) -> String {
    value.chars().take(MAX_TEXT_CHARS).collect()
}

fn task_state_version(conn: &Connection, task_id: &str) -> Result<String> {
    let value: Value = conn.query_row(
        "SELECT status, current_cycle, init_done, pipeline_vars_json, execution_plan_json,
                updated_at, (SELECT COALESCE(MAX(id), 0) FROM events WHERE task_id=tasks.id)
         FROM tasks WHERE id=?1",
        [task_id],
        |row| {
            let pipeline: String = row.get(3)?;
            let plan: String = row.get(4)?;
            Ok(json!({
                "status": row.get::<_, String>(0)?,
                "current_cycle": row.get::<_, i64>(1)?,
                "init_done": row.get::<_, i64>(2)?,
                "pipeline_vars": serde_json::from_str::<Value>(&pipeline).unwrap_or_default(),
                "execution_plan": serde_json::from_str::<Value>(&plan).unwrap_or_default(),
                "updated_at": row.get::<_, String>(5)?,
                "event_cursor": row.get::<_, i64>(6)?,
            }))
        },
    )?;
    hash_value(&value)
}

fn list_boundaries(conn: &Connection, task_id: &str) -> Result<Vec<ResumeBoundary>> {
    let (cycle, execution_plan): (i64, String) = conn.query_row(
        "SELECT current_cycle, execution_plan_json FROM tasks WHERE id=?1",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let state_version = task_state_version(conn, task_id)?;
    let plan: Value = serde_json::from_str(&execution_plan).unwrap_or_default();
    let steps = plan
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let failed_item: Option<String> = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id=?1 AND status='failed' ORDER BY order_no ASC LIMIT 1",
            [task_id],
            |row| row.get(0),
        )
        .optional()?;
    let latest_run: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT cr.id, cr.session_id FROM command_runs cr JOIN task_items ti ON ti.id=cr.task_item_id
             WHERE ti.task_id=?1 ORDER BY cr.started_at DESC LIMIT 1",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let mut boundaries = Vec::new();
    for step in steps {
        let Some(step_id) = step.get("id").and_then(Value::as_str) else {
            continue;
        };
        let side_effect_class = step
            .pointer("/behavior/side_effect_class")
            .and_then(Value::as_str)
            .and_then(parse_side_effect)
            .unwrap_or_else(|| infer_side_effect(&step));
        let item = failed_item.clone();
        let id = stable_boundary_id(task_id, cycle, Some(step_id), item.as_deref());
        boundaries.push(ResumeBoundary {
            id,
            task_id: task_id.to_string(),
            cycle,
            step_id: Some(step_id.to_string()),
            task_item_id: item,
            command_run_id: latest_run.as_ref().map(|run| run.0.clone()),
            provider_session_available: latest_run
                .as_ref()
                .and_then(|run| run.1.as_ref())
                .is_some(),
            checkpoint_id: None,
            side_effect_class,
            replay_safe: side_effect_class.replay_safe(),
            reason: format!("Restart orchestration at logical step {step_id}"),
            state_version: state_version.clone(),
        });
    }
    if boundaries.is_empty() {
        let side_effect_class = SideEffectClass::NonIdempotentExternal;
        boundaries.push(ResumeBoundary {
            id: stable_boundary_id(task_id, cycle, None, failed_item.as_deref()),
            task_id: task_id.to_string(),
            cycle,
            step_id: None,
            task_item_id: failed_item,
            command_run_id: latest_run.as_ref().map(|run| run.0.clone()),
            provider_session_available: latest_run
                .as_ref()
                .and_then(|run| run.1.as_ref())
                .is_some(),
            checkpoint_id: None,
            side_effect_class,
            replay_safe: false,
            reason: "Continue from the persisted task state; workflow side effects are undeclared"
                .to_string(),
            state_version,
        });
    }
    Ok(boundaries)
}

fn infer_side_effect(step: &Value) -> SideEffectClass {
    if step.get("command").and_then(Value::as_str).is_some()
        || step
            .get("required_capability")
            .and_then(Value::as_str)
            .is_some()
    {
        SideEffectClass::NonIdempotentExternal
    } else {
        SideEffectClass::None
    }
}

fn parse_side_effect(value: &str) -> Option<SideEffectClass> {
    match value {
        "none" => Some(SideEffectClass::None),
        "workspace_only" => Some(SideEffectClass::WorkspaceOnly),
        "idempotent_external" => Some(SideEffectClass::IdempotentExternal),
        "non_idempotent_external" => Some(SideEffectClass::NonIdempotentExternal),
        _ => None,
    }
}

fn stable_boundary_id(
    task_id: &str,
    cycle: i64,
    step_id: Option<&str>,
    item_id: Option<&str>,
) -> String {
    let raw = format!(
        "{task_id}:{cycle}:{}:{}",
        step_id.unwrap_or("current"),
        item_id.unwrap_or("task")
    );
    format!("rb-{}", encode_hex(&Sha256::digest(raw.as_bytes())))
}

fn create_plan(
    conn: &Connection,
    task_id: &str,
    boundary_id: &str,
    mode: ResumeMode,
    actor: &str,
    attention_item_id: Option<&str>,
) -> Result<ResumePlan> {
    let boundary = list_boundaries(conn, task_id)?
        .into_iter()
        .find(|boundary| boundary.id == boundary_id)
        .ok_or_else(|| anyhow!("resume boundary is stale or unknown"))?;
    if mode == ResumeMode::RetryItem && boundary.task_item_id.is_none() {
        bail!("retry_item requires an item-scoped boundary");
    }
    if mode == ResumeMode::ResumeProviderSession && !boundary.provider_session_available {
        bail!("provider session is unavailable; create a restart_from_boundary plan instead");
    }
    let plan_id = Uuid::new_v4().to_string();
    let elevated = !boundary.replay_safe;
    let consequence = json!({
        "mode": mode,
        "task_id": task_id,
        "boundary_id": boundary.id,
        "step_id": boundary.step_id,
        "task_item_id": boundary.task_item_id,
        "creates_correlated_child": mode == ResumeMode::RestartFromBoundary || mode == ResumeMode::ResumeProviderSession,
        "workspace_rollback": false,
        "provider_session_reuse": mode == ResumeMode::ResumeProviderSession,
        "fallback_mode": if mode == ResumeMode::ResumeProviderSession { Value::String("restart_from_boundary".to_string()) } else { Value::Null },
    });
    let expires_at = (Utc::now() + Duration::minutes(15)).to_rfc3339();
    let project_id: String = conn.query_row(
        "SELECT project_id FROM tasks WHERE id=?1",
        [task_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO resume_plans
         (id, project_id, task_id, attention_item_id, boundary_id, mode,
          expected_state_version, side_effect_class, replay_safe,
          elevated_confirmation_required, consequence_json, execution_input_json,
          provider_command_run_id, status, expires_at, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, '{}', ?12,
                 'planned', ?13, ?14, ?15)",
        params![
            plan_id,
            project_id,
            task_id,
            attention_item_id,
            boundary.id,
            mode.as_str(),
            boundary.state_version,
            side_effect_label(boundary.side_effect_class),
            i64::from(boundary.replay_safe),
            i64::from(elevated),
            serde_json::to_string(&consequence)?,
            boundary.command_run_id,
            expires_at,
            actor,
            now_ts(),
        ],
    )?;
    Ok(ResumePlan {
        id: plan_id,
        task_id: task_id.to_string(),
        expected_state_version: boundary.state_version.clone(),
        boundary,
        mode,
        consequence,
        elevated_confirmation_required: elevated,
        expires_at,
        status: "planned".to_string(),
    })
}

fn reserve_execution(
    conn: &Connection,
    plan_id: &str,
    expected_state_version: &str,
    idempotency_key: &str,
    actor: &str,
    operator_reason: &str,
    elevated_confirmation: bool,
    elevated_policy_enabled: bool,
) -> Result<ResumeExecutionReservation> {
    if idempotency_key.trim().is_empty() || operator_reason.trim().is_empty() {
        bail!("idempotency_key and operator_reason are required");
    }
    let tx = conn.unchecked_transaction()?;
    if let Some(existing) = tx
        .query_row(
            "SELECT id, status FROM resume_executions WHERE plan_id=?1 AND idempotency_key=?2",
            params![plan_id, idempotency_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        tx.commit()?;
        return Ok(ResumeExecutionReservation {
            id: existing.0,
            plan_id: plan_id.to_string(),
            should_execute: false,
            status: existing.1,
        });
    }
    let (task_id, planned_version, status, expires_at, elevated): (
        String,
        String,
        String,
        String,
        i64,
    ) = tx.query_row(
        "SELECT task_id, expected_state_version, status, expires_at,
                elevated_confirmation_required FROM resume_plans WHERE id=?1",
        [plan_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    if status != "planned" {
        bail!("resume plan is not executable: {status}");
    }
    let expires = chrono::DateTime::parse_from_rfc3339(&expires_at)?;
    if expires < Utc::now() {
        bail!("resume plan has expired");
    }
    let current_version = task_state_version(&tx, &task_id)?;
    if expected_state_version != planned_version || current_version != planned_version {
        bail!("stale resume plan: task state changed; generate a new consequence preview");
    }
    if elevated != 0 && (!elevated_policy_enabled || !elevated_confirmation) {
        bail!("non-idempotent replay denied: elevated policy and confirmation are required");
    }
    let request_hash = hash_value(&json!({
        "plan_id": plan_id,
        "expected_state_version": expected_state_version,
        "actor": actor,
        "operator_reason": operator_reason,
        "elevated_confirmation": elevated_confirmation,
    }))?;
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO resume_executions
         (id, plan_id, actor, operator_reason, idempotency_key, request_hash, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'executing', ?7)",
        params![
            id,
            plan_id,
            actor,
            operator_reason,
            idempotency_key,
            request_hash,
            now_ts(),
        ],
    )?;
    tx.execute(
        "UPDATE resume_plans SET status='executing' WHERE id=?1 AND status='planned'",
        [plan_id],
    )?;
    tx.commit()?;
    Ok(ResumeExecutionReservation {
        id,
        plan_id: plan_id.to_string(),
        should_execute: true,
        status: "executing".to_string(),
    })
}

fn side_effect_label(value: SideEffectClass) -> &'static str {
    match value {
        SideEffectClass::None => "none",
        SideEffectClass::WorkspaceOnly => "workspace_only",
        SideEffectClass::IdempotentExternal => "idempotent_external",
        SideEffectClass::NonIdempotentExternal => "non_idempotent_external",
    }
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = Map::new();
            for key in keys {
                if let Some(value) = map.get(key) {
                    canonical.insert(key.clone(), canonicalize(value));
                }
            }
            Value::Object(canonical)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

fn hash_value(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(&canonicalize(value))?;
    Ok(encode_hex(&Sha256::digest(bytes)))
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use tempfile::tempdir;

    async fn repository() -> (tempfile::TempDir, AsyncHandoffRepository) {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("handoff.db");
        init_schema(&path).expect("schema");
        let conn = Connection::open(&path).expect("open seed connection");
        conn.execute(
            "INSERT INTO tasks
             (id,name,status,goal,target_files_json,mode,workspace_id,workflow_id,project_id,
              workspace_root,qa_targets_json,ticket_dir,execution_plan_json,loop_mode,
              current_cycle,init_done,created_at,updated_at,pipeline_vars_json)
             VALUES ('task-1','task','failed','Fix the failing test','[]','once','ws','wf','p1',
                     '/tmp/ws','[]','docs/ticket',?1,'once',1,1,'2026-01-01','2026-01-01','{}')",
            [json!({"steps":[{"id":"test","builtin":"self_test","behavior":{"side_effect_class":"workspace_only"}}]}).to_string()],
        )
        .expect("seed task");
        conn.execute(
            "INSERT INTO events(task_id,event_type,payload_json,created_at)
             VALUES ('task-1','step_failed',?1,'2026-01-01T00:00:00Z')",
            [json!({"message":"tests failed","changed_files":["src/lib.rs"],"token":"must-redact"}).to_string()],
        )
        .expect("seed event");
        drop(conn);
        let db = Arc::new(AsyncDatabase::open(&path).await.expect("async db"));
        (temp, AsyncHandoffRepository::new(db))
    }

    #[tokio::test]
    async fn same_cursor_returns_same_content_hash_and_snapshot() {
        let (_temp, repository) = repository().await;
        let first = repository
            .generate_snapshot("task-1", Some(1), "operator")
            .await
            .expect("first");
        let second = repository
            .generate_snapshot("task-1", Some(1), "another-operator")
            .await
            .expect("second");
        assert_eq!(first.id, second.id);
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(first.briefing.changed_files, vec!["src/lib.rs"]);
        assert!(
            !serde_json::to_string(&first)
                .expect("json")
                .contains("must-redact")
        );
    }

    #[tokio::test]
    async fn stale_plan_is_rejected_before_execution_reservation() {
        let (_temp, repository) = repository().await;
        let boundary = repository
            .list_boundaries("task-1")
            .await
            .expect("boundaries")
            .remove(0);
        let plan = repository
            .create_plan(
                "task-1",
                &boundary.id,
                ResumeMode::RestartFromBoundary,
                "operator",
                None,
            )
            .await
            .expect("plan");
        repository
            .db
            .writer()
            .call(|conn| {
                conn.execute(
                    "UPDATE tasks SET current_cycle=current_cycle+1, updated_at='2026-01-02' WHERE id='task-1'",
                    [],
                )?;
                Ok(())
            })
            .await
            .expect("mutate task");
        let error = repository
            .reserve_execution(
                &plan.id,
                &plan.expected_state_version,
                "key-1",
                "operator",
                "retry after review",
                false,
                false,
            )
            .await
            .expect_err("stale plan must fail");
        assert!(error.to_string().contains("stale resume plan"));
    }

    #[tokio::test]
    async fn non_idempotent_boundary_requires_policy_and_confirmation() {
        let (_temp, repository) = repository().await;
        repository
            .db
            .writer()
            .call(|conn| {
                conn.execute(
                    "UPDATE tasks SET execution_plan_json=?1 WHERE id='task-1'",
                    [
                        json!({"steps":[{"id":"deploy","required_capability":"deploy"}]})
                            .to_string(),
                    ],
                )?;
                Ok(())
            })
            .await
            .expect("change plan");
        let boundary = repository
            .list_boundaries("task-1")
            .await
            .expect("boundaries")
            .remove(0);
        assert!(!boundary.replay_safe);
        let plan = repository
            .create_plan(
                "task-1",
                &boundary.id,
                ResumeMode::RestartFromBoundary,
                "operator",
                None,
            )
            .await
            .expect("plan");
        let error = repository
            .reserve_execution(
                &plan.id,
                &plan.expected_state_version,
                "key-2",
                "operator",
                "deploy again",
                true,
                false,
            )
            .await
            .expect_err("policy disabled");
        assert!(error.to_string().contains("non-idempotent replay denied"));
    }
}
