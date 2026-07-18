//! Durable source automation reservation and provenance.

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

/// Immutable input captured before any provider call or task mutation.
#[derive(Debug, Clone)]
pub struct ReserveSourceAutomationRoute {
    /// Owning project.
    pub project_id: String,
    /// Source event currently attempting the route.
    pub source_event_id: String,
    /// Provider name.
    pub provider: String,
    /// Provider installation identity.
    pub installation_id: String,
    /// Stable provider message identity.
    pub message_identity: String,
    /// Slack channel identifier.
    pub channel_id: String,
    /// Slack message timestamp.
    pub message_ts: String,
    /// Normalized reaction name.
    pub reaction: String,
    /// Trusted role resolved by binding selection.
    pub resolved_role: String,
    /// Stable binding resource name (not revision).
    pub binding_name: String,
    /// Selected binding content revision.
    pub binding_revision: String,
    /// Selected template resource name.
    pub template_name: String,
    /// Selected template content hash.
    pub template_hash: String,
    /// Internal immutable binding snapshot.
    pub binding_snapshot: serde_json::Value,
    /// Internal immutable template snapshot.
    pub template_snapshot: serde_json::Value,
    /// SecretStore name, never a secret value.
    pub credential_store: String,
    /// SecretStore key, never a secret value.
    pub credential_key: String,
}

/// Explicit current-config adoption for a failed route. The caller must have
/// re-run the current matcher and may only adopt the same stable binding.
#[derive(Debug, Clone)]
pub struct AdoptSourceAutomationGeneration {
    /// Route to advance.
    pub route_id: String,
    /// Optimistic current route version.
    pub expected_version: i64,
    /// Trusted current actor role.
    pub resolved_role: String,
    /// Stable binding name; cross-binding reroute is rejected.
    pub binding_name: String,
    /// Current binding revision.
    pub binding_revision: String,
    /// Current template name.
    pub template_name: String,
    /// Current template hash.
    pub template_hash: String,
    /// Immutable current binding snapshot.
    pub binding_snapshot: serde_json::Value,
    /// Immutable current template snapshot.
    pub template_snapshot: serde_json::Value,
    /// Fresh credential reference.
    pub credential_store: String,
    /// Fresh credential key.
    pub credential_key: String,
    /// Audit request that authorized generation adoption.
    pub created_by_request_id: String,
}

/// Durable route projection safe for trusted service-layer use.
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
    /// Deterministic task identifier reserved before task creation.
    pub deterministic_task_id: String,
    /// Created task identifier.
    pub task_id: Option<String>,
    /// Route lifecycle state.
    pub status: String,
    /// Stable error code.
    pub error_code: Option<String>,
    /// Closed operational error family.
    pub error_category: Option<String>,
    /// Frozen configuration generation currently used by the route.
    pub generation: i64,
    /// Optimistic route version incremented on every durable transition.
    pub version: i64,
    /// Number of claimed execution attempts in the current generation.
    pub attempt_count: i64,
    /// Bounded attempt budget pinned to the route.
    pub max_attempts: i64,
    /// Earliest time at which a retry may be claimed.
    pub next_attempt_at: Option<String>,
    /// Active lease owner, when claimed by a worker.
    pub lease_owner: Option<String>,
    /// Opaque fencing token for the active lease.
    pub lease_token: Option<String>,
    /// Active lease expiry.
    pub lease_expires_at: Option<String>,
    /// Suspension scope that paused this route.
    pub suspended_scope: Option<String>,
    /// Most recent attempt start time.
    pub last_attempt_at: Option<String>,
    /// Route creation timestamp.
    pub created_at: String,
    /// Last transition timestamp.
    pub updated_at: String,
    /// Route completion timestamp.
    pub completed_at: Option<String>,
}

/// One bounded execution attempt. This projection never contains provider bodies,
/// rendered goals, permalinks, or credential values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAutomationRouteAttempt {
    /// Monotonic attempt row identifier.
    pub id: i64,
    /// Parent route.
    pub route_id: String,
    /// Frozen configuration generation.
    pub generation: i64,
    /// Attempt number within the generation.
    pub attempt_no: i64,
    /// Attempt start.
    pub started_at: String,
    /// Attempt completion.
    pub completed_at: Option<String>,
    /// Resulting route state.
    pub result_state: Option<String>,
    /// Stable safe error code.
    pub error_code: Option<String>,
    /// Closed error family.
    pub error_category: Option<String>,
    /// Bounded provider retry hint.
    pub retry_after_seconds: Option<i64>,
}

/// Monotonic route transition used by reconnectable watch clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAutomationRouteChange {
    /// Global change cursor.
    pub id: i64,
    /// Changed route.
    pub route_id: String,
    /// Route version after the transition.
    pub route_version: i64,
    /// Resulting state.
    pub state: String,
    /// Stable safe error code.
    pub error_code: Option<String>,
    /// Transition time.
    pub created_at: String,
}

/// Bounded filters for operator route queries.
#[derive(Debug, Clone, Default)]
pub struct SourceAutomationRouteFilter {
    /// Required project scope when supplied by a project-scoped client.
    pub project_id: Option<String>,
    /// Exact state.
    pub state: Option<String>,
    /// Exact provider.
    pub provider: Option<String>,
    /// Exact binding resource name.
    pub binding_name: Option<String>,
    /// Exact canonical task.
    pub task_id: Option<String>,
    /// Exclusive keyset cursor `(created_at,id)`.
    pub before: Option<(String, String)>,
    /// Requested page size.
    pub limit: usize,
}

/// Privacy-safe route worker status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAutomationStatus {
    /// Project scope.
    pub project_id: String,
    /// Routes that are due or scheduled for retry.
    pub backlog_count: u64,
    /// Age in seconds of the oldest non-terminal route.
    pub oldest_age_seconds: u64,
    /// Routes with active unexpired leases.
    pub active_leases: u64,
    /// Routes currently waiting for retry.
    pub retrying_count: u64,
    /// Routes waiting for human action.
    pub needs_attention_count: u64,
    /// Stable low-cardinality failure-family counts.
    pub failure_categories: BTreeMap<String, u64>,
}

/// Result of reserving an automation identity.
#[derive(Debug, Clone)]
pub struct SourceAutomationReservation {
    /// Durable route.
    pub route: SourceAutomationRoute,
    /// True only for the worker that owns mutation execution.
    pub should_execute: bool,
}

/// Result of one optimistic replay or ignore mutation.
#[derive(Debug, Clone)]
pub struct SourceAutomationMutationResult {
    /// Route after the mutation.
    pub route: SourceAutomationRoute,
    /// Whether this caller executed the transition rather than observing an
    /// idempotent duplicate.
    pub changed: bool,
}

/// Safe input for a pure matcher + renderer simulation.
#[derive(Debug, Clone)]
pub struct SourceAutomationSimulationInput {
    /// Project configuration scope.
    pub project_id: String,
    /// Authenticated provider evidence to simulate.
    pub match_input: crate::source_task_binding::SourceTaskBindingMatchInput,
    /// Caller-supplied sample message URL. It is never resolved over the
    /// network by simulation.
    pub message_url: String,
    /// Optional sample event ID.
    pub event_id: Option<String>,
    /// Stable provider-neutral target ID.
    pub target_id: String,
}

/// Pure simulation output shared with the live match/render primitives.
#[derive(Debug, Clone)]
pub struct SourceAutomationSimulation {
    /// Deterministic binding result.
    pub match_result: crate::source_task_binding::SourceTaskBindingMatchResult,
    /// Rendered task plan only when exactly one binding matched.
    pub rendered: Option<crate::source_task_template::RenderedSourceTaskTemplate>,
}

/// Runs the exact matcher and renderer used by live routing without reading a
/// secret, contacting a provider, or mutating durable state.
pub fn simulate_source_automation(
    config: &crate::config::OrchestratorConfig,
    input: &SourceAutomationSimulationInput,
) -> Result<SourceAutomationSimulation> {
    let match_result = crate::source_task_binding::match_source_task_binding(
        config,
        &input.project_id,
        &input.match_input,
    )?;
    let rendered = match_result
        .template_ref
        .as_deref()
        .filter(|_| match_result.status == "matched")
        .map(|template| {
            crate::source_task_template::render_source_task_template_from_config(
                config,
                &input.project_id,
                template,
                &crate::source_task_template::SourceTaskTemplateRenderInput {
                    provider: input.match_input.provider.clone(),
                    installation_id: input.match_input.installation_id.clone(),
                    message_url: input.message_url.clone(),
                    event_id: input.event_id.clone(),
                    reaction: Some(input.match_input.reaction.clone()),
                    target_id: Some(input.target_id.clone()),
                    installation_verified: false,
                },
            )
        })
        .transpose()?;
    Ok(SourceAutomationSimulation {
        match_result,
        rendered,
    })
}

/// Async route repository.
#[derive(Clone)]
pub struct AsyncSourceAutomationRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncSourceAutomationRepository {
    /// Creates a repository over shared database connections.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Reserves a stable automation identity and links the active route attempt.
    pub async fn reserve(
        &self,
        input: ReserveSourceAutomationRoute,
    ) -> Result<SourceAutomationReservation> {
        self.db
            .writer()
            .call(move |conn| reserve(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Loads a route by route ID.
    pub async fn get(&self, id: &str) -> Result<Option<SourceAutomationRoute>> {
        let id = id.to_owned();
        self.db
            .reader()
            .call(move |conn| read_route(conn, &id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Loads the route linked to a source event.
    pub async fn get_for_event(
        &self,
        source_event_id: &str,
    ) -> Result<Option<SourceAutomationRoute>> {
        let source_event_id = source_event_id.to_owned();
        self.db
            .reader()
            .call(move |conn| {
                let id = conn
                    .query_row(
                        "SELECT automation_route_id FROM source_events WHERE id=?1",
                        [&source_event_id],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten();
                id.map(|id| read_route(conn, &id))
                    .transpose()
                    .map(Option::flatten)
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns the frozen internal snapshots and credential reference.
    pub async fn execution_snapshot(
        &self,
        id: &str,
    ) -> Result<Option<SourceAutomationExecutionSnapshot>> {
        let id = id.to_owned();
        self.db
            .reader()
            .call(move |conn| read_execution_snapshot(conn, &id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Atomically claims due routes with one active route per installation.
    /// Expired attempts are closed with a stable lease-expiry result before a
    /// new fencing token is issued.
    pub async fn claim_due(
        &self,
        owner: &str,
        limit: usize,
        now: DateTime<Utc>,
        lease_seconds: i64,
    ) -> Result<Vec<SourceAutomationRoute>> {
        if owner.trim().is_empty() || owner.len() > 128 {
            bail!("route lease owner must contain 1-128 characters");
        }
        let owner = owner.to_owned();
        let now = now.to_rfc3339();
        let lease_expires = (DateTime::parse_from_rfc3339(&now)?.with_timezone(&Utc)
            + Duration::seconds(lease_seconds.clamp(15, 300)))
        .to_rfc3339();
        self.db
            .writer()
            .call(move |conn| {
                (|| -> Result<Vec<SourceAutomationRoute>> {
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
                        stmt.query_map(params![now, (limit.clamp(1, 100) * 4) as i64], |row| {
                            row.get::<_, String>(0)
                        })?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                    };
                    let mut claimed = Vec::new();
                    let mut installations = HashSet::new();
                    for id in candidate_ids {
                        if claimed.len() >= limit.clamp(1, 100) {
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
                            params![installation, id, now],
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
                            params![id, now],
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
                            params![id, owner, token, lease_expires, now],
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
                            params![id, now],
                        )?;
                        append_route_change(&tx, &id)?;
                        claimed.push(
                            read_route(&tx, &id)?.context("claimed automation route missing")?,
                        );
                    }
                    tx.commit()?;
                    Ok(claimed)
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Stores a validated permalink and advances the leased route to rendering.
    pub async fn record_permalink(
        &self,
        id: &str,
        lease_token: &str,
        permalink: &str,
    ) -> Result<SourceAutomationRoute> {
        self.transition_with_lease(
            id,
            lease_token,
            "rendered",
            None,
            None,
            Some(permalink),
            None,
            None,
        )
        .await
    }

    /// Marks that the canonical audit/task mutation boundary has been reached.
    pub async fn mark_creating(
        &self,
        id: &str,
        lease_token: &str,
    ) -> Result<SourceAutomationRoute> {
        self.transition_with_lease(id, lease_token, "creating", None, None, None, None, None)
            .await
    }

    /// Completes a leased route with its canonical task.
    pub async fn complete(
        &self,
        id: &str,
        lease_token: &str,
        task_id: &str,
    ) -> Result<SourceAutomationRoute> {
        self.transition_with_lease(
            id,
            lease_token,
            "routed",
            None,
            None,
            None,
            Some(task_id),
            None,
        )
        .await
    }

    /// Releases a transient failure onto the durable retry schedule.
    pub async fn schedule_retry(
        &self,
        id: &str,
        lease_token: &str,
        error_code: &str,
        error_category: &str,
        now: DateTime<Utc>,
        retry_after_seconds: Option<u64>,
    ) -> Result<SourceAutomationRoute> {
        let current = self.get(id).await?.context("automation route missing")?;
        let delay = retry_delay_seconds(&current.id, current.attempt_count, retry_after_seconds);
        let next = (now + Duration::seconds(delay as i64)).to_rfc3339();
        self.transition_with_lease(
            id,
            lease_token,
            "retrying",
            Some(error_code),
            Some(error_category),
            None,
            None,
            Some((next, retry_after_seconds)),
        )
        .await
    }

    /// Moves an operator-fixable leased route into Attention.
    pub async fn needs_attention(
        &self,
        id: &str,
        lease_token: &str,
        error_code: &str,
        error_category: &str,
    ) -> Result<SourceAutomationRoute> {
        self.transition_with_lease(
            id,
            lease_token,
            "needs_attention",
            Some(error_code),
            Some(error_category),
            None,
            None,
            None,
        )
        .await
    }

    /// Releases an active lease into a non-actionable suspended state.
    pub async fn suspend_leased(
        &self,
        id: &str,
        lease_token: &str,
        scope: &str,
    ) -> Result<SourceAutomationRoute> {
        let id = id.to_owned();
        let lease_token = lease_token.to_owned();
        let scope = scope.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                (|| -> Result<SourceAutomationRoute> {
                    let tx = conn.unchecked_transaction()?;
                    let now = now_ts();
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
                        bail!("automation route lease is stale");
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
                    let route = read_route(&tx, &id)?.context("suspended route missing")?;
                    tx.commit()?;
                    Ok(route)
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Moves a non-actionable invariant failure to a stable terminal state.
    pub async fn fail_terminal(
        &self,
        id: &str,
        lease_token: &str,
        error_code: &str,
        error_category: &str,
    ) -> Result<SourceAutomationRoute> {
        self.transition_with_lease(
            id,
            lease_token,
            "failed",
            Some(error_code),
            Some(error_category),
            None,
            None,
            None,
        )
        .await
    }

    /// Requeues a terminal actionable route using optimistic concurrency.
    pub async fn replay(
        &self,
        id: &str,
        expected_version: i64,
    ) -> Result<SourceAutomationMutationResult> {
        let id = id.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                (|| -> Result<SourceAutomationMutationResult> {
                    let tx = conn.unchecked_transaction()?;
                    let now = now_ts();
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
                        let current = read_route(&tx, &id)?.context("automation route missing")?;
                        if current.version != expected_version {
                            bail!(
                                "automation route version conflict: expected {expected_version}, current {}",
                                current.version
                            );
                        }
                        bail!("automation route is not replayable");
                    }
                    append_route_change(&tx, &id)?;
                    let route = read_route(&tx, &id)?.context("replayed automation route missing")?;
                    tx.commit()?;
                    Ok(SourceAutomationMutationResult { route, changed })
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Creates a new immutable generation after explicit current-config
    /// preview/adoption. The stable automation identity and deterministic task
    /// fence remain unchanged.
    pub async fn adopt_generation(
        &self,
        input: AdoptSourceAutomationGeneration,
    ) -> Result<SourceAutomationMutationResult> {
        self.db
            .writer()
            .call(move |conn| {
                (|| -> Result<SourceAutomationMutationResult> {
                    let tx = conn.unchecked_transaction()?;
                    let current = read_route(&tx, &input.route_id)?
                        .context("automation route missing")?;
                    if current.version != input.expected_version {
                        bail!(
                            "automation route version conflict: expected {}, current {}",
                            input.expected_version,
                            current.version
                        );
                    }
                    if !matches!(current.status.as_str(), "needs_attention" | "failed") {
                        bail!("automation route is not replayable");
                    }
                    if current.binding_name != input.binding_name {
                        bail!("current config selects a different binding; cross-binding reroute is denied");
                    }
                    let generation = current.generation + 1;
                    let request_id = format!(
                        "req-source-auto-{}-g{generation}",
                        &current.automation_key[..24]
                    );
                    let binding_snapshot = serde_json::to_string(&input.binding_snapshot)?;
                    let template_snapshot = serde_json::to_string(&input.template_snapshot)?;
                    let now = now_ts();
                    tx.execute(
                        "INSERT INTO source_automation_route_generations
                         (route_id,generation,binding_name,binding_revision,template_name,template_hash,
                          binding_snapshot_json,template_snapshot_json,credential_store,credential_key,
                          request_id,deterministic_task_id,created_by_request_id,created_at)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                        params![
                            input.route_id,
                            generation,
                            input.binding_name,
                            input.binding_revision,
                            input.template_name,
                            input.template_hash,
                            binding_snapshot,
                            template_snapshot,
                            input.credential_store,
                            input.credential_key,
                            request_id,
                            current.deterministic_task_id,
                            input.created_by_request_id,
                            now,
                        ],
                    )?;
                    tx.execute(
                        "UPDATE source_automation_routes SET generation=?2,version=version+1,
                         resolved_role=?3,binding_revision=?4,template_name=?5,template_hash=?6,
                         binding_snapshot_json=?7,template_snapshot_json=?8,credential_store=?9,
                         credential_key=?10,request_id=?11,status=CASE
                           WHEN permalink_status='resolved' THEN 'rendered' ELSE 'matched' END,
                         attempt_count=0,next_attempt_at=?12,error_code=NULL,error_category=NULL,
                         retry_after=NULL,lease_owner=NULL,lease_token=NULL,lease_expires_at=NULL,
                         lease_claimed_at=NULL,suspended_scope=NULL,completed_at=NULL,updated_at=?12
                         WHERE id=?1 AND version=?13",
                        params![
                            input.route_id,
                            generation,
                            input.resolved_role,
                            input.binding_revision,
                            input.template_name,
                            input.template_hash,
                            binding_snapshot,
                            template_snapshot,
                            input.credential_store,
                            input.credential_key,
                            request_id,
                            now,
                            input.expected_version,
                        ],
                    )?;
                    append_route_change(&tx, &input.route_id)?;
                    let route = read_route(&tx, &input.route_id)?
                        .context("adopted automation route missing")?;
                    tx.commit()?;
                    Ok(SourceAutomationMutationResult {
                        route,
                        changed: true,
                    })
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Deliberately ignores an actionable route using optimistic concurrency.
    pub async fn ignore(
        &self,
        id: &str,
        expected_version: i64,
    ) -> Result<SourceAutomationMutationResult> {
        let id = id.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                (|| -> Result<SourceAutomationMutationResult> {
                    let tx = conn.unchecked_transaction()?;
                    let now = now_ts();
                    let changed = tx.execute(
                        "UPDATE source_automation_routes SET status='ignored',version=version+1,
                         next_attempt_at=NULL,lease_owner=NULL,lease_token=NULL,lease_expires_at=NULL,
                         lease_claimed_at=NULL,suspended_scope=NULL,updated_at=?3,completed_at=?3
                         WHERE id=?1 AND version=?2 AND status IN ('needs_attention','failed','retrying','suspended')",
                        params![id, expected_version, now],
                    )? == 1;
                    if !changed {
                        let current = read_route(&tx, &id)?.context("automation route missing")?;
                        if current.version != expected_version {
                            bail!(
                                "automation route version conflict: expected {expected_version}, current {}",
                                current.version
                            );
                        }
                        bail!("automation route cannot be ignored from its current state");
                    }
                    complete_open_attempt(&tx, &id, "ignored", None, None, None, &now)?;
                    append_route_change(&tx, &id)?;
                    let route = read_route(&tx, &id)?.context("ignored automation route missing")?;
                    tx.commit()?;
                    Ok(SourceAutomationMutationResult { route, changed })
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    #[allow(clippy::too_many_arguments)]
    async fn transition_with_lease(
        &self,
        id: &str,
        lease_token: &str,
        state: &str,
        error_code: Option<&str>,
        error_category: Option<&str>,
        permalink: Option<&str>,
        task_id: Option<&str>,
        retry: Option<(String, Option<u64>)>,
    ) -> Result<SourceAutomationRoute> {
        let id = id.to_owned();
        let lease_token = lease_token.to_owned();
        let state = state.to_owned();
        let error_code = error_code.map(str::to_owned);
        let error_category = error_category.map(str::to_owned);
        let permalink = permalink.map(str::to_owned);
        let task_id = task_id.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                (|| -> Result<SourceAutomationRoute> {
                    let tx = conn.unchecked_transaction()?;
                    let now = now_ts();
                    let terminal = matches!(
                        state.as_str(),
                        "routed" | "needs_attention" | "ignored" | "failed"
                    );
                    let release = terminal || matches!(state.as_str(), "retrying" | "suspended");
                    let (next_attempt_at, retry_after_seconds) = retry
                        .map(|(next, hint)| (Some(next), hint))
                        .unwrap_or((None, None));
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
                            id,
                            lease_token,
                            state,
                            error_code,
                            error_category,
                            permalink,
                            task_id,
                            next_attempt_at,
                            retry_after_seconds.map(|value| value.to_string()),
                            now,
                            terminal,
                            release,
                        ],
                    )?;
                    if changed != 1 {
                        bail!("automation route lease is stale or route is terminal");
                    }
                    if release {
                        complete_open_attempt(
                            &tx,
                            &id,
                            &state,
                            error_code.as_deref(),
                            error_category.as_deref(),
                            retry_after_seconds,
                            &now,
                        )?;
                    }
                    append_route_change(&tx, &id)?;
                    let route = read_route(&tx, &id)?.context("automation route missing")?;
                    tx.commit()?;
                    Ok(route)
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists routes with stable keyset pagination and bounded filters.
    pub async fn list(
        &self,
        filter: SourceAutomationRouteFilter,
    ) -> Result<Vec<SourceAutomationRoute>> {
        self.db
            .reader()
            .call(move |conn| {
                (|| -> Result<Vec<SourceAutomationRoute>> {
                    let (before_at, before_id) = filter
                        .before
                        .map(|(at, id)| (Some(at), Some(id)))
                        .unwrap_or((None, None));
                    let mut stmt = conn.prepare(
                        "SELECT id FROM source_automation_routes
                         WHERE (?1 IS NULL OR project_id=?1)
                           AND (?2 IS NULL OR status=?2)
                           AND (?3 IS NULL OR provider=?3)
                           AND (?4 IS NULL OR binding_name=?4)
                           AND (?5 IS NULL OR task_id=?5)
                           AND (?6 IS NULL OR created_at<?6 OR (created_at=?6 AND id<?7))
                         ORDER BY created_at DESC,id DESC LIMIT ?8",
                    )?;
                    let ids = stmt
                        .query_map(
                            params![
                                filter.project_id,
                                filter.state,
                                filter.provider,
                                filter.binding_name,
                                filter.task_id,
                                before_at,
                                before_id,
                                filter.limit.clamp(1, 200) as i64,
                            ],
                            |row| row.get::<_, String>(0),
                        )?
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    ids.into_iter()
                        .map(|id| read_route(conn, &id)?.context("automation route missing"))
                        .collect()
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists a bounded attempt history for one route.
    pub async fn attempts(
        &self,
        route_id: &str,
        limit: usize,
    ) -> Result<Vec<SourceAutomationRouteAttempt>> {
        let route_id = route_id.to_owned();
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id,route_id,generation,attempt_no,started_at,completed_at,
                     result_state,error_code,error_category,retry_after_seconds
                     FROM source_automation_route_attempts WHERE route_id=?1
                     ORDER BY generation DESC,attempt_no DESC LIMIT ?2",
                )?;
                let rows =
                    stmt.query_map(params![route_id, limit.clamp(1, 200) as i64], |row| {
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
                    })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await
            .map_err(flatten_err)
    }

    /// Reads monotonic route changes after a reconnect cursor.
    pub async fn changes_since(
        &self,
        project_id: Option<&str>,
        after: i64,
        limit: usize,
    ) -> Result<Vec<SourceAutomationRouteChange>> {
        let project_id = project_id.map(str::to_owned);
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT c.id,c.route_id,c.route_version,c.state,c.error_code,c.created_at
                     FROM source_automation_route_changes c
                     JOIN source_automation_routes r ON r.id=c.route_id
                     WHERE c.id>?1 AND (?2 IS NULL OR r.project_id=?2)
                     ORDER BY c.id LIMIT ?3",
                )?;
                let rows = stmt.query_map(
                    params![after.max(0), project_id, limit.clamp(1, 200) as i64],
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

    /// Returns privacy-safe worker backlog and failure-family health.
    pub async fn status(
        &self,
        project_id: &str,
        now: DateTime<Utc>,
    ) -> Result<SourceAutomationStatus> {
        let project_id = project_id.to_owned();
        self.db
            .reader()
            .call(move |conn| {
                (|| -> Result<SourceAutomationStatus> {
                    let (backlog_count, oldest, active_leases, retrying_count, needs_attention_count): (
                        u64,
                        Option<String>,
                        u64,
                        u64,
                        u64,
                    ) = conn.query_row(
                        "SELECT
                           SUM(CASE WHEN status IN ('matched','resolving','rendered','creating','retrying','suspended') THEN 1 ELSE 0 END),
                           MIN(CASE WHEN status IN ('matched','resolving','rendered','creating','retrying','suspended') THEN created_at END),
                           SUM(CASE WHEN lease_token IS NOT NULL AND lease_expires_at>?2 THEN 1 ELSE 0 END),
                           SUM(CASE WHEN status='retrying' THEN 1 ELSE 0 END),
                           SUM(CASE WHEN status='needs_attention' THEN 1 ELSE 0 END)
                         FROM source_automation_routes WHERE project_id=?1",
                        params![project_id, now.to_rfc3339()],
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
                    let oldest_age_seconds = oldest
                        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                        .map(|value| {
                            now.signed_duration_since(value.with_timezone(&Utc))
                                .num_seconds()
                                .max(0) as u64
                        })
                        .unwrap_or_default();
                    let mut stmt = conn.prepare(
                        "SELECT COALESCE(error_category,'unknown'),COUNT(*)
                         FROM source_automation_routes WHERE project_id=?1
                           AND status IN ('needs_attention','failed')
                         GROUP BY COALESCE(error_category,'unknown') ORDER BY 1 LIMIT 32",
                    )?;
                    let failure_categories = stmt
                        .query_map([&project_id], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                        })?
                        .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
                    Ok(SourceAutomationStatus {
                        project_id,
                        backlog_count,
                        oldest_age_seconds,
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

    /// Pauses unleased routes for an installation or binding scope. Active
    /// leases finish their bounded transition and observe suspension before a
    /// later retry claim.
    pub async fn suspend_scope(
        &self,
        project_id: &str,
        installation_id: Option<&str>,
        binding_name: Option<&str>,
        scope: &str,
    ) -> Result<usize> {
        self.set_scope_suspended(project_id, installation_id, binding_name, scope, true)
            .await
    }

    /// Resumes routes paused by the matching installation or binding scope.
    pub async fn resume_scope(
        &self,
        project_id: &str,
        installation_id: Option<&str>,
        binding_name: Option<&str>,
        scope: &str,
    ) -> Result<usize> {
        self.set_scope_suspended(project_id, installation_id, binding_name, scope, false)
            .await
    }

    async fn set_scope_suspended(
        &self,
        project_id: &str,
        installation_id: Option<&str>,
        binding_name: Option<&str>,
        scope: &str,
        suspend: bool,
    ) -> Result<usize> {
        let project_id = project_id.to_owned();
        let installation_id = installation_id.map(str::to_owned);
        let binding_name = binding_name.map(str::to_owned);
        let scope = scope.to_owned();
        self.db
            .writer()
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
                            params![project_id, installation_id, binding_name, scope, suspend],
                            |row| row.get::<_, String>(0),
                        )?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                    };
                    let now = now_ts();
                    for id in &ids {
                        if suspend {
                            tx.execute(
                                "UPDATE source_automation_routes SET status='suspended',
                                 suspended_scope=?2,version=version+1,updated_at=?3 WHERE id=?1",
                                params![id, scope, now],
                            )?;
                        } else {
                            tx.execute(
                                "UPDATE source_automation_routes SET
                                 status=CASE WHEN permalink_status='resolved' THEN 'rendered' ELSE 'matched' END,
                                 suspended_scope=NULL,next_attempt_at=?2,version=version+1,updated_at=?2
                                 WHERE id=?1",
                                params![id, now],
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

    /// Applies the daemon retention window to sensitive/per-attempt metadata
    /// while retaining route/task/audit provenance.
    pub async fn cleanup_metadata(&self, retention_days: u32, limit: usize) -> Result<u64> {
        let days = retention_days.clamp(1, 365);
        let limit = limit.clamp(1, 10_000);
        self.db
            .writer()
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
                    let now = now_ts();
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
}

/// Internal frozen values required to execute a reserved route.
#[derive(Debug, Clone)]
pub struct SourceAutomationExecutionSnapshot {
    /// Binding snapshot.
    pub binding: serde_json::Value,
    /// Template snapshot.
    pub template: serde_json::Value,
    /// SecretStore name.
    pub credential_store: String,
    /// SecretStore key.
    pub credential_key: String,
}

/// Computes the default one-task-per-message/badge/binding identity.
pub fn automation_key(
    project_id: &str,
    installation_id: &str,
    message_identity: &str,
    reaction: &str,
    binding_name: &str,
) -> String {
    digest_hex(
        format!(
            "source-automation:{project_id}:{installation_id}:{message_identity}:{reaction}:{binding_name}"
        )
        .as_bytes(),
    )
}

/// Computes the deterministic task ID for an automation key.
pub fn deterministic_automation_task_id(key: &str) -> String {
    format!("source-auto-{}", &key[..24])
}

fn reserve(
    conn: &Connection,
    input: ReserveSourceAutomationRoute,
) -> Result<SourceAutomationReservation> {
    for (label, value) in [
        ("project_id", input.project_id.as_str()),
        ("source_event_id", input.source_event_id.as_str()),
        ("installation_id", input.installation_id.as_str()),
        ("message_identity", input.message_identity.as_str()),
        ("binding_name", input.binding_name.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > 512 {
            bail!("{label} must contain 1-512 characters");
        }
    }
    let key = automation_key(
        &input.project_id,
        &input.installation_id,
        &input.message_identity,
        &input.reaction,
        &input.binding_name,
    );
    let id = format!("route-{}", &key[..24]);
    let task_id = deterministic_automation_task_id(&key);
    let request_id = format!("req-source-auto-{}", &key[..24]);
    let binding_snapshot = serde_json::to_string(&input.binding_snapshot)?;
    let template_snapshot = serde_json::to_string(&input.template_snapshot)?;
    let now = now_ts();
    let tx = conn.unchecked_transaction()?;
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO source_automation_routes
         (id,project_id,automation_key,source_event_id,provider,installation_id,message_identity,
          channel_id,message_ts,reaction,resolved_role,binding_name,binding_revision,template_name,template_hash,
          binding_snapshot_json,template_snapshot_json,credential_store,credential_key,request_id,
          deterministic_task_id,status,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,
                 'matched',?22,?22)",
        params![
            id,
            input.project_id,
            key,
            input.source_event_id,
            input.provider,
            input.installation_id,
            input.message_identity,
            input.channel_id,
            input.message_ts,
            input.reaction,
            input.resolved_role,
            input.binding_name,
            input.binding_revision,
            input.template_name,
            input.template_hash,
            binding_snapshot,
            template_snapshot,
            input.credential_store,
            input.credential_key,
            request_id,
            task_id,
            now
        ],
    )? == 1;
    let should_execute = inserted;
    if inserted {
        tx.execute(
            "INSERT INTO source_automation_route_generations
             (route_id,generation,binding_name,binding_revision,template_name,template_hash,
              binding_snapshot_json,template_snapshot_json,credential_store,credential_key,
              request_id,deterministic_task_id,created_at)
             VALUES(?1,1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                id,
                input.binding_name,
                input.binding_revision,
                input.template_name,
                input.template_hash,
                binding_snapshot,
                template_snapshot,
                input.credential_store,
                input.credential_key,
                request_id,
                task_id,
                now,
            ],
        )?;
        append_route_change(&tx, &id)?;
    }
    tx.execute(
        "UPDATE source_events SET automation_route_id=?2 WHERE id=?1",
        params![input.source_event_id, id],
    )?;
    tx.execute(
        "UPDATE source_routing_attempts SET automation_route_id=?2
         WHERE source_event_id=?1 AND attempt_no=(SELECT routing_attempts FROM source_events WHERE id=?1)",
        params![input.source_event_id, id],
    )?;
    let route = read_route(&tx, &id)?.context("reserved automation route missing")?;
    if route.project_id != input.project_id
        || route.installation_id != input.installation_id
        || route.message_identity != input.message_identity
        || route.reaction != input.reaction
        || route.resolved_role != input.resolved_role
        || route.binding_name != input.binding_name
    {
        bail!("automation identity collision");
    }
    tx.commit()?;
    Ok(SourceAutomationReservation {
        route,
        should_execute,
    })
}

fn read_route(conn: &Connection, id: &str) -> Result<Option<SourceAutomationRoute>> {
    conn.query_row(
        "SELECT id,project_id,automation_key,source_event_id,provider,installation_id,
         message_identity,channel_id,message_ts,reaction,resolved_role,binding_name,binding_revision,
         template_name,template_hash,permalink_status,permalink,request_id,
         deterministic_task_id,task_id,status,error_code,error_category,generation,version,
         attempt_count,max_attempts,next_attempt_at,lease_owner,lease_token,lease_expires_at,
         suspended_scope,last_attempt_at,created_at,updated_at,completed_at
         FROM source_automation_routes WHERE id=?1",
        [id],
        |row| {
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
        },
    )
    .optional()
    .map_err(Into::into)
}

fn read_execution_snapshot(
    conn: &Connection,
    id: &str,
) -> Result<Option<SourceAutomationExecutionSnapshot>> {
    conn.query_row(
        "SELECT g.binding_snapshot_json,g.template_snapshot_json,g.credential_store,g.credential_key
         FROM source_automation_routes r
         JOIN source_automation_route_generations g
           ON g.route_id=r.id AND g.generation=r.generation
         WHERE r.id=?1",
        [id],
        |row| {
            let binding: String = row.get(0)?;
            let template: String = row.get(1)?;
            let binding = serde_json::from_str(&binding).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    binding.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let template = serde_json::from_str(&template).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    template.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(SourceAutomationExecutionSnapshot {
                binding,
                template,
                credential_store: row.get(2)?,
                credential_key: row.get(3)?,
            })
        },
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

/// Computes deterministic bounded exponential backoff. Provider Retry-After is
/// treated as a lower bound after the adapter has capped it.
pub fn retry_delay_seconds(route_id: &str, attempt_no: i64, retry_after: Option<u64>) -> u64 {
    let exponent = attempt_no.saturating_sub(1).clamp(0, 7) as u32;
    let base = 2_u64
        .saturating_mul(2_u64.saturating_pow(exponent))
        .min(240);
    let digest = Sha256::digest(format!("{route_id}:{attempt_no}").as_bytes());
    let jitter_window = (base / 5).max(1);
    let jitter = u64::from(digest[0]) % (jitter_window + 1);
    base.saturating_add(jitter)
        .max(retry_after.unwrap_or_default().min(300))
        .min(300)
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::ConcurrencyPolicy;
    use crate::config::{
        OrchestratorConfig, SourceTaskBindingConfig, SourceTaskBindingMatchConfig,
        SourceTaskTemplateActionConfig, SourceTaskTemplateConfig, SourceTaskTemplateSkillConfig,
        TriggerActionConfig, TriggerConfig, TriggerEventConfig, TriggerWebhookConfig,
    };
    use crate::source::{
        ExternalActorRef, IngestSourceEvent, NormalizedSourceEvent, SourceEventKind,
    };
    use crate::test_utils::TestState;
    use std::collections::{BTreeMap, HashMap};

    fn input(event_id: String) -> ReserveSourceAutomationRoute {
        input_for(event_id, "T1", "C1:1.23")
    }

    fn input_for(
        event_id: String,
        installation_id: &str,
        message_identity: &str,
    ) -> ReserveSourceAutomationRoute {
        let (channel_id, message_ts) = message_identity.split_once(':').expect("message identity");
        ReserveSourceAutomationRoute {
            project_id: "demo".into(),
            source_event_id: event_id,
            provider: "slack".into(),
            installation_id: installation_id.into(),
            message_identity: message_identity.into(),
            channel_id: channel_id.into(),
            message_ts: message_ts.into(),
            reaction: "agent-analyze".into(),
            resolved_role: "operator".into(),
            binding_name: "analyze".into(),
            binding_revision: "rev-1".into(),
            template_name: "analyze-template".into(),
            template_hash: "hash-1".into(),
            binding_snapshot: serde_json::json!({"name": "analyze"}),
            template_snapshot: serde_json::json!({"skill": {"name": "analyze"}}),
            credential_store: "slack-api".into(),
            credential_key: "BOT_TOKEN".into(),
        }
    }

    async fn ingest_event(
        source: &crate::source::AsyncSourceRepository,
        external_event_id: &str,
    ) -> crate::source::SourceEventRecord {
        source
            .ingest(IngestSourceEvent {
                project_id: "demo".into(),
                event: NormalizedSourceEvent {
                    provider: "slack".into(),
                    installation_id: "T1".into(),
                    external_event_id: external_event_id.into(),
                    kind: SourceEventKind::ReactionAdded,
                    reaction: Some(crate::source::SourceReactionRef {
                        name: "agent-analyze".into(),
                        target: crate::source::ExternalArtifactRef {
                            kind: "message".into(),
                            external_id: "C1:1.23".into(),
                            url: None,
                        },
                    }),
                    actor: ExternalActorRef {
                        external_id: "U1".into(),
                        display_name: None,
                    },
                    conversation: None,
                    text_summary: None,
                    command: None,
                    attachments: vec![],
                    occurred_at: "2026-07-17T00:00:00Z".into(),
                },
                payload_hash: format!("hash-{external_event_id}"),
                raw_payload_ref: None,
            })
            .await
            .expect("ingest")
            .event
    }

    #[tokio::test]
    async fn duplicate_delivery_reserves_one_automation_identity() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let source = crate::source::AsyncSourceRepository::new(state.async_database.clone());
        let event = ingest_event(&source, "Ev1").await;
        let repository = AsyncSourceAutomationRepository::new(state.async_database.clone());
        let first = repository
            .reserve(input(event.id.clone()))
            .await
            .expect("reserve");
        let second = repository.reserve(input(event.id)).await.expect("dedupe");
        assert!(first.should_execute);
        assert!(!second.should_execute);
        assert_eq!(first.route.id, second.route.id);
        assert_eq!(
            first.route.deterministic_task_id,
            second.route.deterministic_task_id
        );
    }

    #[test]
    fn retry_backoff_is_deterministic_bounded_and_respects_retry_after() {
        let first = retry_delay_seconds("route-1", 3, None);
        assert_eq!(first, retry_delay_seconds("route-1", 3, None));
        assert!((8..=10).contains(&first));
        assert_eq!(retry_delay_seconds("route-1", 3, Some(75)), 75);
        assert_eq!(retry_delay_seconds("route-1", 99, Some(900)), 300);
    }

    #[tokio::test]
    async fn lease_expiry_is_reclaimed_and_attempt_history_is_closed() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let source = crate::source::AsyncSourceRepository::new(state.async_database.clone());
        let event = ingest_event(&source, "Ev-lease").await;
        let repository = AsyncSourceAutomationRepository::new(state.async_database.clone());
        let route = repository
            .reserve(input(event.id))
            .await
            .expect("reserve")
            .route;
        let now = Utc::now();
        let first = repository
            .claim_due("worker-a", 10, now, 15)
            .await
            .expect("first claim");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].attempt_count, 1);

        let reclaimed = repository
            .claim_due("worker-b", 10, now + Duration::seconds(16), 15)
            .await
            .expect("reclaim");
        assert_eq!(reclaimed.len(), 1);
        assert_eq!(reclaimed[0].id, route.id);
        assert_eq!(reclaimed[0].attempt_count, 2);
        assert_ne!(first[0].lease_token, reclaimed[0].lease_token);
        let attempts = repository.attempts(&route.id, 10).await.expect("attempts");
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[1].error_code.as_deref(),
            Some("route_lease_expired")
        );
        assert_eq!(attempts[1].result_state.as_deref(), Some("retrying"));
    }

    #[tokio::test]
    async fn claims_at_most_one_route_per_installation() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let source = crate::source::AsyncSourceRepository::new(state.async_database.clone());
        let first_event = ingest_event(&source, "Ev-one").await;
        let second_event = ingest_event(&source, "Ev-two").await;
        let repository = AsyncSourceAutomationRepository::new(state.async_database.clone());
        repository
            .reserve(input_for(first_event.id, "T1", "C1:1.23"))
            .await
            .expect("first reserve");
        repository
            .reserve(input_for(second_event.id, "T1", "C1:2.34"))
            .await
            .expect("second reserve");
        let claimed = repository
            .claim_due("worker", 10, Utc::now(), 60)
            .await
            .expect("claim");
        assert_eq!(claimed.len(), 1);
    }

    #[tokio::test]
    async fn retry_replay_ignore_and_watch_use_stable_versions() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let source = crate::source::AsyncSourceRepository::new(state.async_database.clone());
        let event = ingest_event(&source, "Ev-ops").await;
        let repository = AsyncSourceAutomationRepository::new(state.async_database.clone());
        let route = repository
            .reserve(input(event.id))
            .await
            .expect("reserve")
            .route;
        let now = Utc::now();
        let claimed = repository
            .claim_due("worker", 1, now, 60)
            .await
            .expect("claim")
            .pop()
            .expect("route");
        let retried = repository
            .schedule_retry(
                &route.id,
                claimed.lease_token.as_deref().expect("lease"),
                "slack_rate_limited",
                "rate_limit",
                now,
                Some(45),
            )
            .await
            .expect("retry");
        assert_eq!(retried.status, "retrying");
        assert!(
            repository
                .claim_due("worker", 1, now + Duration::seconds(44), 60)
                .await
                .expect("early claim")
                .is_empty()
        );
        let claimed = repository
            .claim_due("worker", 1, now + Duration::seconds(46), 60)
            .await
            .expect("due claim")
            .pop()
            .expect("route");
        let blocked = repository
            .needs_attention(
                &route.id,
                claimed.lease_token.as_deref().expect("lease"),
                "slack_credential_rejected",
                "credential",
            )
            .await
            .expect("attention state");
        let replayed = repository
            .replay(&route.id, blocked.version)
            .await
            .expect("replay");
        assert!(replayed.changed);
        assert!(repository.replay(&route.id, blocked.version).await.is_err());

        let claimed = repository
            .claim_due("worker", 1, Utc::now() + Duration::seconds(1), 60)
            .await
            .expect("replay claim")
            .pop()
            .expect("route");
        let blocked_again = repository
            .needs_attention(
                &route.id,
                claimed.lease_token.as_deref().expect("lease"),
                "slack_message_forbidden",
                "visibility",
            )
            .await
            .expect("attention state");
        let ignored = repository
            .ignore(&route.id, blocked_again.version)
            .await
            .expect("ignore");
        assert_eq!(ignored.route.status, "ignored");
        assert!(
            repository
                .changes_since(Some("demo"), 0, 100)
                .await
                .expect("changes")
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
        );
        let attempts = repository.attempts(&route.id, 10).await.expect("attempts");
        assert_eq!(attempts.len(), 3);
        assert_eq!(attempts[0].result_state.as_deref(), Some("needs_attention"));
    }

    #[tokio::test]
    async fn scope_suspend_resume_and_status_preserve_history() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let source = crate::source::AsyncSourceRepository::new(state.async_database.clone());
        let event = ingest_event(&source, "Ev-suspend").await;
        let repository = AsyncSourceAutomationRepository::new(state.async_database.clone());
        let route = repository
            .reserve(input(event.id))
            .await
            .expect("reserve")
            .route;
        assert_eq!(
            repository
                .suspend_scope("demo", None, Some("analyze"), "binding:analyze")
                .await
                .expect("suspend"),
            1
        );
        assert_eq!(
            repository.get(&route.id).await.unwrap().unwrap().status,
            "suspended"
        );
        let status = repository.status("demo", Utc::now()).await.expect("status");
        assert_eq!(status.backlog_count, 1);
        assert_eq!(
            repository
                .list(SourceAutomationRouteFilter {
                    project_id: Some("demo".into()),
                    state: Some("suspended".into()),
                    limit: 10,
                    ..Default::default()
                })
                .await
                .expect("list")
                .len(),
            1
        );
        assert_eq!(
            repository
                .resume_scope("demo", None, Some("analyze"), "binding:analyze")
                .await
                .expect("resume"),
            1
        );
        assert_eq!(
            repository.get(&route.id).await.unwrap().unwrap().status,
            "matched"
        );
    }

    fn simulation_config() -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        let project = config.ensure_project(Some("demo"));
        project.triggers.insert(
            "slack-main".into(),
            TriggerConfig {
                cron: None,
                event: Some(TriggerEventConfig {
                    source: "webhook".into(),
                    filter: None,
                    webhook: Some(TriggerWebhookConfig {
                        connection_ref: None,
                        secret: None,
                        outbound_credential: None,
                        signature_header: None,
                        crd_ref: None,
                        provider: Some("slack".into()),
                        installation_id: Some("T1".into()),
                        actor_roles: HashMap::from([("U1".into(), "operator".into())]),
                        reaction_routing: "bindings".into(),
                        timestamp_tolerance_secs: 300,
                    }),
                    filesystem: None,
                }),
                action: TriggerActionConfig {
                    workflow: "noop".into(),
                    workspace: "noop".into(),
                    args: None,
                    start: false,
                },
                concurrency_policy: ConcurrencyPolicy::Allow,
                suspend: false,
                history_limit: None,
                throttle: None,
            },
        );
        project.source_task_bindings.insert(
            "analyze".into(),
            SourceTaskBindingConfig {
                trigger_ref: "slack-main".into(),
                match_rule: SourceTaskBindingMatchConfig {
                    event_kind: "reaction_added".into(),
                    reaction: "agent-analyze".into(),
                    target_kind: "message".into(),
                    channels: vec!["C1".into()],
                    all_channels: false,
                },
                template_ref: "analyze-template".into(),
                allowed_actor_roles: vec!["operator".into()],
                suspend: false,
            },
        );
        project.source_task_templates.insert(
            "analyze-template".into(),
            SourceTaskTemplateConfig {
                skill: SourceTaskTemplateSkillConfig {
                    name: "analyze".into(),
                    invocation: "$analyze".into(),
                    args: vec![],
                },
                action: SourceTaskTemplateActionConfig {
                    workflow: "basic".into(),
                    workspace: "default".into(),
                    start: false,
                    initial_vars: BTreeMap::new(),
                },
                goal_template: "{skill_invocation} {source_message_url}".into(),
                allowed_variables: vec!["skill_invocation".into(), "source_message_url".into()],
            },
        );
        config
    }

    #[test]
    fn simulation_uses_the_live_matcher_and_renderer_without_side_effects() {
        let config = simulation_config();
        let input = SourceAutomationSimulationInput {
            project_id: "demo".into(),
            match_input: crate::source_task_binding::SourceTaskBindingMatchInput {
                provider: "slack".into(),
                installation_id: "T1".into(),
                event_kind: "reaction_added".into(),
                reaction: "agent-analyze".into(),
                target_kind: "message".into(),
                channel_id: "C1".into(),
                external_actor_id: "U1".into(),
            },
            message_url: "https://acme.slack.com/archives/C1/p123".into(),
            event_id: Some("Ev-sim".into()),
            target_id: "C1:1.23".into(),
        };
        let direct_match = crate::source_task_binding::match_source_task_binding(
            &config,
            "demo",
            &input.match_input,
        )
        .expect("direct match");
        let simulation = simulate_source_automation(&config, &input).expect("simulation");
        assert_eq!(simulation.match_result.status, direct_match.status);
        assert_eq!(simulation.match_result.binding_id, direct_match.binding_id);
        assert_eq!(
            simulation.rendered.expect("rendered").goal,
            "$analyze https://acme.slack.com/archives/C1/p123"
        );
    }
}
