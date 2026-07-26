//! Durable source automation reservation and provenance.
//!
//! The route tables live in `orchestrator_persistence::source_automation_routes`.
//! What stays here is everything that decides rather than stores: what a
//! well-formed reservation input is, how the stable automation identity and the
//! deterministic task id are derived, how long a retry waits, which states
//! release a lease, and what a refused fence means to the operator who asked.
//!
//! The row types are the store's and are re-exported below. They are flat
//! columns with no enums and no embedded JSON, so there is nothing above the
//! boundary left to parse — with one exception. A route's frozen binding and
//! template snapshots are `serde_json::Value` here and text down there, and
//! [`AsyncSourceAutomationRepository::execution_snapshot`] is where they become
//! typed. That parse used to happen inside a row mapper, where a malformed
//! snapshot had to be reported as a column-conversion failure against a column
//! index computed from the string's own length (FR-130 B15).

use crate::async_database::AsyncDatabase;
use crate::config_load::now_ts;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use orchestrator_persistence::source_automation_routes as store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

pub use orchestrator_persistence::source_automation_routes::{
    SourceAutomationRoute, SourceAutomationRouteAttempt, SourceAutomationRouteChange,
    SourceAutomationRouteFilter,
};

/// Longest any bounded identity field may be.
const MAX_IDENTITY_FIELD: usize = 512;

/// Longest a lease owner label may be.
const MAX_LEASE_OWNER: usize = 128;

/// Bounds on how long a claim may hold a route, whatever the caller asks for.
const LEASE_SECONDS: std::ops::RangeInclusive<i64> = 15..=300;

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
        for (label, value) in [
            ("project_id", input.project_id.as_str()),
            ("source_event_id", input.source_event_id.as_str()),
            ("installation_id", input.installation_id.as_str()),
            ("message_identity", input.message_identity.as_str()),
            ("binding_name", input.binding_name.as_str()),
        ] {
            if value.trim().is_empty() || value.len() > MAX_IDENTITY_FIELD {
                bail!("{label} must contain 1-{MAX_IDENTITY_FIELD} characters");
            }
        }
        let key = automation_key(
            &input.project_id,
            &input.installation_id,
            &input.message_identity,
            &input.reaction,
            &input.binding_name,
        );
        let outcome = store::reserve(
            &self.db,
            store::NewRoute {
                id: route_id(&key),
                automation_key: key.clone(),
                request_id: reservation_request_id(&key),
                deterministic_task_id: deterministic_automation_task_id(&key),
                identity: store::RouteIdentity {
                    project_id: input.project_id,
                    installation_id: input.installation_id,
                    message_identity: input.message_identity,
                    reaction: input.reaction,
                    resolved_role: input.resolved_role,
                    binding_name: input.binding_name,
                },
                source_event_id: input.source_event_id,
                provider: input.provider,
                channel_id: input.channel_id,
                message_ts: input.message_ts,
                binding_revision: input.binding_revision,
                template_name: input.template_name,
                template_hash: input.template_hash,
                binding_snapshot_json: serde_json::to_string(&input.binding_snapshot)?,
                template_snapshot_json: serde_json::to_string(&input.template_snapshot)?,
                credential_store: input.credential_store,
                credential_key: input.credential_key,
                created_at: now_ts(),
            },
        )
        .await?;
        match outcome {
            store::Reservation::Reserved(route) => Ok(SourceAutomationReservation {
                route: *route,
                should_execute: true,
            }),
            store::Reservation::Existing(route) => Ok(SourceAutomationReservation {
                route: *route,
                should_execute: false,
            }),
            store::Reservation::IdentityCollision(_) => bail!("automation identity collision"),
        }
    }

    /// Loads a route by route ID.
    pub async fn get(&self, id: &str) -> Result<Option<SourceAutomationRoute>> {
        store::read_route(&self.db, id.to_owned()).await
    }

    /// Loads the route linked to a source event.
    pub async fn get_for_event(
        &self,
        source_event_id: &str,
    ) -> Result<Option<SourceAutomationRoute>> {
        store::read_route_for_event(&self.db, source_event_id.to_owned()).await
    }

    /// Returns the frozen internal snapshots and credential reference.
    pub async fn execution_snapshot(
        &self,
        id: &str,
    ) -> Result<Option<SourceAutomationExecutionSnapshot>> {
        let Some(row) = store::read_execution_snapshot(&self.db, id.to_owned()).await? else {
            return Ok(None);
        };
        Ok(Some(SourceAutomationExecutionSnapshot {
            binding: serde_json::from_str(&row.binding_json)
                .with_context(|| format!("parse frozen binding snapshot of route {id}"))?,
            template: serde_json::from_str(&row.template_json)
                .with_context(|| format!("parse frozen template snapshot of route {id}"))?,
            credential_store: row.credential_store,
            credential_key: row.credential_key,
        }))
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
        if owner.trim().is_empty() || owner.len() > MAX_LEASE_OWNER {
            bail!("route lease owner must contain 1-{MAX_LEASE_OWNER} characters");
        }
        let lease_expires_at = (now
            + Duration::seconds(lease_seconds.clamp(*LEASE_SECONDS.start(), *LEASE_SECONDS.end())))
        .to_rfc3339();
        store::claim_due(
            &self.db,
            store::Claim {
                owner: owner.to_owned(),
                limit,
                now: now.to_rfc3339(),
                lease_expires_at,
            },
        )
        .await
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
        store::suspend_leased(
            &self.db,
            id.to_owned(),
            lease_token.to_owned(),
            scope.to_owned(),
            now_ts(),
        )
        .await?
        .context("automation route lease is stale")
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
        let outcome = store::replay(&self.db, id.to_owned(), expected_version, now_ts()).await?;
        let route = applied_or_explain(
            outcome,
            expected_version,
            "automation route is not replayable",
        )?;
        Ok(SourceAutomationMutationResult {
            route,
            changed: true,
        })
    }

    /// Creates a new immutable generation after explicit current-config
    /// preview/adoption. The stable automation identity and deterministic task
    /// fence remain unchanged.
    pub async fn adopt_generation(
        &self,
        input: AdoptSourceAutomationGeneration,
    ) -> Result<SourceAutomationMutationResult> {
        // Read first, only to learn the generation number the audit request id
        // embeds. The write fences on `version`, and every mutation of a route
        // bumps `version`, so a generation that moved under us is a rejected
        // fence rather than a wrong id.
        let current = self
            .get(&input.route_id)
            .await?
            .context("automation route missing")?;
        let generation = current.generation + 1;
        let outcome = store::adopt_generation(
            &self.db,
            store::NewGeneration {
                route_id: input.route_id,
                expected_version: input.expected_version,
                generation,
                request_id: generation_request_id(&current.automation_key, generation),
                deterministic_task_id: current.deterministic_task_id,
                resolved_role: input.resolved_role,
                binding_name: input.binding_name.clone(),
                binding_revision: input.binding_revision,
                template_name: input.template_name,
                template_hash: input.template_hash,
                binding_snapshot_json: serde_json::to_string(&input.binding_snapshot)?,
                template_snapshot_json: serde_json::to_string(&input.template_snapshot)?,
                credential_store: input.credential_store,
                credential_key: input.credential_key,
                created_by_request_id: input.created_by_request_id,
                now: now_ts(),
            },
        )
        .await?;
        if let store::Mutation::Rejected(route) = &outcome
            && route.version == input.expected_version
            && route.binding_name != input.binding_name
        {
            bail!("current config selects a different binding; cross-binding reroute is denied");
        }
        let route = applied_or_explain(
            outcome,
            input.expected_version,
            "automation route is not replayable",
        )?;
        Ok(SourceAutomationMutationResult {
            route,
            changed: true,
        })
    }

    /// Deliberately ignores an actionable route using optimistic concurrency.
    pub async fn ignore(
        &self,
        id: &str,
        expected_version: i64,
    ) -> Result<SourceAutomationMutationResult> {
        let outcome = store::ignore(&self.db, id.to_owned(), expected_version, now_ts()).await?;
        let route = applied_or_explain(
            outcome,
            expected_version,
            "automation route cannot be ignored from its current state",
        )?;
        Ok(SourceAutomationMutationResult {
            route,
            changed: true,
        })
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
        let terminal = matches!(state, "routed" | "needs_attention" | "ignored" | "failed");
        let release = terminal || matches!(state, "retrying" | "suspended");
        let (next_attempt_at, retry_after_seconds) = retry
            .map(|(next, hint)| (Some(next), hint))
            .unwrap_or((None, None));
        store::transition_leased(
            &self.db,
            store::LeaseTransition {
                id: id.to_owned(),
                lease_token: lease_token.to_owned(),
                state: state.to_owned(),
                error_code: error_code.map(str::to_owned),
                error_category: error_category.map(str::to_owned),
                permalink: permalink.map(str::to_owned),
                task_id: task_id.map(str::to_owned),
                next_attempt_at,
                retry_after_seconds,
                terminal,
                release,
                now: now_ts(),
            },
        )
        .await?
        .context("automation route lease is stale or route is terminal")
    }

    /// Lists routes with stable keyset pagination and bounded filters.
    pub async fn list(
        &self,
        filter: SourceAutomationRouteFilter,
    ) -> Result<Vec<SourceAutomationRoute>> {
        store::list_routes(&self.db, filter).await
    }

    /// Lists a bounded attempt history for one route.
    pub async fn attempts(
        &self,
        route_id: &str,
        limit: usize,
    ) -> Result<Vec<SourceAutomationRouteAttempt>> {
        store::read_attempts(&self.db, route_id.to_owned(), limit).await
    }

    /// Reads monotonic route changes after a reconnect cursor.
    pub async fn changes_since(
        &self,
        project_id: Option<&str>,
        after: i64,
        limit: usize,
    ) -> Result<Vec<SourceAutomationRouteChange>> {
        store::read_changes(&self.db, project_id.map(str::to_owned), after, limit).await
    }

    /// Returns privacy-safe worker backlog and failure-family health.
    pub async fn status(
        &self,
        project_id: &str,
        now: DateTime<Utc>,
    ) -> Result<SourceAutomationStatus> {
        let counts =
            store::read_status_counts(&self.db, project_id.to_owned(), now.to_rfc3339()).await?;
        let oldest_age_seconds = counts
            .oldest_created_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| {
                now.signed_duration_since(value.with_timezone(&Utc))
                    .num_seconds()
                    .max(0) as u64
            })
            .unwrap_or_default();
        Ok(SourceAutomationStatus {
            project_id: project_id.to_owned(),
            backlog_count: counts.backlog_count,
            oldest_age_seconds,
            active_leases: counts.active_leases,
            retrying_count: counts.retrying_count,
            needs_attention_count: counts.needs_attention_count,
            failure_categories: counts.failure_categories,
        })
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
        store::set_scope_suspended(
            &self.db,
            store::ScopeSuspension {
                project_id: project_id.to_owned(),
                installation_id: installation_id.map(str::to_owned),
                binding_name: binding_name.map(str::to_owned),
                scope: scope.to_owned(),
                suspend,
                now: now_ts(),
            },
        )
        .await
    }

    /// Applies the daemon retention window to sensitive/per-attempt metadata
    /// while retaining route/task/audit provenance.
    pub async fn cleanup_metadata(&self, retention_days: u32, limit: usize) -> Result<u64> {
        store::cleanup_metadata(&self.db, retention_days, limit, now_ts()).await
    }
}

/// Turns a fenced store outcome into the route or into the reason the caller
/// asked for something the row would not allow.
///
/// The store reports that its fence did not hold and hands back the row as it
/// actually is. Only the caller knows which of its conditions it cares about
/// naming first, and a version that moved is a different thing to tell an
/// operator than a state that was never eligible.
fn applied_or_explain(
    outcome: store::Mutation,
    expected_version: i64,
    ineligible: &str,
) -> Result<SourceAutomationRoute> {
    match outcome {
        store::Mutation::Applied(route) => Ok(*route),
        store::Mutation::Rejected(route) if route.version != expected_version => bail!(
            "automation route version conflict: expected {expected_version}, current {}",
            route.version
        ),
        store::Mutation::Rejected(_) => bail!("{ineligible}"),
        store::Mutation::Missing => bail!("automation route missing"),
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

/// Computes the route identifier for an automation key.
pub fn route_id(key: &str) -> String {
    format!("route-{}", &key[..24])
}

/// Computes the deterministic task ID for an automation key.
pub fn deterministic_automation_task_id(key: &str) -> String {
    format!("source-auto-{}", &key[..24])
}

/// Computes the canonical audit request ID of a route's first generation.
pub fn reservation_request_id(key: &str) -> String {
    format!("req-source-auto-{}", &key[..24])
}

/// Computes the canonical audit request ID of a later generation.
pub fn generation_request_id(key: &str, generation: i64) -> String {
    format!("req-source-auto-{}-g{generation}", &key[..24])
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
