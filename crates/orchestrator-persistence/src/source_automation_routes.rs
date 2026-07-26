//! The source-automation tables: `source_automation_routes`,
//! `source_automation_route_generations`, `source_automation_route_attempts`
//! and `source_automation_route_changes`.
//!
//! `core::source_automation` owns the contract: what a well-formed reservation
//! input is, how the stable automation identity and the deterministic task id
//! are derived, how long a retry waits, which states release a lease, and what
//! a refused fence means to an operator. This module owns the statements and
//! the six transactions that have to stay atomic.
//!
//! Two shapes are deliberate here, and both were named by earlier batches:
//!
//! * **The frozen binding and template snapshots cross the boundary as text.**
//!   They are `serde_json::Value` above and `String` here, so a malformed
//!   snapshot is an `anyhow` error naming the route rather than a
//!   `FromSqlConversionFailure` carrying an invented column index. These were
//!   the last two such constructions in `core` (FR-130 B15).
//! * **Every fenced write reports whether its fence held and nothing more.**
//!   `Mutation::Rejected` carries the row as it actually is; deciding whether
//!   that is a version conflict, an unreplayable state or a cross-binding
//!   reroute is the caller's, because those are three different things to say
//!   to an operator and only the caller knows which it asked for.
//!
//! Several fences here exist in more than one copy, and a copy is only pinned
//! by an assertion that reaches the statement it lives in:
//!
//! | Fence | Copies |
//! |---|---|
//! | optimistic `version=?` | 3 — `replay`, `ignore`, `adopt_generation` |
//! | lease `lease_token=?2` | 2 — `transition_leased`, `suspend_leased` |
//! | `attempt_count < max_attempts` | 2 — the claim's candidate `SELECT` and its `UPDATE` |
//! | due-state allowlist | 2 — the same two claim statements |
//! | replayable-state allowlist | 2 — `replay` and `adopt_generation` |
//! | `completed_at IS NULL` | 2 — the claim's expiry closeout and `complete_open_attempt` |

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::async_database::{AsyncDatabase, flatten_err};

/// Longest list any read here will return, whatever the caller asks for.
pub const MAX_LIST_ROWS: usize = 200;

/// Most routes one claim may take, whatever the caller asks for.
pub const MAX_CLAIM_BATCH: usize = 100;

/// How many candidate rows a claim scans per route it may take. A claim skips
/// candidates whose installation is already busy, so it has to look past them.
pub const CLAIM_CANDIDATE_FACTOR: usize = 4;

/// Most rows one retention pass may delete per table.
pub const MAX_CLEANUP_ROWS: usize = 10_000;

/// Longest retention window a caller may ask for, in days.
pub const MAX_RETENTION_DAYS: u32 = 365;

/// Most failure categories one status read reports.
pub const MAX_FAILURE_CATEGORIES: usize = 32;

/// One `source_automation_routes` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAutomationRoute {
    /// Route identifier.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Stable automation identity digest.
    pub automation_key: String,
    /// First source event that reserved this route.
    pub source_event_id: String,
    /// Provider.
    pub provider: String,
    /// Installation identity.
    pub installation_id: String,
    /// Provider message identity.
    pub message_identity: String,
    /// Channel identifier.
    pub channel_id: String,
    /// Message timestamp.
    pub message_ts: String,
    /// Normalized reaction.
    pub reaction: String,
    /// Trusted role resolved for the source actor.
    pub resolved_role: String,
    /// Binding resource name.
    pub binding_name: String,
    /// Frozen binding revision.
    pub binding_revision: String,
    /// Template resource name.
    pub template_name: String,
    /// Frozen template hash.
    pub template_hash: String,
    /// Protected permalink resolution state.
    pub permalink_status: String,
    /// Protected permalink; service callers must enforce role authorization.
    pub permalink: Option<String>,
    /// Canonical audit request identifier.
    pub request_id: String,
    /// Deterministic task identity fence.
    pub deterministic_task_id: String,
    /// Canonical task, once created.
    pub task_id: Option<String>,
    /// Route lifecycle state.
    pub status: String,
    /// Stable failure code, when the route has one.
    pub error_code: Option<String>,
    /// Failure family.
    pub error_category: Option<String>,
    /// Immutable config generation.
    pub generation: i64,
    /// Optimistic concurrency version.
    pub version: i64,
    /// Attempts spent so far.
    pub attempt_count: i64,
    /// Attempt ceiling.
    pub max_attempts: i64,
    /// Earliest time the route may be claimed again.
    pub next_attempt_at: Option<String>,
    /// Current lease owner.
    pub lease_owner: Option<String>,
    /// Current fencing token.
    pub lease_token: Option<String>,
    /// Lease expiry.
    pub lease_expires_at: Option<String>,
    /// Scope that suspended this route, when one did.
    pub suspended_scope: Option<String>,
    /// Last attempt start.
    pub last_attempt_at: Option<String>,
    /// Creation time.
    pub created_at: String,
    /// Last mutation time.
    pub updated_at: String,
    /// Terminal completion time.
    pub completed_at: Option<String>,
}

/// One `source_automation_route_attempts` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAutomationRouteAttempt {
    /// Attempt identifier.
    pub id: i64,
    /// Owning route.
    pub route_id: String,
    /// Config generation the attempt ran under.
    pub generation: i64,
    /// Attempt ordinal within the generation.
    pub attempt_no: i64,
    /// Attempt start.
    pub started_at: String,
    /// Attempt completion, when it closed.
    pub completed_at: Option<String>,
    /// State the attempt resolved to.
    pub result_state: Option<String>,
    /// Stable failure code.
    pub error_code: Option<String>,
    /// Failure family.
    pub error_category: Option<String>,
    /// Provider-supplied retry hint.
    pub retry_after_seconds: Option<i64>,
}

/// One `source_automation_route_changes` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAutomationRouteChange {
    /// Monotonic change cursor.
    pub id: i64,
    /// Route the change belongs to.
    pub route_id: String,
    /// Route version at the time of the change.
    pub route_version: i64,
    /// Route state at the time of the change.
    pub state: String,
    /// Stable failure code at the time of the change.
    pub error_code: Option<String>,
    /// Change time.
    pub created_at: String,
}

/// Bounded filters for operator route queries.
#[derive(Debug, Clone, Default)]
pub struct SourceAutomationRouteFilter {
    /// Project scope.
    pub project_id: Option<String>,
    /// Lifecycle state.
    pub state: Option<String>,
    /// Provider label.
    pub provider: Option<String>,
    /// Binding resource name.
    pub binding_name: Option<String>,
    /// Canonical task.
    pub task_id: Option<String>,
    /// Keyset cursor: `(created_at, id)` of the last row already seen.
    pub before: Option<(String, String)>,
    /// Page size, clamped to [`MAX_LIST_ROWS`].
    pub limit: usize,
}

/// The six columns that together are the route's identity.
///
/// The route id is a truncation of a digest over these, so the reservation
/// re-reads the row it landed on and compares: a row whose identity columns
/// differ is not the row the caller meant to write, whatever the id says.
#[derive(Debug, Clone)]
pub struct RouteIdentity {
    /// Owning project.
    pub project_id: String,
    /// Installation identity.
    pub installation_id: String,
    /// Provider message identity.
    pub message_identity: String,
    /// Normalized reaction.
    pub reaction: String,
    /// Trusted role resolved for the source actor.
    pub resolved_role: String,
    /// Binding resource name.
    pub binding_name: String,
}

/// A route to reserve, with every identifier already derived by the caller.
#[derive(Debug, Clone)]
pub struct NewRoute {
    /// Route identifier, derived from the automation key.
    pub id: String,
    /// Stable automation identity digest.
    pub automation_key: String,
    /// Canonical audit request identifier.
    pub request_id: String,
    /// Deterministic task identity fence.
    pub deterministic_task_id: String,
    /// The columns that must match on read-back.
    pub identity: RouteIdentity,
    /// Source event that is attempting the route.
    pub source_event_id: String,
    /// Provider label.
    pub provider: String,
    /// Channel identifier.
    pub channel_id: String,
    /// Message timestamp.
    pub message_ts: String,
    /// Selected binding revision.
    pub binding_revision: String,
    /// Selected template resource name.
    pub template_name: String,
    /// Selected template content hash.
    pub template_hash: String,
    /// Frozen binding snapshot, already serialized.
    pub binding_snapshot_json: String,
    /// Frozen template snapshot, already serialized.
    pub template_snapshot_json: String,
    /// SecretStore name, never a secret value.
    pub credential_store: String,
    /// SecretStore key, never a secret value.
    pub credential_key: String,
    /// Reservation time.
    pub created_at: String,
}

/// What a reservation landed on.
#[derive(Debug, Clone)]
pub enum Reservation {
    /// The route did not exist and this call created it.
    Reserved(Box<SourceAutomationRoute>),
    /// The route already existed; the source event was linked to it.
    Existing(Box<SourceAutomationRoute>),
    /// The row under this id has a different identity. Nothing was written.
    IdentityCollision(Box<SourceAutomationRoute>),
}

/// A claim request whose window and expiry the caller has already decided.
#[derive(Debug, Clone)]
pub struct Claim {
    /// Worker taking the lease.
    pub owner: String,
    /// Most routes to take, clamped to [`MAX_CLAIM_BATCH`].
    pub limit: usize,
    /// Claim time.
    pub now: String,
    /// When the issued leases expire.
    pub lease_expires_at: String,
}

/// A lease-fenced state transition.
#[derive(Debug, Clone)]
pub struct LeaseTransition {
    /// Route to move.
    pub id: String,
    /// Fencing token the caller believes it holds.
    pub lease_token: String,
    /// State to move to.
    pub state: String,
    /// Stable failure code, when the transition carries one.
    pub error_code: Option<String>,
    /// Failure family.
    pub error_category: Option<String>,
    /// Validated permalink, when this transition resolves one.
    pub permalink: Option<String>,
    /// Canonical task, when this transition completes the route.
    pub task_id: Option<String>,
    /// Next claim time, when this transition schedules a retry.
    pub next_attempt_at: Option<String>,
    /// Provider retry hint carried onto the attempt record.
    pub retry_after_seconds: Option<u64>,
    /// Whether the target state is terminal; sets `completed_at`.
    pub terminal: bool,
    /// Whether the target state releases the lease and closes the attempt.
    pub release: bool,
    /// Transition time.
    pub now: String,
}

/// A new immutable config generation for an existing route.
#[derive(Debug, Clone)]
pub struct NewGeneration {
    /// Route to advance.
    pub route_id: String,
    /// Version the caller believes is current.
    pub expected_version: i64,
    /// Generation number to write, derived by the caller.
    pub generation: i64,
    /// Audit request identifier for the new generation, derived by the caller.
    pub request_id: String,
    /// Deterministic task identity fence, unchanged across generations.
    pub deterministic_task_id: String,
    /// Trusted current actor role.
    pub resolved_role: String,
    /// Stable binding name; the fence rejects a different one.
    pub binding_name: String,
    /// Current binding revision.
    pub binding_revision: String,
    /// Current template name.
    pub template_name: String,
    /// Current template hash.
    pub template_hash: String,
    /// Immutable current binding snapshot, already serialized.
    pub binding_snapshot_json: String,
    /// Immutable current template snapshot, already serialized.
    pub template_snapshot_json: String,
    /// Fresh credential store reference.
    pub credential_store: String,
    /// Fresh credential key reference.
    pub credential_key: String,
    /// Audit request that authorized generation adoption.
    pub created_by_request_id: String,
    /// Adoption time.
    pub now: String,
}

/// Whether a fenced mutation applied, and the row as it actually is.
#[derive(Debug, Clone)]
pub enum Mutation {
    /// The fence held; the row is as this call left it.
    Applied(Box<SourceAutomationRoute>),
    /// The fence did not hold; the row is untouched and is carried back so the
    /// caller can say which of its conditions failed.
    Rejected(Box<SourceAutomationRoute>),
    /// No such route.
    Missing,
}

/// A scope-wide suspend or resume.
#[derive(Debug, Clone)]
pub struct ScopeSuspension {
    /// Project scope.
    pub project_id: String,
    /// Installation scope, when narrowed to one.
    pub installation_id: Option<String>,
    /// Binding scope, when narrowed to one.
    pub binding_name: Option<String>,
    /// Scope label recorded on suspended rows and matched on resume.
    pub scope: String,
    /// True to suspend, false to resume.
    pub suspend: bool,
    /// Mutation time.
    pub now: String,
}

/// The frozen snapshots a reserved route executes under, still unparsed.
#[derive(Debug, Clone)]
pub struct ExecutionSnapshotRow {
    /// Binding snapshot, as stored.
    pub binding_json: String,
    /// Template snapshot, as stored.
    pub template_json: String,
    /// SecretStore name.
    pub credential_store: String,
    /// SecretStore key.
    pub credential_key: String,
}

/// Backlog and failure counts for one project, without the age arithmetic.
#[derive(Debug, Clone)]
pub struct StatusCounts {
    /// Routes still in a non-terminal state.
    pub backlog_count: u64,
    /// Creation time of the oldest such route, when there is one.
    pub oldest_created_at: Option<String>,
    /// Routes holding an unexpired lease.
    pub active_leases: u64,
    /// Routes waiting on the retry schedule.
    pub retrying_count: u64,
    /// Routes waiting on an operator.
    pub needs_attention_count: u64,
    /// Failure-family histogram over actionable and failed routes.
    pub failure_categories: BTreeMap<String, u64>,
}

const ROUTE_COLUMNS: &str = "id,project_id,automation_key,source_event_id,provider,installation_id,
     message_identity,channel_id,message_ts,reaction,resolved_role,binding_name,binding_revision,
     template_name,template_hash,permalink_status,permalink,request_id,
     deterministic_task_id,task_id,status,error_code,error_category,generation,version,
     attempt_count,max_attempts,next_attempt_at,lease_owner,lease_token,lease_expires_at,
     suspended_scope,last_attempt_at,created_at,updated_at,completed_at";

fn map_route(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceAutomationRoute> {
    Ok(SourceAutomationRoute {
        id: row.get(0)?,
        project_id: row.get(1)?,
        automation_key: row.get(2)?,
        source_event_id: row.get(3)?,
        provider: row.get(4)?,
        installation_id: row.get(5)?,
        message_identity: row.get(6)?,
        channel_id: row.get(7)?,
        message_ts: row.get(8)?,
        reaction: row.get(9)?,
        resolved_role: row.get(10)?,
        binding_name: row.get(11)?,
        binding_revision: row.get(12)?,
        template_name: row.get(13)?,
        template_hash: row.get(14)?,
        permalink_status: row.get(15)?,
        permalink: row.get(16)?,
        request_id: row.get(17)?,
        deterministic_task_id: row.get(18)?,
        task_id: row.get(19)?,
        status: row.get(20)?,
        error_code: row.get(21)?,
        error_category: row.get(22)?,
        generation: row.get(23)?,
        version: row.get(24)?,
        attempt_count: row.get(25)?,
        max_attempts: row.get(26)?,
        next_attempt_at: row.get(27)?,
        lease_owner: row.get(28)?,
        lease_token: row.get(29)?,
        lease_expires_at: row.get(30)?,
        suspended_scope: row.get(31)?,
        last_attempt_at: row.get(32)?,
        created_at: row.get(33)?,
        updated_at: row.get(34)?,
        completed_at: row.get(35)?,
    })
}

fn route_row(conn: &Connection, id: &str) -> Result<Option<SourceAutomationRoute>> {
    conn.query_row(
        &format!("SELECT {ROUTE_COLUMNS} FROM source_automation_routes WHERE id=?1"),
        [id],
        map_route,
    )
    .optional()
    .map_err(Into::into)
}

fn append_route_change(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO source_automation_route_changes
         (route_id,route_version,state,error_code,created_at)
         SELECT id,version,status,error_code,updated_at FROM source_automation_routes WHERE id=?1",
        [id],
    )?;
    Ok(())
}

fn complete_open_attempt(
    conn: &Connection,
    id: &str,
    result_state: &str,
    error_code: Option<&str>,
    error_category: Option<&str>,
    retry_after_seconds: Option<u64>,
    completed_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE source_automation_route_attempts SET completed_at=?2,result_state=?3,
         error_code=?4,error_category=?5,retry_after_seconds=?6
         WHERE id=(SELECT id FROM source_automation_route_attempts
                   WHERE route_id=?1 AND completed_at IS NULL ORDER BY id DESC LIMIT 1)",
        params![
            id,
            completed_at,
            result_state,
            error_code,
            error_category,
            retry_after_seconds.map(|value| value as i64),
        ],
    )?;
    Ok(())
}

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
}

/// Reserves the automation identity and links the source event to the route.
///
/// The link is written whether or not this call created the route: a duplicate
/// delivery still has to point at the identity it lost the race for.
pub async fn reserve(db: &AsyncDatabase, route: NewRoute) -> Result<Reservation> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<Reservation> {
                let tx = conn.unchecked_transaction()?;
                let inserted = tx.execute(
                    "INSERT OR IGNORE INTO source_automation_routes
                     (id,project_id,automation_key,source_event_id,provider,installation_id,message_identity,
                      channel_id,message_ts,reaction,resolved_role,binding_name,binding_revision,template_name,
                      template_hash,binding_snapshot_json,template_snapshot_json,credential_store,credential_key,
                      request_id,deterministic_task_id,status,created_at,updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,
                             'matched',?22,?22)",
                    params![
                        route.id,
                        route.identity.project_id,
                        route.automation_key,
                        route.source_event_id,
                        route.provider,
                        route.identity.installation_id,
                        route.identity.message_identity,
                        route.channel_id,
                        route.message_ts,
                        route.identity.reaction,
                        route.identity.resolved_role,
                        route.identity.binding_name,
                        route.binding_revision,
                        route.template_name,
                        route.template_hash,
                        route.binding_snapshot_json,
                        route.template_snapshot_json,
                        route.credential_store,
                        route.credential_key,
                        route.request_id,
                        route.deterministic_task_id,
                        route.created_at,
                    ],
                )? == 1;
                if inserted {
                    tx.execute(
                        "INSERT INTO source_automation_route_generations
                         (route_id,generation,binding_name,binding_revision,template_name,template_hash,
                          binding_snapshot_json,template_snapshot_json,credential_store,credential_key,
                          request_id,deterministic_task_id,created_at)
                         VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                        params![
                            route.id,
                            route.identity.binding_name,
                            route.binding_revision,
                            route.template_name,
                            route.template_hash,
                            route.binding_snapshot_json,
                            route.template_snapshot_json,
                            route.credential_store,
                            route.credential_key,
                            route.request_id,
                            route.deterministic_task_id,
                            route.created_at,
                        ],
                    )?;
                    append_route_change(&tx, &route.id)?;
                }
                tx.execute(
                    "UPDATE source_events SET automation_route_id=?2 WHERE id=?1",
                    params![route.source_event_id, route.id],
                )?;
                tx.execute(
                    "UPDATE source_routing_attempts SET automation_route_id=?2
                     WHERE source_event_id=?1
                       AND attempt_no=(SELECT routing_attempts FROM source_events WHERE id=?1)",
                    params![route.source_event_id, route.id],
                )?;
                let stored =
                    route_row(&tx, &route.id)?.context("reserved automation route missing")?;
                if stored.project_id != route.identity.project_id
                    || stored.installation_id != route.identity.installation_id
                    || stored.message_identity != route.identity.message_identity
                    || stored.reaction != route.identity.reaction
                    || stored.resolved_role != route.identity.resolved_role
                    || stored.binding_name != route.identity.binding_name
                {
                    // Dropping the transaction rolls back the event link too:
                    // pointing an event at a route that is not its own is worse
                    // than leaving it unlinked.
                    return Ok(Reservation::IdentityCollision(Box::new(stored)));
                }
                tx.commit()?;
                Ok(if inserted {
                    Reservation::Reserved(Box::new(stored))
                } else {
                    Reservation::Existing(Box::new(stored))
                })
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Reads one route by identifier.
pub async fn read_route(db: &AsyncDatabase, id: String) -> Result<Option<SourceAutomationRoute>> {
    db.reader()
        .call(move |conn| route_row(conn, &id).map_err(other))
        .await
        .map_err(flatten_err)
}

/// Reads the route a source event was linked to, if it was linked to one.
pub async fn read_route_for_event(
    db: &AsyncDatabase,
    source_event_id: String,
) -> Result<Option<SourceAutomationRoute>> {
    db.reader()
        .call(move |conn| {
            (|| -> Result<Option<SourceAutomationRoute>> {
                let id = conn
                    .query_row(
                        "SELECT automation_route_id FROM source_events WHERE id=?1",
                        [&source_event_id],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten();
                id.map(|id| route_row(conn, &id))
                    .transpose()
                    .map(Option::flatten)
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Reads the frozen snapshots of the route's current generation, unparsed.
pub async fn read_execution_snapshot(
    db: &AsyncDatabase,
    id: String,
) -> Result<Option<ExecutionSnapshotRow>> {
    db.reader()
        .call(move |conn| {
            conn.query_row(
                "SELECT g.binding_snapshot_json,g.template_snapshot_json,
                        g.credential_store,g.credential_key
                 FROM source_automation_routes r
                 JOIN source_automation_route_generations g
                   ON g.route_id=r.id AND g.generation=r.generation
                 WHERE r.id=?1",
                [id],
                |row| {
                    Ok(ExecutionSnapshotRow {
                        binding_json: row.get(0)?,
                        template_json: row.get(1)?,
                        credential_store: row.get(2)?,
                        credential_key: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(tokio_rusqlite::Error::from)
        })
        .await
        .map_err(flatten_err)
}

/// Claims due routes, at most one active route per installation.
///
/// An expired lease's open attempt is closed with `route_lease_expired` before
/// a new fencing token is issued, so an attempt history never contains two open
/// attempts for the same route.
pub async fn claim_due(db: &AsyncDatabase, claim: Claim) -> Result<Vec<SourceAutomationRoute>> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<Vec<SourceAutomationRoute>> {
                let want = claim.limit.clamp(1, MAX_CLAIM_BATCH);
                let tx = conn.unchecked_transaction()?;
                let candidate_ids = {
                    let mut stmt = tx.prepare(
                        "SELECT id FROM source_automation_routes
                         WHERE status IN ('matched','retrying','resolving','rendered','creating')
                           AND attempt_count < max_attempts
                           AND (next_attempt_at IS NULL OR next_attempt_at<=?1)
                           AND (lease_expires_at IS NULL OR lease_expires_at<=?1)
                         ORDER BY COALESCE(next_attempt_at,created_at),created_at,id LIMIT ?2",
                    )?;
                    stmt.query_map(
                        params![claim.now, (want * CLAIM_CANDIDATE_FACTOR) as i64],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<std::result::Result<Vec<_>, _>>()?
                };
                let mut claimed = Vec::new();
                let mut installations = HashSet::new();
                for id in candidate_ids {
                    if claimed.len() >= want {
                        break;
                    }
                    let installation: String = tx.query_row(
                        "SELECT installation_id FROM source_automation_routes WHERE id=?1",
                        [&id],
                        |row| row.get(0),
                    )?;
                    if !installations.insert(installation.clone()) {
                        continue;
                    }
                    let occupied: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM source_automation_routes
                         WHERE installation_id=?1 AND id!=?2 AND lease_token IS NOT NULL
                           AND lease_expires_at>?3)",
                        params![installation, id, claim.now],
                        |row| row.get(0),
                    )?;
                    if occupied {
                        continue;
                    }
                    tx.execute(
                        "UPDATE source_automation_route_attempts
                         SET completed_at=?2,result_state='retrying',error_code='route_lease_expired',
                             error_category='transient'
                         WHERE route_id=?1 AND completed_at IS NULL",
                        params![id, claim.now],
                    )?;
                    let token = uuid::Uuid::new_v4().to_string();
                    let changed = tx.execute(
                        "UPDATE source_automation_routes SET
                           status=CASE
                             WHEN status='creating' THEN 'creating'
                             WHEN permalink_status='resolved' THEN 'rendered'
                             ELSE 'resolving' END,
                           attempt_count=attempt_count+1,version=version+1,
                           lease_owner=?2,lease_token=?3,lease_expires_at=?4,
                           lease_claimed_at=?5,last_attempt_at=?5,next_attempt_at=NULL,
                           error_code=NULL,error_category=NULL,retry_after=NULL,updated_at=?5
                         WHERE id=?1 AND status IN ('matched','retrying','resolving','rendered','creating')
                           AND attempt_count < max_attempts
                           AND (next_attempt_at IS NULL OR next_attempt_at<=?5)
                           AND (lease_expires_at IS NULL OR lease_expires_at<=?5)",
                        params![id, claim.owner, token, claim.lease_expires_at, claim.now],
                    )?;
                    if changed != 1 {
                        continue;
                    }
                    tx.execute(
                        "INSERT INTO source_automation_route_attempts
                         (route_id,generation,attempt_no,lease_token,started_at)
                         SELECT r.id,r.generation,
                           COALESCE((SELECT MAX(a.attempt_no)
                                     FROM source_automation_route_attempts a
                                     WHERE a.route_id=r.id AND a.generation=r.generation),0)+1,
                           r.lease_token,?2
                         FROM source_automation_routes r WHERE r.id=?1",
                        params![id, claim.now],
                    )?;
                    append_route_change(&tx, &id)?;
                    claimed
                        .push(route_row(&tx, &id)?.context("claimed automation route missing")?);
                }
                tx.commit()?;
                Ok(claimed)
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Applies a lease-fenced transition, closing the open attempt when the target
/// state releases the lease. `None` means the fence did not hold: either the
/// token is stale or the route already reached a terminal state.
pub async fn transition_leased(
    db: &AsyncDatabase,
    transition: LeaseTransition,
) -> Result<Option<SourceAutomationRoute>> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<Option<SourceAutomationRoute>> {
                let tx = conn.unchecked_transaction()?;
                let changed = tx.execute(
                    "UPDATE source_automation_routes SET status=?3,version=version+1,
                     error_code=?4,error_category=?5,
                     permalink_status=CASE WHEN ?6 IS NULL THEN permalink_status ELSE 'resolved' END,
                     permalink=COALESCE(?6,permalink),task_id=COALESCE(?7,task_id),
                     next_attempt_at=?8,retry_after=?9,updated_at=?10,
                     completed_at=CASE WHEN ?11 THEN ?10 ELSE completed_at END,
                     lease_owner=CASE WHEN ?12 THEN NULL ELSE lease_owner END,
                     lease_token=CASE WHEN ?12 THEN NULL ELSE lease_token END,
                     lease_expires_at=CASE WHEN ?12 THEN NULL ELSE lease_expires_at END,
                     lease_claimed_at=CASE WHEN ?12 THEN NULL ELSE lease_claimed_at END
                     WHERE id=?1 AND lease_token=?2 AND status NOT IN ('routed','ignored')",
                    params![
                        transition.id,
                        transition.lease_token,
                        transition.state,
                        transition.error_code,
                        transition.error_category,
                        transition.permalink,
                        transition.task_id,
                        transition.next_attempt_at,
                        transition
                            .retry_after_seconds
                            .map(|value| value.to_string()),
                        transition.now,
                        transition.terminal,
                        transition.release,
                    ],
                )?;
                if changed != 1 {
                    return Ok(None);
                }
                if transition.release {
                    complete_open_attempt(
                        &tx,
                        &transition.id,
                        &transition.state,
                        transition.error_code.as_deref(),
                        transition.error_category.as_deref(),
                        transition.retry_after_seconds,
                        &transition.now,
                    )?;
                }
                append_route_change(&tx, &transition.id)?;
                let route =
                    route_row(&tx, &transition.id)?.context("automation route missing")?;
                tx.commit()?;
                Ok(Some(route))
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Releases an active lease into a suspended, non-actionable state. `None`
/// means the fencing token was stale.
pub async fn suspend_leased(
    db: &AsyncDatabase,
    id: String,
    lease_token: String,
    scope: String,
    now: String,
) -> Result<Option<SourceAutomationRoute>> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<Option<SourceAutomationRoute>> {
                let tx = conn.unchecked_transaction()?;
                let changed = tx.execute(
                    "UPDATE source_automation_routes SET status='suspended',
                     suspended_scope=?3,error_code='automation_scope_suspended',
                     error_category='policy',version=version+1,updated_at=?4,
                     next_attempt_at=NULL,lease_owner=NULL,lease_token=NULL,
                     lease_expires_at=NULL,lease_claimed_at=NULL
                     WHERE id=?1 AND lease_token=?2",
                    params![id, lease_token, scope, now],
                )?;
                if changed != 1 {
                    return Ok(None);
                }
                complete_open_attempt(
                    &tx,
                    &id,
                    "suspended",
                    Some("automation_scope_suspended"),
                    Some("policy"),
                    None,
                    &now,
                )?;
                append_route_change(&tx, &id)?;
                let route = route_row(&tx, &id)?.context("suspended route missing")?;
                tx.commit()?;
                Ok(Some(route))
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Requeues a terminal actionable route under the caller's optimistic version.
pub async fn replay(
    db: &AsyncDatabase,
    id: String,
    expected_version: i64,
    now: String,
) -> Result<Mutation> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<Mutation> {
                let tx = conn.unchecked_transaction()?;
                let changed = tx.execute(
                    "UPDATE source_automation_routes SET
                       status=CASE WHEN permalink_status='resolved' THEN 'rendered' ELSE 'matched' END,
                       version=version+1,attempt_count=0,next_attempt_at=?3,error_code=NULL,
                       error_category=NULL,retry_after=NULL,lease_owner=NULL,lease_token=NULL,
                       lease_expires_at=NULL,lease_claimed_at=NULL,suspended_scope=NULL,
                       completed_at=NULL,updated_at=?3
                     WHERE id=?1 AND version=?2 AND status IN ('needs_attention','failed')",
                    params![id, expected_version, now],
                )? == 1;
                if !changed {
                    return Ok(match route_row(&tx, &id)? {
                        Some(route) => Mutation::Rejected(Box::new(route)),
                        None => Mutation::Missing,
                    });
                }
                append_route_change(&tx, &id)?;
                let route =
                    route_row(&tx, &id)?.context("replayed automation route missing")?;
                tx.commit()?;
                Ok(Mutation::Applied(Box::new(route)))
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Deliberately abandons an actionable route under the caller's optimistic
/// version, closing its open attempt.
pub async fn ignore(
    db: &AsyncDatabase,
    id: String,
    expected_version: i64,
    now: String,
) -> Result<Mutation> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<Mutation> {
                let tx = conn.unchecked_transaction()?;
                let changed = tx.execute(
                    "UPDATE source_automation_routes SET status='ignored',version=version+1,
                     next_attempt_at=NULL,lease_owner=NULL,lease_token=NULL,lease_expires_at=NULL,
                     lease_claimed_at=NULL,suspended_scope=NULL,updated_at=?3,completed_at=?3
                     WHERE id=?1 AND version=?2
                       AND status IN ('needs_attention','failed','retrying','suspended')",
                    params![id, expected_version, now],
                )? == 1;
                if !changed {
                    return Ok(match route_row(&tx, &id)? {
                        Some(route) => Mutation::Rejected(Box::new(route)),
                        None => Mutation::Missing,
                    });
                }
                complete_open_attempt(&tx, &id, "ignored", None, None, None, &now)?;
                append_route_change(&tx, &id)?;
                let route = route_row(&tx, &id)?.context("ignored automation route missing")?;
                tx.commit()?;
                Ok(Mutation::Applied(Box::new(route)))
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Writes a new immutable config generation and points the route at it.
///
/// The `UPDATE` carries all three fences the adoption depends on — the
/// optimistic version, the replayable-state allowlist, and the stable binding
/// name — so a rejection is one write, not three reads.
pub async fn adopt_generation(db: &AsyncDatabase, input: NewGeneration) -> Result<Mutation> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<Mutation> {
                let tx = conn.unchecked_transaction()?;
                let changed = tx.execute(
                    "UPDATE source_automation_routes SET generation=?2,version=version+1,
                     resolved_role=?3,binding_revision=?4,template_name=?5,template_hash=?6,
                     binding_snapshot_json=?7,template_snapshot_json=?8,credential_store=?9,
                     credential_key=?10,request_id=?11,status=CASE
                       WHEN permalink_status='resolved' THEN 'rendered' ELSE 'matched' END,
                     attempt_count=0,next_attempt_at=?12,error_code=NULL,error_category=NULL,
                     retry_after=NULL,lease_owner=NULL,lease_token=NULL,lease_expires_at=NULL,
                     lease_claimed_at=NULL,suspended_scope=NULL,completed_at=NULL,updated_at=?12
                     WHERE id=?1 AND version=?13 AND binding_name=?14
                       AND status IN ('needs_attention','failed')",
                    params![
                        input.route_id,
                        input.generation,
                        input.resolved_role,
                        input.binding_revision,
                        input.template_name,
                        input.template_hash,
                        input.binding_snapshot_json,
                        input.template_snapshot_json,
                        input.credential_store,
                        input.credential_key,
                        input.request_id,
                        input.now,
                        input.expected_version,
                        input.binding_name,
                    ],
                )? == 1;
                if !changed {
                    return Ok(match route_row(&tx, &input.route_id)? {
                        Some(route) => Mutation::Rejected(Box::new(route)),
                        None => Mutation::Missing,
                    });
                }
                tx.execute(
                    "INSERT INTO source_automation_route_generations
                     (route_id,generation,binding_name,binding_revision,template_name,template_hash,
                      binding_snapshot_json,template_snapshot_json,credential_store,credential_key,
                      request_id,deterministic_task_id,created_by_request_id,created_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    params![
                        input.route_id,
                        input.generation,
                        input.binding_name,
                        input.binding_revision,
                        input.template_name,
                        input.template_hash,
                        input.binding_snapshot_json,
                        input.template_snapshot_json,
                        input.credential_store,
                        input.credential_key,
                        input.request_id,
                        input.deterministic_task_id,
                        input.created_by_request_id,
                        input.now,
                    ],
                )?;
                append_route_change(&tx, &input.route_id)?;
                let route =
                    route_row(&tx, &input.route_id)?.context("adopted automation route missing")?;
                tx.commit()?;
                Ok(Mutation::Applied(Box::new(route)))
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Lists routes newest-first under a keyset cursor and bounded filters.
pub async fn list_routes(
    db: &AsyncDatabase,
    filter: SourceAutomationRouteFilter,
) -> Result<Vec<SourceAutomationRoute>> {
    db.reader()
        .call(move |conn| {
            (|| -> Result<Vec<SourceAutomationRoute>> {
                let (before_at, before_id) = filter
                    .before
                    .map(|(at, id)| (Some(at), Some(id)))
                    .unwrap_or((None, None));
                let mut stmt = conn.prepare(&format!(
                    "SELECT {ROUTE_COLUMNS} FROM source_automation_routes
                     WHERE (?1 IS NULL OR project_id=?1)
                       AND (?2 IS NULL OR status=?2)
                       AND (?3 IS NULL OR provider=?3)
                       AND (?4 IS NULL OR binding_name=?4)
                       AND (?5 IS NULL OR task_id=?5)
                       AND (?6 IS NULL OR created_at<?6 OR (created_at=?6 AND id<?7))
                     ORDER BY created_at DESC,id DESC LIMIT ?8"
                ))?;
                let rows = stmt.query_map(
                    params![
                        filter.project_id,
                        filter.state,
                        filter.provider,
                        filter.binding_name,
                        filter.task_id,
                        before_at,
                        before_id,
                        filter.limit.clamp(1, MAX_LIST_ROWS) as i64,
                    ],
                    map_route,
                )?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Reads a bounded attempt history for one route, newest generation first.
pub async fn read_attempts(
    db: &AsyncDatabase,
    route_id: String,
    limit: usize,
) -> Result<Vec<SourceAutomationRouteAttempt>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id,route_id,generation,attempt_no,started_at,completed_at,
                 result_state,error_code,error_category,retry_after_seconds
                 FROM source_automation_route_attempts WHERE route_id=?1
                 ORDER BY generation DESC,attempt_no DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(
                params![route_id, limit.clamp(1, MAX_LIST_ROWS) as i64],
                |row| {
                    Ok(SourceAutomationRouteAttempt {
                        id: row.get(0)?,
                        route_id: row.get(1)?,
                        generation: row.get(2)?,
                        attempt_no: row.get(3)?,
                        started_at: row.get(4)?,
                        completed_at: row.get(5)?,
                        result_state: row.get(6)?,
                        error_code: row.get(7)?,
                        error_category: row.get(8)?,
                        retry_after_seconds: row.get(9)?,
                    })
                },
            )?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
        .map_err(flatten_err)
}

/// Reads monotonic route changes after a reconnect cursor.
pub async fn read_changes(
    db: &AsyncDatabase,
    project_id: Option<String>,
    after: i64,
    limit: usize,
) -> Result<Vec<SourceAutomationRouteChange>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id,c.route_id,c.route_version,c.state,c.error_code,c.created_at
                 FROM source_automation_route_changes c
                 JOIN source_automation_routes r ON r.id=c.route_id
                 WHERE c.id>?1 AND (?2 IS NULL OR r.project_id=?2)
                 ORDER BY c.id LIMIT ?3",
            )?;
            let rows = stmt.query_map(
                params![
                    after.max(0),
                    project_id,
                    limit.clamp(1, MAX_LIST_ROWS) as i64
                ],
                |row| {
                    Ok(SourceAutomationRouteChange {
                        id: row.get(0)?,
                        route_id: row.get(1)?,
                        route_version: row.get(2)?,
                        state: row.get(3)?,
                        error_code: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
        .map_err(flatten_err)
}

/// Reads backlog and failure counts for one project. The age of the oldest
/// backlogged route is arithmetic against a clock, so it is left to the caller.
pub async fn read_status_counts(
    db: &AsyncDatabase,
    project_id: String,
    now: String,
) -> Result<StatusCounts> {
    db.reader()
        .call(move |conn| {
            (|| -> Result<StatusCounts> {
                let (
                    backlog_count,
                    oldest_created_at,
                    active_leases,
                    retrying_count,
                    needs_attention_count,
                ): (u64, Option<String>, u64, u64, u64) = conn.query_row(
                    "SELECT
                       SUM(CASE WHEN status IN ('matched','resolving','rendered','creating','retrying','suspended') THEN 1 ELSE 0 END),
                       MIN(CASE WHEN status IN ('matched','resolving','rendered','creating','retrying','suspended') THEN created_at END),
                       SUM(CASE WHEN lease_token IS NOT NULL AND lease_expires_at>?2 THEN 1 ELSE 0 END),
                       SUM(CASE WHEN status='retrying' THEN 1 ELSE 0 END),
                       SUM(CASE WHEN status='needs_attention' THEN 1 ELSE 0 END)
                     FROM source_automation_routes WHERE project_id=?1",
                    params![project_id, now],
                    |row| {
                        Ok((
                            row.get::<_, Option<u64>>(0)?.unwrap_or_default(),
                            row.get(1)?,
                            row.get::<_, Option<u64>>(2)?.unwrap_or_default(),
                            row.get::<_, Option<u64>>(3)?.unwrap_or_default(),
                            row.get::<_, Option<u64>>(4)?.unwrap_or_default(),
                        ))
                    },
                )?;
                let mut stmt = conn.prepare(&format!(
                    "SELECT COALESCE(error_category,'unknown'),COUNT(*)
                     FROM source_automation_routes WHERE project_id=?1
                       AND status IN ('needs_attention','failed')
                     GROUP BY COALESCE(error_category,'unknown')
                     ORDER BY 1 LIMIT {MAX_FAILURE_CATEGORIES}"
                ))?;
                let failure_categories = stmt
                    .query_map([&project_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                    })?
                    .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
                Ok(StatusCounts {
                    backlog_count,
                    oldest_created_at,
                    active_leases,
                    retrying_count,
                    needs_attention_count,
                    failure_categories,
                })
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Suspends or resumes every unleased route in a scope, returning how many
/// rows moved. Leased routes are left alone: they finish their bounded
/// transition and observe the suspension at the next claim.
pub async fn set_scope_suspended(db: &AsyncDatabase, scope: ScopeSuspension) -> Result<usize> {
    db.writer()
        .call(move |conn| {
            (|| -> Result<usize> {
                let tx = conn.unchecked_transaction()?;
                let ids = {
                    let mut stmt = tx.prepare(
                        "SELECT id FROM source_automation_routes
                         WHERE project_id=?1 AND (?2 IS NULL OR installation_id=?2)
                           AND (?3 IS NULL OR binding_name=?3)
                           AND lease_token IS NULL
                           AND ((?5 AND status IN ('matched','retrying','resolving','rendered','creating'))
                                OR (NOT ?5 AND status='suspended' AND suspended_scope=?4))",
                    )?;
                    stmt.query_map(
                        params![
                            scope.project_id,
                            scope.installation_id,
                            scope.binding_name,
                            scope.scope,
                            scope.suspend
                        ],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<std::result::Result<Vec<_>, _>>()?
                };
                for id in &ids {
                    if scope.suspend {
                        tx.execute(
                            "UPDATE source_automation_routes SET status='suspended',
                             suspended_scope=?2,version=version+1,updated_at=?3 WHERE id=?1",
                            params![id, scope.scope, scope.now],
                        )?;
                    } else {
                        tx.execute(
                            "UPDATE source_automation_routes SET
                             status=CASE WHEN permalink_status='resolved' THEN 'rendered' ELSE 'matched' END,
                             suspended_scope=NULL,next_attempt_at=?2,version=version+1,updated_at=?2
                             WHERE id=?1",
                            params![id, scope.now],
                        )?;
                    }
                    append_route_change(&tx, id)?;
                }
                tx.commit()?;
                Ok(ids.len())
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Drops per-attempt metadata and permalinks past the retention window while
/// keeping route, task and audit provenance. Returns how many rows it touched.
pub async fn cleanup_metadata(
    db: &AsyncDatabase,
    retention_days: u32,
    limit: usize,
    now: String,
) -> Result<u64> {
    let days = retention_days.clamp(1, MAX_RETENTION_DAYS);
    let limit = limit.clamp(1, MAX_CLEANUP_ROWS);
    db.writer()
        .call(move |conn| {
            (|| -> Result<u64> {
                let tx = conn.unchecked_transaction()?;
                let attempts = tx.execute(
                    &format!(
                        "DELETE FROM source_automation_route_attempts WHERE id IN (
                         SELECT a.id FROM source_automation_route_attempts a
                         JOIN source_automation_routes r ON r.id=a.route_id
                         WHERE datetime(a.completed_at) < datetime('now','-{days} days')
                           AND r.status IN ('routed','ignored','failed') LIMIT {limit})"
                    ),
                    [],
                )?;
                let changes = tx.execute(
                    &format!(
                        "DELETE FROM source_automation_route_changes WHERE id IN (
                         SELECT c.id FROM source_automation_route_changes c
                         JOIN source_automation_routes r ON r.id=c.route_id
                         WHERE datetime(c.created_at) < datetime('now','-{days} days')
                           AND r.status IN ('routed','ignored','failed') LIMIT {limit})"
                    ),
                    [],
                )?;
                let permalink_ids = {
                    let mut stmt = tx.prepare(&format!(
                        "SELECT id FROM source_automation_routes
                         WHERE permalink IS NOT NULL AND status IN ('routed','ignored','failed')
                           AND datetime(completed_at) < datetime('now','-{days} days') LIMIT {limit}"
                    ))?;
                    stmt.query_map([], |row| row.get::<_, String>(0))?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                for id in &permalink_ids {
                    tx.execute(
                        "UPDATE source_automation_routes SET permalink=NULL,
                         permalink_status='expired',version=version+1,updated_at=?2 WHERE id=?1",
                        params![id, now],
                    )?;
                    append_route_change(&tx, id)?;
                }
                tx.commit()?;
                Ok((attempts + changes + permalink_ids.len()) as u64)
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}
