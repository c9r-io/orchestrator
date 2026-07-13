//! Persistent operational queue for human-actionable workflow conditions.

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Semantic attention severity. Intervention items sort before attention items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionSeverity {
    /// Work cannot safely continue without a human decision.
    Intervention,
    /// A human should review the condition, but the system is not necessarily blocked.
    Attention,
}

impl AttentionSeverity {
    /// Returns the stable wire and storage label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intervention => "intervention",
            Self::Attention => "attention",
        }
    }
}

/// Lifecycle state of an attention item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState {
    /// Unassigned actionable work.
    Open,
    /// Work owned by one actor.
    Claimed,
    /// Work hidden until a specified time.
    Snoozed,
    /// Condition cleared or explicitly decided.
    Resolved,
}

impl AttentionState {
    /// Returns the stable wire and storage label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Claimed => "claimed",
            Self::Snoozed => "snoozed",
            Self::Resolved => "resolved",
        }
    }
}

/// Safe action descriptor rendered by clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttentionActionDescriptor {
    /// Allowlisted action identifier.
    pub id: String,
    /// Human-readable action label.
    pub label: String,
    /// Minimum control-plane role.
    pub required_role: String,
    /// Whether explicit confirmation is required.
    pub confirmation: String,
    /// Bounded JSON input schema for UI rendering.
    pub input_schema: serde_json::Value,
}

/// Persisted attention item returned by repository and public APIs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttentionItem {
    /// Stable identifier.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Originating task.
    pub task_id: String,
    /// Optional originating task item.
    pub task_item_id: Option<String>,
    /// Optional workflow step.
    pub step_id: Option<String>,
    /// Optional interactive session.
    pub session_id: Option<String>,
    /// Built-in policy kind.
    pub kind: String,
    /// Severity label.
    pub severity: String,
    /// Lifecycle state.
    pub state: String,
    /// Redacted title.
    pub title: String,
    /// Redacted bounded summary.
    pub summary: String,
    /// Optional requested-decision schema/value.
    pub requested_decision: Option<serde_json::Value>,
    /// Safe allowlisted actions.
    pub actions: Vec<AttentionActionDescriptor>,
    /// Stable active-condition deduplication key.
    pub dedupe_key: String,
    /// Trusted actor currently owning the item.
    pub assignee: Option<String>,
    /// Source event identifier.
    pub source_event_id: String,
    /// Number of occurrences aggregated into this item.
    pub occurrence_count: i64,
    /// Number of resolved-to-open transitions.
    pub reopen_count: i64,
    /// Optimistic concurrency version.
    pub version: i64,
    /// Creation timestamp.
    pub created_at: String,
    /// Last mutation timestamp.
    pub updated_at: String,
    /// Most recent source occurrence timestamp.
    pub last_occurred_at: String,
    /// Optional snooze deadline.
    pub snoozed_until: Option<String>,
    /// Optional SLA deadline.
    pub sla_deadline: Option<String>,
    /// Optional resolution timestamp.
    pub resolved_at: Option<String>,
    /// Optional structured resolution.
    pub resolution: Option<serde_json::Value>,
}

/// Filter for cross-task attention queries.
#[derive(Debug, Clone, Default)]
pub struct AttentionFilter {
    /// Project filter.
    pub project_id: Option<String>,
    /// State filter.
    pub state: Option<String>,
    /// Kind filter.
    pub kind: Option<String>,
    /// Severity filter.
    pub severity: Option<String>,
    /// Assignee filter (`me`, `unassigned`, or actor ID).
    pub assignee: Option<String>,
    /// Task filter.
    pub task_id: Option<String>,
    /// Maximum results.
    pub limit: usize,
}

/// A source event waiting for attention policy evaluation.
#[derive(Debug, Clone)]
pub struct AttentionSourceEvent {
    /// Numeric event-table identifier.
    pub id: i64,
    /// Project owning the task.
    pub project_id: String,
    /// Task identifier.
    pub task_id: String,
    /// Optional task item.
    pub task_item_id: Option<String>,
    /// Event type.
    pub event_type: String,
    /// Parsed event payload.
    pub payload: serde_json::Value,
    /// Event timestamp.
    pub created_at: String,
}

/// Materialized candidate produced by a built-in policy.
#[derive(Debug, Clone)]
pub struct AttentionCandidate {
    /// Deterministic item identifier used on first materialization.
    pub id: String,
    /// Project scope.
    pub project_id: String,
    /// Task scope.
    pub task_id: String,
    /// Optional task item.
    pub task_item_id: Option<String>,
    /// Optional step.
    pub step_id: Option<String>,
    /// Optional session.
    pub session_id: Option<String>,
    /// Policy kind.
    pub kind: String,
    /// Severity.
    pub severity: AttentionSeverity,
    /// Redacted title.
    pub title: String,
    /// Redacted summary.
    pub summary: String,
    /// Requested decision payload.
    pub requested_decision: Option<serde_json::Value>,
    /// Safe actions.
    pub actions: Vec<AttentionActionDescriptor>,
    /// Stable active dedupe key.
    pub dedupe_key: String,
    /// Source event ID.
    pub source_event_id: String,
    /// Source timestamp.
    pub occurred_at: String,
    /// Optional SLA deadline.
    pub sla_deadline: Option<String>,
}

/// One operation in an atomic projector batch.
#[derive(Debug, Clone)]
pub enum AttentionProjectionOp {
    /// Create, aggregate, or reopen a condition.
    Upsert(Box<AttentionCandidate>),
    /// Resolve matching active step conditions.
    ResolveStep {
        /// Task identifier.
        task_id: String,
        /// Optional task item.
        task_item_id: Option<String>,
        /// Step identifier.
        step_id: String,
        /// Source event identifier.
        source_event_id: String,
    },
    /// Resolve all active items for a terminal task.
    ResolveTask {
        /// Task identifier.
        task_id: String,
        /// Source event identifier.
        source_event_id: String,
    },
}

/// Mutation requested by an authenticated operator.
#[derive(Debug, Clone)]
pub enum AttentionMutation {
    /// Claim an open item.
    Claim,
    /// Snooze an open or claimed item until RFC3339 time.
    Snooze {
        /// RFC3339 deadline after which the item becomes open again.
        until: String,
    },
    /// Resolve an active item with a structured reason.
    Resolve {
        /// Short operator-supplied resolution reason.
        reason: String,
    },
}

impl AttentionMutation {
    fn kind(&self) -> &'static str {
        match self {
            Self::Claim => "claim",
            Self::Snooze { .. } => "snooze",
            Self::Resolve { .. } => "resolve",
        }
    }

    fn request_value(&self) -> serde_json::Value {
        match self {
            Self::Claim => serde_json::json!({}),
            Self::Snooze { until } => serde_json::json!({"until": until}),
            Self::Resolve { reason } => serde_json::json!({"reason": reason}),
        }
    }
}

/// Monotonic change record used by follow streams.
#[derive(Debug, Clone)]
pub struct AttentionChange {
    /// Change sequence.
    pub id: i64,
    /// Item identifier.
    pub attention_item_id: String,
    /// Change kind (`upsert` or `remove`).
    pub change_kind: String,
    /// Item version after the change.
    pub item_version: i64,
}

/// Result of atomically reserving an allowlisted action.
#[derive(Debug, Clone)]
pub struct AttentionActionReservation {
    /// Item after the reservation (or current item for an idempotent replay).
    pub item: AttentionItem,
    /// True only for the caller that owns the external side effect.
    pub should_execute: bool,
}

/// Async SQLite repository for attention state.
#[derive(Clone)]
pub struct AsyncAttentionRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncAttentionRepository {
    /// Creates a repository backed by the shared async database.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Returns one attention item.
    pub async fn get(&self, id: &str) -> Result<Option<AttentionItem>> {
        let id = id.to_owned();
        self.db
            .reader()
            .call(move |conn| read_item(conn, &id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Lists attention items with operator-oriented ordering.
    pub async fn list(
        &self,
        filter: AttentionFilter,
        current_actor: Option<&str>,
    ) -> Result<Vec<AttentionItem>> {
        let actor = current_actor.map(str::to_owned);
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM attention_items ORDER BY updated_at DESC LIMIT 1000",
                )?;
                let ids = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let mut loaded = Vec::new();
                for id in ids {
                    if let Some(item) = read_item(conn, &id).map_err(other)? {
                        loaded.push(item);
                    }
                }
                let mut items = loaded
                    .into_iter()
                    .filter(|item| filter_matches(item, &filter, actor.as_deref()))
                    .collect::<Vec<_>>();
                items.sort_by(|left, right| attention_order(left, right, actor.as_deref()));
                items.truncate(filter.limit.clamp(1, 500));
                Ok(items)
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads source events after the durable projector cursor.
    pub async fn load_source_events(
        &self,
        after_id: i64,
        limit: usize,
    ) -> Result<Vec<AttentionSourceEvent>> {
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT e.id, t.project_id, e.task_id, e.task_item_id, e.event_type,
                            e.payload_json, e.created_at
                     FROM events e JOIN tasks t ON t.id = e.task_id
                     WHERE e.id > ?1 ORDER BY e.id ASC LIMIT ?2",
                )?;
                let rows =
                    stmt.query_map(params![after_id, limit.clamp(1, 1000) as i64], |row| {
                        let raw: String = row.get(5)?;
                        Ok(AttentionSourceEvent {
                            id: row.get(0)?,
                            project_id: row.get(1)?,
                            task_id: row.get(2)?,
                            task_item_id: row.get(3)?,
                            event_type: row.get(4)?,
                            payload: serde_json::from_str(&raw).unwrap_or_default(),
                            created_at: row.get(6)?,
                        })
                    })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns the last committed source-event cursor.
    pub async fn projector_cursor(&self) -> Result<i64> {
        self.db
            .reader()
            .call(|conn| {
                Ok(conn.query_row(
                    "SELECT last_event_id FROM attention_projector_state WHERE projector='builtin'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
            .map_err(flatten_err)
    }

    /// Applies materialization operations and advances the cursor atomically.
    pub async fn apply_projection_batch(
        &self,
        operations: Vec<AttentionProjectionOp>,
        last_event_id: i64,
    ) -> Result<()> {
        self.db
            .writer()
            .call(move |conn| {
                let tx = conn.unchecked_transaction()?;
                for operation in operations {
                    apply_projection_op(&tx, operation).map_err(other)?;
                }
                tx.execute(
                    "UPDATE attention_projector_state SET last_event_id=?1, updated_at=?2 WHERE projector='builtin'",
                    params![last_event_id, now_ts()],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    /// Upserts one provider-originated attention candidate without advancing
    /// the built-in task-event projector cursor.
    pub async fn upsert_external_candidate(
        &self,
        candidate: AttentionCandidate,
    ) -> Result<AttentionItem> {
        let id = candidate.id.clone();
        self.db
            .writer()
            .call(move |conn| {
                let tx = conn.unchecked_transaction()?;
                apply_projection_op(&tx, AttentionProjectionOp::Upsert(Box::new(candidate)))
                    .map_err(other)?;
                tx.commit()?;
                read_item(conn, &id)
                    .map_err(other)?
                    .ok_or_else(|| other(anyhow!("external attention item missing")))
            })
            .await
            .map_err(flatten_err)
    }

    /// Applies an optimistic, idempotent human mutation.
    pub async fn mutate(
        &self,
        id: &str,
        expected_version: i64,
        idempotency_key: &str,
        actor: &str,
        mutation: AttentionMutation,
    ) -> Result<AttentionItem> {
        let id = id.to_owned();
        let key = idempotency_key.to_owned();
        let actor = actor.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                mutate_item(conn, &id, expected_version, &key, &actor, mutation).map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Atomically reserves an action so only one concurrent caller may execute it.
    pub async fn reserve_action(
        &self,
        id: &str,
        expected_version: i64,
        idempotency_key: &str,
        actor: &str,
        action_id: &str,
        input: &serde_json::Value,
    ) -> Result<AttentionActionReservation> {
        let id = id.to_owned();
        let key = idempotency_key.to_owned();
        let actor = actor.to_owned();
        let action_id = action_id.to_owned();
        let input = input.clone();
        self.db
            .writer()
            .call(move |conn| {
                reserve_action(
                    conn,
                    &id,
                    expected_version,
                    &key,
                    &actor,
                    &action_id,
                    &input,
                )
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Completes a reserved action and records its outcome.
    pub async fn complete_action(
        &self,
        id: &str,
        idempotency_key: &str,
        actor: &str,
        action_id: &str,
        error_code: Option<&str>,
    ) -> Result<AttentionItem> {
        let id = id.to_owned();
        let key = idempotency_key.to_owned();
        let actor = actor.to_owned();
        let action_id = action_id.to_owned();
        let error = error_code.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                complete_action(conn, &id, &key, &actor, &action_id, error.as_deref())
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns changes after a monotonic sequence.
    pub async fn changes_since(&self, after: i64, limit: usize) -> Result<Vec<AttentionChange>> {
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, attention_item_id, change_kind, item_version
                     FROM attention_changes WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![after, limit.clamp(1, 500) as i64], |row| {
                    Ok(AttentionChange {
                        id: row.get(0)?,
                        attention_item_id: row.get(1)?,
                        change_kind: row.get(2)?,
                        item_version: row.get(3)?,
                    })
                })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns the latest change sequence.
    pub async fn latest_change_id(&self) -> Result<i64> {
        self.db
            .reader()
            .call(|conn| {
                Ok(conn.query_row(
                    "SELECT COALESCE(MAX(id),0) FROM attention_changes",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
            .map_err(flatten_err)
    }

    /// Restores expired snoozes to open state.
    pub async fn wake_expired_snoozes(&self, now: &str) -> Result<usize> {
        let now = now.to_owned();
        self.db
            .writer()
            .call(move |conn| wake_snoozes(conn, &now).map_err(other))
            .await
            .map_err(flatten_err)
    }
}

fn other(error: impl Into<anyhow::Error>) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into().into())
}

fn read_item(conn: &Connection, id: &str) -> Result<Option<AttentionItem>> {
    conn.query_row(
        "SELECT id, project_id, task_id, task_item_id, step_id, session_id, kind, severity,
                state, title, summary, requested_decision_json, actions_json, dedupe_key,
                assignee, source_event_id, occurrence_count, reopen_count, version, created_at,
                updated_at, last_occurred_at, snoozed_until, sla_deadline, resolved_at,
                resolution_json
         FROM attention_items WHERE id=?1",
        params![id],
        |row| {
            let decision: Option<String> = row.get(11)?;
            let actions: String = row.get(12)?;
            let resolution: Option<String> = row.get(25)?;
            Ok(AttentionItem {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_id: row.get(2)?,
                task_item_id: row.get(3)?,
                step_id: row.get(4)?,
                session_id: row.get(5)?,
                kind: row.get(6)?,
                severity: row.get(7)?,
                state: row.get(8)?,
                title: row.get(9)?,
                summary: row.get(10)?,
                requested_decision: decision.and_then(|value| serde_json::from_str(&value).ok()),
                actions: serde_json::from_str(&actions).unwrap_or_default(),
                dedupe_key: row.get(13)?,
                assignee: row.get(14)?,
                source_event_id: row.get(15)?,
                occurrence_count: row.get(16)?,
                reopen_count: row.get(17)?,
                version: row.get(18)?,
                created_at: row.get(19)?,
                updated_at: row.get(20)?,
                last_occurred_at: row.get(21)?,
                snoozed_until: row.get(22)?,
                sla_deadline: row.get(23)?,
                resolved_at: row.get(24)?,
                resolution: resolution.and_then(|value| serde_json::from_str(&value).ok()),
            })
        },
    )
    .optional()
    .context("load attention item")
}

fn filter_matches(item: &AttentionItem, filter: &AttentionFilter, actor: Option<&str>) -> bool {
    filter
        .project_id
        .as_ref()
        .is_none_or(|v| &item.project_id == v)
        && filter.state.as_ref().is_none_or(|v| &item.state == v)
        && filter.kind.as_ref().is_none_or(|v| &item.kind == v)
        && filter.severity.as_ref().is_none_or(|v| &item.severity == v)
        && filter.task_id.as_ref().is_none_or(|v| &item.task_id == v)
        && filter
            .assignee
            .as_ref()
            .is_none_or(|value| match value.as_str() {
                "me" => item.assignee.as_deref() == actor,
                "unassigned" => item.assignee.is_none(),
                other => item.assignee.as_deref() == Some(other),
            })
}

fn attention_order(
    left: &AttentionItem,
    right: &AttentionItem,
    actor: Option<&str>,
) -> std::cmp::Ordering {
    let severity = |item: &AttentionItem| {
        if item.severity == "intervention" {
            0
        } else {
            1
        }
    };
    let ownership = |item: &AttentionItem| match (item.assignee.as_deref(), actor) {
        (Some(owner), Some(current)) if owner == current => 0,
        (None, _) => 1,
        (Some(_), _) => 2,
    };
    (
        severity(left),
        ownership(left),
        left.sla_deadline.as_deref().unwrap_or("~"),
        left.created_at.as_str(),
    )
        .cmp(&(
            severity(right),
            ownership(right),
            right.sla_deadline.as_deref().unwrap_or("~"),
            right.created_at.as_str(),
        ))
}

fn apply_projection_op(conn: &Connection, operation: AttentionProjectionOp) -> Result<()> {
    match operation {
        AttentionProjectionOp::Upsert(candidate) => upsert_candidate(conn, &candidate),
        AttentionProjectionOp::ResolveStep {
            task_id,
            task_item_id,
            step_id,
            source_event_id,
        } => resolve_matching(
            conn,
            &task_id,
            task_item_id.as_deref(),
            Some(&step_id),
            &source_event_id,
        ),
        AttentionProjectionOp::ResolveTask {
            task_id,
            source_event_id,
        } => resolve_matching(conn, &task_id, None, None, &source_event_id),
    }
}

fn upsert_candidate(conn: &Connection, candidate: &AttentionCandidate) -> Result<()> {
    let active_id: Option<String> = conn
        .query_row(
            "SELECT id FROM attention_items WHERE project_id=?1 AND dedupe_key=?2
         AND state IN ('open','claimed','snoozed') LIMIT 1",
            params![candidate.project_id, candidate.dedupe_key],
            |row| row.get(0),
        )
        .optional()?;
    let now = now_ts();
    let actions = serde_json::to_string(&candidate.actions)?;
    let decision = candidate
        .requested_decision
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    if let Some(id) = active_id {
        conn.execute(
            "UPDATE attention_items SET severity=?2, title=?3, summary=?4,
             requested_decision_json=?5, actions_json=?6, source_event_id=?7,
             occurrence_count=occurrence_count+1, version=version+1, updated_at=?8,
             last_occurred_at=?9, sla_deadline=?10 WHERE id=?1",
            params![
                id,
                candidate.severity.as_str(),
                candidate.title,
                candidate.summary,
                decision,
                actions,
                candidate.source_event_id,
                now,
                candidate.occurred_at,
                candidate.sla_deadline
            ],
        )?;
        append_change(conn, &id, "upsert")?;
        return Ok(());
    }
    let resolved_id: Option<String> = conn.query_row(
        "SELECT id FROM attention_items WHERE project_id=?1 AND dedupe_key=?2 AND state='resolved'
         ORDER BY resolved_at DESC LIMIT 1",
        params![candidate.project_id, candidate.dedupe_key], |row| row.get(0),
    ).optional()?;
    let id = resolved_id.unwrap_or_else(|| candidate.id.clone());
    if read_item(conn, &id)?.is_some() {
        conn.execute(
            "UPDATE attention_items SET state='open', severity=?2, title=?3, summary=?4,
             requested_decision_json=?5, actions_json=?6, assignee=NULL, source_event_id=?7,
             occurrence_count=occurrence_count+1, reopen_count=reopen_count+1, version=version+1,
             updated_at=?8, last_occurred_at=?9, snoozed_until=NULL, sla_deadline=?10,
             resolved_at=NULL, resolution_json=NULL WHERE id=?1",
            params![
                id,
                candidate.severity.as_str(),
                candidate.title,
                candidate.summary,
                decision,
                actions,
                candidate.source_event_id,
                now,
                candidate.occurred_at,
                candidate.sla_deadline
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO attention_items(id,project_id,task_id,task_item_id,step_id,session_id,
             kind,severity,state,title,summary,requested_decision_json,actions_json,dedupe_key,
             source_event_id,created_at,updated_at,last_occurred_at,sla_deadline)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'open',?9,?10,?11,?12,?13,?14,?15,?15,?16,?17)",
            params![
                id,
                candidate.project_id,
                candidate.task_id,
                candidate.task_item_id,
                candidate.step_id,
                candidate.session_id,
                candidate.kind,
                candidate.severity.as_str(),
                candidate.title,
                candidate.summary,
                decision,
                actions,
                candidate.dedupe_key,
                candidate.source_event_id,
                now,
                candidate.occurred_at,
                candidate.sla_deadline
            ],
        )?;
    }
    append_change(conn, &id, "upsert")
}

fn resolve_matching(
    conn: &Connection,
    task_id: &str,
    item_id: Option<&str>,
    step_id: Option<&str>,
    source_event_id: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id FROM attention_items WHERE task_id=?1 AND state IN ('open','claimed','snoozed')",
    )?;
    let ids = stmt
        .query_map(params![task_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for id in ids {
        let Some(item) = read_item(conn, &id)? else {
            continue;
        };
        if item_id.is_some() && item.task_item_id.as_deref() != item_id {
            continue;
        }
        if step_id.is_some() && item.step_id.as_deref() != step_id {
            continue;
        }
        let now = now_ts();
        conn.execute(
            "UPDATE attention_items SET state='resolved', assignee=NULL, version=version+1,
             updated_at=?2, resolved_at=?2, snoozed_until=NULL,
             resolution_json=?3, source_event_id=?4 WHERE id=?1",
            params![
                id,
                now,
                serde_json::json!({"reason":"condition_cleared"}).to_string(),
                source_event_id
            ],
        )?;
        append_change(conn, &id, "remove")?;
    }
    Ok(())
}

fn append_change(conn: &Connection, id: &str, kind: &str) -> Result<()> {
    let version: i64 = conn.query_row(
        "SELECT version FROM attention_items WHERE id=?1",
        params![id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO attention_changes(attention_item_id,change_kind,item_version,created_at)
         VALUES(?1,?2,?3,?4)",
        params![id, kind, version, now_ts()],
    )?;
    Ok(())
}

fn mutate_item(
    conn: &Connection,
    id: &str,
    expected_version: i64,
    key: &str,
    actor: &str,
    mutation: AttentionMutation,
) -> Result<AttentionItem> {
    let request = mutation.request_value();
    let request_json = serde_json::to_string(&request)?;
    let request_hash = digest_hex(format!("{}:{request_json}", mutation.kind()).as_bytes());
    let tx = conn.unchecked_transaction()?;
    let previous: Option<String> = tx.query_row(
        "SELECT request_hash FROM attention_actions WHERE attention_item_id=?1 AND idempotency_key=?2",
        params![id, key], |row| row.get(0),
    ).optional()?;
    if let Some(previous_hash) = previous {
        if previous_hash != request_hash {
            bail!("idempotency key was already used for a different request");
        }
        let item = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item not found"))?;
        tx.commit()?;
        return Ok(item);
    }
    let item = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item not found"))?;
    if item.version != expected_version {
        bail!(
            "attention item version conflict: expected {expected_version}, current {}",
            item.version
        );
    }
    let now = now_ts();
    let changed = match &mutation {
        AttentionMutation::Claim => tx.execute(
            "UPDATE attention_items SET state='claimed',assignee=?2,version=version+1,updated_at=?3
             WHERE id=?1 AND version=?4 AND state='open'", params![id, actor, now, expected_version],
        )?,
        AttentionMutation::Snooze { until } => tx.execute(
            "UPDATE attention_items SET state='snoozed',snoozed_until=?2,version=version+1,updated_at=?3
             WHERE id=?1 AND version=?4 AND state IN ('open','claimed')", params![id, until, now, expected_version],
        )?,
        AttentionMutation::Resolve { reason } => tx.execute(
            "UPDATE attention_items SET state='resolved',assignee=NULL,resolved_at=?2,updated_at=?2,
             snoozed_until=NULL,resolution_json=?3,version=version+1
             WHERE id=?1 AND version=?4 AND state IN ('open','claimed','snoozed')",
            params![id, now, serde_json::json!({"reason":reason,"actor":actor}).to_string(), expected_version],
        )?,
    };
    if changed != 1 {
        bail!("attention item state does not allow {}", mutation.kind());
    }
    tx.execute(
        "INSERT INTO attention_actions(attention_item_id,actor,mutation_kind,idempotency_key,
         request_hash,target_version,status,request_json,result_json,created_at,completed_at)
         VALUES(?1,?2,?3,?4,?5,?6,'succeeded',?7,'{}',?8,?8)",
        params![
            id,
            actor,
            mutation.kind(),
            key,
            request_hash,
            expected_version,
            request_json,
            now
        ],
    )?;
    append_change(
        &tx,
        id,
        if matches!(mutation, AttentionMutation::Resolve { .. }) {
            "remove"
        } else {
            "upsert"
        },
    )?;
    let result = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item disappeared"))?;
    tx.commit()?;
    Ok(result)
}

fn digest_hex(input: &[u8]) -> String {
    Sha256::digest(input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reserve_action(
    conn: &Connection,
    id: &str,
    expected_version: i64,
    key: &str,
    actor: &str,
    action_id: &str,
    input: &serde_json::Value,
) -> Result<AttentionActionReservation> {
    let request_json = serde_json::to_string(input)?;
    let request_hash = digest_hex(format!("action:{action_id}:{request_json}").as_bytes());
    let tx = conn.unchecked_transaction()?;
    let previous: Option<(String, String)> = tx
        .query_row(
            "SELECT request_hash,status FROM attention_actions
             WHERE attention_item_id=?1 AND idempotency_key=?2",
            params![id, key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((previous_hash, _status)) = previous {
        if previous_hash != request_hash {
            bail!("idempotency key was already used for a different request");
        }
        let item = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item not found"))?;
        tx.commit()?;
        return Ok(AttentionActionReservation {
            item,
            should_execute: false,
        });
    }
    let item = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item not found"))?;
    if item.version != expected_version {
        bail!(
            "attention item version conflict: expected {expected_version}, current {}",
            item.version
        );
    }
    let changed = tx.execute(
        "UPDATE attention_items SET state='claimed',assignee=?2,version=version+1,updated_at=?3
         WHERE id=?1 AND version=?4 AND state='open'",
        params![id, actor, now_ts(), expected_version],
    )?;
    if changed != 1 {
        bail!("attention item state does not allow action reservation");
    }
    tx.execute(
        "INSERT INTO attention_actions(attention_item_id,actor,mutation_kind,action_id,
         idempotency_key,request_hash,target_version,status,request_json,created_at)
         VALUES(?1,?2,'action',?3,?4,?5,?6,'started',?7,?8)",
        params![
            id,
            actor,
            action_id,
            key,
            request_hash,
            expected_version,
            request_json,
            now_ts()
        ],
    )?;
    append_change(&tx, id, "upsert")?;
    let item = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item disappeared"))?;
    tx.commit()?;
    Ok(AttentionActionReservation {
        item,
        should_execute: true,
    })
}

fn complete_action(
    conn: &Connection,
    id: &str,
    key: &str,
    actor: &str,
    action_id: &str,
    error_code: Option<&str>,
) -> Result<AttentionItem> {
    let tx = conn.unchecked_transaction()?;
    let status: String = tx.query_row(
        "SELECT status FROM attention_actions WHERE attention_item_id=?1
         AND idempotency_key=?2 AND action_id=?3",
        params![id, key, action_id],
        |row| row.get(0),
    )?;
    if status != "started" {
        let item = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item not found"))?;
        tx.commit()?;
        return Ok(item);
    }
    let now = now_ts();
    if let Some(error) = error_code {
        tx.execute(
            "UPDATE attention_actions SET status='failed',error_code=?3,completed_at=?4
             WHERE attention_item_id=?1 AND idempotency_key=?2",
            params![id, key, error, now],
        )?;
        tx.execute(
            "UPDATE attention_items SET state='open',assignee=NULL,version=version+1,updated_at=?2
             WHERE id=?1 AND state='claimed' AND assignee=?3",
            params![id, now, actor],
        )?;
        append_change(&tx, id, "upsert")?;
    } else {
        tx.execute(
            "UPDATE attention_actions SET status='succeeded',result_json='{}',completed_at=?3
             WHERE attention_item_id=?1 AND idempotency_key=?2",
            params![id, key, now],
        )?;
        tx.execute(
            "UPDATE attention_items SET state='resolved',assignee=NULL,resolved_at=?2,updated_at=?2,
             resolution_json=?3,version=version+1 WHERE id=?1 AND state='claimed' AND assignee=?4",
            params![
                id,
                now,
                serde_json::json!({"reason":format!("action:{action_id}"),"actor":actor}).to_string(),
                actor
            ],
        )?;
        append_change(&tx, id, "remove")?;
    }
    let item = read_item(&tx, id)?.ok_or_else(|| anyhow!("attention item disappeared"))?;
    tx.commit()?;
    Ok(item)
}

fn wake_snoozes(conn: &Connection, now: &str) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT id FROM attention_items WHERE state='snoozed' AND snoozed_until IS NOT NULL AND snoozed_until <= ?1",
    )?;
    let ids = stmt
        .query_map(params![now], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for id in &ids {
        conn.execute(
            "UPDATE attention_items SET state='open',assignee=NULL,snoozed_until=NULL,
             version=version+1,updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        append_change(conn, id, "upsert")?;
    }
    Ok(ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use tempfile::tempdir;

    fn candidate(id: &str) -> AttentionCandidate {
        AttentionCandidate {
            id: id.into(),
            project_id: "p".into(),
            task_id: "t".into(),
            task_item_id: Some("i".into()),
            step_id: Some("qa".into()),
            session_id: None,
            kind: "step_failed".into(),
            severity: AttentionSeverity::Intervention,
            title: "Step failed".into(),
            summary: "qa failed".into(),
            requested_decision: None,
            actions: vec![],
            dedupe_key: "step_failed:i:qa".into(),
            source_event_id: "1".into(),
            occurred_at: "2026-01-01T00:00:00Z".into(),
            sla_deadline: None,
        }
    }

    async fn repo() -> (tempfile::TempDir, AsyncAttentionRepository) {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("attention.db");
        init_schema(&path).expect("schema");
        let db = Arc::new(AsyncDatabase::open(&path).await.expect("open"));
        (temp, AsyncAttentionRepository::new(db))
    }

    #[tokio::test]
    async fn duplicate_projection_aggregates_occurrences() {
        let (_temp, repo) = repo().await;
        repo.apply_projection_batch(
            vec![AttentionProjectionOp::Upsert(Box::new(candidate("a")))],
            1,
        )
        .await
        .expect("first");
        repo.apply_projection_batch(
            vec![AttentionProjectionOp::Upsert(Box::new(candidate("a")))],
            2,
        )
        .await
        .expect("second");
        let item = repo.get("a").await.expect("get").expect("item");
        assert_eq!(item.occurrence_count, 2);
        assert_eq!(item.version, 2);
        assert_eq!(
            repo.list(
                AttentionFilter {
                    limit: 10,
                    ..Default::default()
                },
                None
            )
            .await
            .expect("list")
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn optimistic_claim_and_idempotency_are_enforced() {
        let (_temp, repo) = repo().await;
        repo.apply_projection_batch(
            vec![AttentionProjectionOp::Upsert(Box::new(candidate("a")))],
            1,
        )
        .await
        .expect("project");
        let claimed = repo
            .mutate("a", 1, "key-1", "actor-a", AttentionMutation::Claim)
            .await
            .expect("claim");
        assert_eq!(claimed.state, "claimed");
        assert!(
            repo.mutate("a", 1, "key-2", "actor-b", AttentionMutation::Claim)
                .await
                .is_err()
        );
        let replay = repo
            .mutate("a", 1, "key-1", "actor-a", AttentionMutation::Claim)
            .await
            .expect("replay");
        assert_eq!(replay.assignee.as_deref(), Some("actor-a"));
    }

    #[tokio::test]
    async fn action_reservation_is_concurrent_and_replay_safe() {
        let (_temp, repo) = repo().await;
        repo.apply_projection_batch(
            vec![AttentionProjectionOp::Upsert(Box::new(candidate("a")))],
            1,
        )
        .await
        .expect("project");
        let reservation = repo
            .reserve_action(
                "a",
                1,
                "action-key",
                "actor-a",
                "acknowledge",
                &serde_json::json!({}),
            )
            .await
            .expect("reserve");
        assert!(reservation.should_execute);
        assert!(
            repo.reserve_action(
                "a",
                1,
                "other-key",
                "actor-b",
                "acknowledge",
                &serde_json::json!({})
            )
            .await
            .is_err()
        );
        let completed = repo
            .complete_action("a", "action-key", "actor-a", "acknowledge", None)
            .await
            .expect("complete");
        assert_eq!(completed.state, "resolved");
        let replay = repo
            .reserve_action(
                "a",
                1,
                "action-key",
                "actor-a",
                "acknowledge",
                &serde_json::json!({}),
            )
            .await
            .expect("replay");
        assert!(!replay.should_execute);
        assert_eq!(replay.item.state, "resolved");
    }
}
