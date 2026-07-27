//! Provider-neutral external source event and process-binding contract.
//!
//! The tables and their statements live in
//! `orchestrator_persistence::source_events` (FR-130 B11). What stays here is
//! everything a store cannot decide: what a well-formed event looks like, how a
//! deterministic identifier is derived from provider coordinates, how long a
//! failed delivery waits before its next attempt, and what it means when an
//! external id or a retry key comes back attached to something else.

use crate::async_database::AsyncDatabase;
use crate::config_load::now_ts;
use anyhow::{Context, Result, bail};
use orchestrator_persistence::source_events as store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAX_NORMALIZED_PAYLOAD_BYTES: usize = 64 * 1024;

/// Closed, provider-neutral command set accepted from source adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum SourceCommand {
    /// Approve an allowlisted attention action.
    Approve {
        /// Target attention item.
        attention_item_id: String,
        /// Optimistic attention item version encoded in the signed action token.
        expected_version: i64,
    },
    /// Reject an allowlisted attention action.
    Reject {
        /// Target attention item.
        attention_item_id: String,
        /// Optimistic attention item version encoded in the signed action token.
        expected_version: i64,
    },
    /// Retry an attention item or failed process boundary.
    Retry {
        /// Target attention item.
        attention_item_id: String,
        /// Optimistic attention item version encoded in the signed action token.
        expected_version: i64,
    },
    /// Add bounded untrusted context to the bound process.
    AddContext,
    /// Cancel a bound process through normal control-plane policy.
    Cancel,
    /// Create a child process from the bound process.
    Branch,
    /// Return a console deep link without mutating process state.
    OpenConsole,
}

/// Provider-neutral source event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEventKind {
    /// Human-authored message or comment.
    Message,
    /// Explicit allowlisted command.
    Command,
    /// External artifact update.
    Artifact,
    /// A verified actor added a named reaction to an external artifact.
    ReactionAdded,
    /// Provider lifecycle notification with no process mutation.
    System,
}

impl SourceEventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Command => "command",
            Self::Artifact => "artifact",
            Self::ReactionAdded => "reaction_added",
            Self::System => "system",
        }
    }
}

/// External actor identity asserted by a verified adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalActorRef {
    /// Provider-owned stable actor ID.
    pub external_id: String,
    /// Optional bounded display label.
    pub display_name: Option<String>,
}

/// Provider conversation correlation coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRef {
    /// Provider conversation or channel ID.
    pub conversation_id: String,
    /// Provider thread/root identifier when present.
    pub thread_id: Option<String>,
    /// Whether the event represents a new top-level conversation entry.
    pub top_level: bool,
}

/// Bounded reference to an external artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalArtifactRef {
    /// Provider-neutral artifact kind.
    pub kind: String,
    /// Stable provider-owned artifact ID.
    pub external_id: String,
    /// Optional safe URL retained as reference only.
    pub url: Option<String>,
}

/// Provider-neutral description of a reaction and its target artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReactionRef {
    /// Canonical provider reaction name without presentation delimiters.
    pub name: String,
    /// Artifact that received the reaction.
    pub target: ExternalArtifactRef,
}

/// Normalized event contract shared by Slack and non-Slack adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedSourceEvent {
    /// Provider name such as `slack` or `fixture`.
    pub provider: String,
    /// Configured installation identity.
    pub installation_id: String,
    /// Provider event identity used for deduplication.
    pub external_event_id: String,
    /// Semantic event kind.
    pub kind: SourceEventKind,
    /// Reaction metadata, present only when `kind` is `reaction_added`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<SourceReactionRef>,
    /// External actor reference.
    pub actor: ExternalActorRef,
    /// Optional conversation coordinates.
    pub conversation: Option<ConversationRef>,
    /// Optional bounded and redacted presentation summary.
    pub text_summary: Option<String>,
    /// Optional closed command.
    pub command: Option<SourceCommand>,
    /// External artifact references.
    pub attachments: Vec<ExternalArtifactRef>,
    /// Provider occurrence timestamp in RFC3339 form.
    pub occurred_at: String,
}

/// Input accepted by the durable ingestion repository.
#[derive(Debug, Clone)]
pub struct IngestSourceEvent {
    /// Project selected by trusted installation configuration.
    pub project_id: String,
    /// Normalized event.
    pub event: NormalizedSourceEvent,
    /// SHA-256 of the authenticated raw body.
    pub payload_hash: String,
    /// Optional secure raw-payload reference; the Slack pilot leaves this empty.
    pub raw_payload_ref: Option<String>,
}

/// Durable source event returned by services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEventRecord {
    /// Orchestrator source-event ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Provider name.
    pub provider: String,
    /// Installation identity.
    pub installation_id: String,
    /// Provider event identity.
    pub external_event_id: String,
    /// Semantic event type.
    pub event_type: String,
    /// Optional external actor ID.
    pub external_actor_id: Option<String>,
    /// Optional conversation ID.
    pub conversation_id: Option<String>,
    /// Optional thread/root ID.
    pub thread_id: Option<String>,
    /// Provider occurrence timestamp.
    pub occurred_at: String,
    /// Durable receipt timestamp.
    pub received_at: String,
    /// Normalized provider-neutral payload.
    pub normalized: NormalizedSourceEvent,
    /// Authenticated payload digest.
    pub payload_hash: String,
    /// Routing state.
    pub routing_state: String,
    /// Number of claimed routing attempts.
    pub routing_attempts: i64,
    /// Deterministic or resolved task ID.
    pub routed_task_id: Option<String>,
    /// Last stable error code.
    pub last_error_code: Option<String>,
}

/// Result of inserting a possibly retried provider event.
#[derive(Debug, Clone)]
pub struct IngestResult {
    /// Durable event record.
    pub event: SourceEventRecord,
    /// True only when this call inserted the row.
    pub inserted: bool,
}

/// Durable external-source binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBinding {
    /// Binding ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Bound task/process.
    pub task_id: String,
    /// Provider name.
    pub provider: String,
    /// Installation identity.
    pub installation_id: String,
    /// Optional conversation ID.
    pub conversation_id: Option<String>,
    /// Optional thread/root ID.
    pub thread_id: Option<String>,
    /// Binding type (`primary`, `related`, `notification_target`, or `automation`).
    pub binding_type: String,
    /// Source event that created the binding.
    pub created_by_event_id: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// Input for creating an idempotent source binding.
#[derive(Debug, Clone)]
pub struct CreateSourceBinding {
    /// Owning project.
    pub project_id: String,
    /// Bound task/process.
    pub task_id: String,
    /// Provider name.
    pub provider: String,
    /// Installation identity.
    pub installation_id: String,
    /// Optional conversation ID.
    pub conversation_id: Option<String>,
    /// Optional thread/root ID.
    pub thread_id: Option<String>,
    /// Binding type.
    pub binding_type: String,
    /// Source event that created the binding.
    pub created_by_event_id: String,
}

/// Audit reservation for one allowlisted command received from an external source.
#[derive(Debug, Clone)]
pub struct SourceCommandActionInput {
    /// Canonical request identifier joining this projection to the control-plane audit.
    pub request_id: String,
    /// Source event carrying the command.
    pub source_event_id: String,
    /// Authenticated provider actor identity.
    pub actor: String,
    /// Locally resolved role.
    pub resolved_role: String,
    /// Target domain (`task` or `attention_item`).
    pub target_type: String,
    /// Target identifier.
    pub target_id: String,
    /// Closed command name.
    pub action: String,
    /// Stable retry key.
    pub idempotency_key: String,
    /// Digest of the normalized command request.
    pub request_hash: String,
}

/// Async repository for source events and bindings.
#[derive(Clone)]
pub struct AsyncSourceRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncSourceRepository {
    /// Creates a repository over the shared database connections.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Inserts a normalized event or returns the existing identical event.
    pub async fn ingest(&self, input: IngestSourceEvent) -> Result<IngestResult> {
        validate_ingest(&input)?;
        let normalized_payload_json = serde_json::to_string(&input.event)?;
        if normalized_payload_json.len() > MAX_NORMALIZED_PAYLOAD_BYTES {
            bail!("normalized source payload exceeds 65536 bytes");
        }
        let id = stable_source_id(
            &input.event.provider,
            &input.event.installation_id,
            &input.event.external_event_id,
        );
        let conversation_id = input
            .event
            .conversation
            .as_ref()
            .map(|value| value.conversation_id.clone());
        let thread_id = input
            .event
            .conversation
            .as_ref()
            .and_then(|value| value.thread_id.clone());
        let payload_hash = input.payload_hash.clone();
        let (row, inserted) = store::ingest_event(
            &self.db,
            store::NewSourceEvent {
                id,
                project_id: input.project_id,
                provider: input.event.provider.clone(),
                installation_id: input.event.installation_id.clone(),
                external_event_id: input.event.external_event_id.clone(),
                event_type: input.event.kind.as_str().to_string(),
                external_actor_id: input.event.actor.external_id.clone(),
                conversation_id,
                thread_id,
                occurred_at: input.event.occurred_at.clone(),
                received_at: now_ts(),
                normalized_payload_json,
                raw_payload_ref: input.raw_payload_ref,
                payload_hash: payload_hash.clone(),
            },
        )
        .await?;
        let event = record_from_row(row)?;
        // The identifier is derived from provider coordinates, so a second event
        // arriving under the same coordinates with a different body means the
        // provider reused an id. Fail closed rather than silently returning the
        // first body under the second event's name.
        if event.payload_hash != payload_hash {
            bail!("external event id was reused with a different payload");
        }
        Ok(IngestResult { event, inserted })
    }

    /// Returns one source event.
    pub async fn get(&self, id: &str) -> Result<Option<SourceEventRecord>> {
        store::read_event(&self.db, id.to_owned())
            .await?
            .map(record_from_row)
            .transpose()
    }

    /// Lists recent events, optionally scoped to a task or routing state.
    pub async fn list(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
        routing_state: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SourceEventRecord>> {
        let rows = store::list_events(
            &self.db,
            project_id.map(str::to_owned),
            task_id.map(str::to_owned),
            routing_state.map(str::to_owned),
            limit,
        )
        .await?;
        rows.into_iter().map(record_from_row).collect()
    }

    /// Atomically claims a bounded routing batch.
    pub async fn claim_pending(&self, limit: usize) -> Result<Vec<SourceEventRecord>> {
        // A claim older than this is treated as abandoned by whoever took it.
        let stale_before = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        let rows = store::claim_pending_events(&self.db, limit, stale_before, now_ts()).await?;
        rows.into_iter().map(record_from_row).collect()
    }

    /// Counts source events that still require routing or retry.
    pub async fn routing_lag(&self) -> Result<u64> {
        store::routing_lag(&self.db).await
    }

    /// Completes a claimed routing attempt.
    pub async fn complete_routing(
        &self,
        id: &str,
        state: &str,
        task_id: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<()> {
        if !matches!(state, "routed" | "ignored" | "needs_attention" | "failed") {
            bail!("invalid terminal routing state: {state}");
        }
        // Only a failure earns another attempt, and it waits 30 seconds for it.
        let next_attempt_at = (state == "failed").then(|| {
            chrono::Utc::now()
                .checked_add_signed(chrono::Duration::seconds(30))
                .unwrap_or_else(chrono::Utc::now)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        });
        let closed = store::complete_routing(
            &self.db,
            id.to_owned(),
            state.to_owned(),
            task_id.map(str::to_owned),
            error_code.map(str::to_owned),
            next_attempt_at,
            now_ts(),
        )
        .await?;
        if !closed {
            bail!("source event is not in routing state");
        }
        Ok(())
    }

    /// Transfers a matched reaction from the delivery claim to the independent
    /// durable automation route worker.
    pub async fn defer_to_automation(&self, id: &str, route_id: &str) -> Result<()> {
        let deferred =
            store::defer_to_automation(&self.db, id.to_owned(), route_id.to_owned(), now_ts())
                .await?;
        if !deferred {
            bail!("source event is not in routing state");
        }
        Ok(())
    }

    /// Projects one terminal automation route outcome onto every provider
    /// delivery attached to the same stable automation identity.
    pub async fn complete_automation_route(
        &self,
        route_id: &str,
        state: &str,
        task_id: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<()> {
        if !matches!(state, "routed" | "ignored" | "needs_attention" | "failed") {
            bail!("invalid automation terminal state: {state}");
        }
        store::complete_automation_route(
            &self.db,
            route_id.to_owned(),
            state.to_owned(),
            task_id.map(str::to_owned),
            error_code.map(str::to_owned),
            now_ts(),
        )
        .await
    }

    /// Returns every delivery attached to a replayed route to the independent
    /// automation-pending projection without consuming a delivery retry.
    pub async fn requeue_automation_route(&self, route_id: &str) -> Result<()> {
        store::requeue_automation_route(&self.db, route_id.to_owned()).await
    }

    /// Requeues a failed or attention-blocked event for administrator replay.
    pub async fn replay(&self, id: &str) -> Result<()> {
        if !store::replay_event(&self.db, id.to_owned()).await? {
            bail!("source event is not replayable");
        }
        Ok(())
    }

    /// Creates or returns a null-safe correlation binding.
    pub async fn create_binding(&self, input: CreateSourceBinding) -> Result<SourceBinding> {
        if !matches!(
            input.binding_type.as_str(),
            "primary" | "related" | "notification_target" | "automation"
        ) {
            bail!("unsupported binding_type");
        }
        let base_key =
            correlation_key(input.conversation_id.as_deref(), input.thread_id.as_deref());
        // A Slack message may deliberately select multiple badge bindings. Primary and
        // related correlations stay exclusive, while each reserved automation route gets
        // its own idempotent binding identity for that same message.
        let key = if input.binding_type == "automation" {
            let event_digest = digest_hex(input.created_by_event_id.as_bytes());
            format!("{base_key}:automation:{}", &event_digest[..24])
        } else {
            base_key
        };
        let digest = digest_hex(
            format!(
                "{}:{}:{}:{}",
                input.provider, input.installation_id, key, input.binding_type
            )
            .as_bytes(),
        );
        let expected_task_id = input.task_id.clone();
        let binding = store::create_binding(
            &self.db,
            store::NewSourceBinding {
                id: format!("bind-{}", &digest[..24]),
                project_id: input.project_id,
                task_id: input.task_id,
                provider: input.provider,
                installation_id: input.installation_id,
                conversation_id: input.conversation_id,
                thread_id: input.thread_id,
                correlation_key: key,
                binding_type: input.binding_type,
                created_by_event_id: input.created_by_event_id,
            },
            now_ts(),
        )
        .await?;
        if binding.task_id != expected_task_id {
            bail!("source correlation is already bound to another task");
        }
        Ok(binding_from_row(binding))
    }

    /// Finds bindings for exact provider conversation coordinates.
    pub async fn find_bindings(
        &self,
        provider: &str,
        installation_id: &str,
        conversation_id: Option<&str>,
        thread_id: Option<&str>,
    ) -> Result<Vec<SourceBinding>> {
        Ok(store::find_bindings(
            &self.db,
            provider.to_owned(),
            installation_id.to_owned(),
            conversation_id.map(str::to_owned),
            thread_id.map(str::to_owned),
        )
        .await?
        .into_iter()
        .map(binding_from_row)
        .collect())
    }

    /// Lists bindings for one task/process.
    pub async fn list_bindings(&self, task_id: &str) -> Result<Vec<SourceBinding>> {
        Ok(store::list_bindings(&self.db, task_id.to_owned())
            .await?
            .into_iter()
            .map(binding_from_row)
            .collect())
    }

    /// Reserves a command audit row. Returns false when the same command already succeeded.
    pub async fn begin_command_action(&self, input: SourceCommandActionInput) -> Result<bool> {
        let digest = digest_hex(
            format!(
                "source-command:{}:{}",
                input.source_event_id, input.idempotency_key
            )
            .as_bytes(),
        );
        let outcome = store::begin_command_action(
            &self.db,
            store::NewCommandAction {
                id: format!("source-action-{}", &digest[..24]),
                source_event_id: input.source_event_id,
                actor: input.actor,
                resolved_role: input.resolved_role,
                target_type: input.target_type,
                target_id: input.target_id,
                action: input.action,
                idempotency_key: input.idempotency_key,
                request_hash: input.request_hash,
                request_id: input.request_id,
            },
            now_ts(),
        )
        .await?;
        match outcome {
            store::CommandActionStart::Started | store::CommandActionStart::Restarted => Ok(true),
            store::CommandActionStart::AlreadySucceeded => Ok(false),
            store::CommandActionStart::RequestMismatch => {
                bail!("source command idempotency key was reused with a different request")
            }
        }
    }

    /// Completes a previously reserved source command audit row.
    pub async fn complete_command_action(
        &self,
        source_event_id: &str,
        idempotency_key: &str,
        status: &str,
        result: Option<&serde_json::Value>,
        error_code: Option<&str>,
    ) -> Result<()> {
        let result_json = result.map(serde_json::to_string).transpose()?;
        let completed = store::complete_command_action(
            &self.db,
            source_event_id.to_owned(),
            idempotency_key.to_owned(),
            status.to_owned(),
            result_json,
            error_code.map(str::to_owned),
            now_ts(),
        )
        .await?;
        if !completed {
            bail!("source command audit reservation missing");
        }
        Ok(())
    }
}

/// Parses a stored row back into a record, including its normalized payload.
///
/// The parse lives here rather than in the row reader because
/// `NormalizedSourceEvent` is this module's type; down in the store a
/// `serde_json` failure had to be reported as a column-conversion failure to
/// satisfy the row mapper's signature.
fn record_from_row(row: store::SourceEventRow) -> Result<SourceEventRecord> {
    let normalized = serde_json::from_str(&row.normalized_payload_json)
        .with_context(|| format!("parse normalized payload of source event {}", row.id))?;
    Ok(SourceEventRecord {
        id: row.id,
        project_id: row.project_id,
        provider: row.provider,
        installation_id: row.installation_id,
        external_event_id: row.external_event_id,
        event_type: row.event_type,
        external_actor_id: row.external_actor_id,
        conversation_id: row.conversation_id,
        thread_id: row.thread_id,
        occurred_at: row.occurred_at,
        received_at: row.received_at,
        normalized,
        payload_hash: row.payload_hash,
        routing_state: row.routing_state,
        routing_attempts: row.routing_attempts,
        routed_task_id: row.routed_task_id,
        last_error_code: row.last_error_code,
    })
}

fn binding_from_row(row: store::SourceBindingRow) -> SourceBinding {
    SourceBinding {
        id: row.id,
        project_id: row.project_id,
        task_id: row.task_id,
        provider: row.provider,
        installation_id: row.installation_id,
        conversation_id: row.conversation_id,
        thread_id: row.thread_id,
        binding_type: row.binding_type,
        created_by_event_id: row.created_by_event_id,
        created_at: row.created_at,
    }
}

/// Returns a deterministic task ID for crash-safe source routing.
pub fn deterministic_task_id(source_event_id: &str) -> String {
    let digest = digest_hex(format!("source-task:{source_event_id}").as_bytes());
    format!("source-{}", &digest[..24])
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 {
        bail!("{label} must contain 1-256 characters");
    }
    Ok(())
}

/// The contract an incoming event must satisfy before it is worth storing.
fn validate_ingest(input: &IngestSourceEvent) -> Result<()> {
    validate_identifier("project_id", &input.project_id)?;
    validate_identifier("provider", &input.event.provider)?;
    validate_identifier("installation_id", &input.event.installation_id)?;
    validate_identifier("external_event_id", &input.event.external_event_id)?;
    validate_identifier("external_actor_id", &input.event.actor.external_id)?;
    if input.event.command.is_some() && input.event.kind != SourceEventKind::Command {
        bail!("source command requires kind=command");
    }
    match (&input.event.kind, &input.event.reaction) {
        (SourceEventKind::ReactionAdded, Some(reaction)) => validate_reaction(reaction)?,
        (SourceEventKind::ReactionAdded, None) => {
            bail!("source reaction_added requires reaction metadata")
        }
        (_, Some(_)) => bail!("source reaction metadata requires kind=reaction_added"),
        _ => {}
    }
    Ok(())
}

fn validate_reaction(reaction: &SourceReactionRef) -> Result<()> {
    if reaction.name.is_empty()
        || reaction.name.len() > 128
        || !reaction.name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '+' | '-')
        })
    {
        bail!(
            "source reaction name must contain 1-128 ASCII alphanumeric, '_', '+', or '-' characters"
        );
    }
    if reaction.target.kind.is_empty()
        || reaction.target.kind.len() > 64
        || !reaction.target.kind.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-')
        })
    {
        bail!(
            "source reaction target kind must contain 1-64 lowercase alphanumeric, '_', or '-' characters"
        );
    }
    validate_identifier(
        "source reaction target external_id",
        &reaction.target.external_id,
    )?;
    if reaction.target.url.is_some() {
        bail!("source reaction target URL must be resolved by a provider adapter");
    }
    Ok(())
}

fn stable_source_id(provider: &str, installation_id: &str, external_event_id: &str) -> String {
    let digest = digest_hex(format!("{provider}:{installation_id}:{external_event_id}").as_bytes());
    format!("src-{}", &digest[..24])
}

fn correlation_key(conversation_id: Option<&str>, thread_id: Option<&str>) -> String {
    format!(
        "{}:{}",
        conversation_id.unwrap_or("-"),
        thread_id.unwrap_or("-")
    )
}

fn digest_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use tempfile::tempdir;

    fn fixture(external_event_id: &str) -> IngestSourceEvent {
        IngestSourceEvent {
            project_id: "default".into(),
            event: NormalizedSourceEvent {
                provider: "fixture".into(),
                installation_id: "test-installation".into(),
                external_event_id: external_event_id.into(),
                kind: SourceEventKind::Message,
                reaction: None,
                actor: ExternalActorRef {
                    external_id: "actor-1".into(),
                    display_name: None,
                },
                conversation: Some(ConversationRef {
                    conversation_id: "conversation-1".into(),
                    thread_id: Some("thread-1".into()),
                    top_level: false,
                }),
                text_summary: Some("bounded context".into()),
                command: None,
                attachments: Vec::new(),
                occurred_at: "2026-07-14T00:00:00Z".into(),
            },
            payload_hash: "hash-1".into(),
            raw_payload_ref: None,
        }
    }

    fn reaction_fixture(external_event_id: &str, target_kind: &str) -> IngestSourceEvent {
        let mut input = fixture(external_event_id);
        input.event.kind = SourceEventKind::ReactionAdded;
        input.event.reaction = Some(SourceReactionRef {
            name: "agent_docs".into(),
            target: ExternalArtifactRef {
                kind: target_kind.into(),
                external_id: "conversation-1:1700000000.000001".into(),
                url: None,
            },
        });
        input.event.text_summary = None;
        input
    }

    async fn repository() -> (tempfile::TempDir, AsyncSourceRepository) {
        let temp = tempdir().expect("temp dir");
        let path = temp.path().join("source.db");
        init_schema(&path).expect("schema");
        let conn = orchestrator_persistence::test_support::open_conn(&path).expect("connection");
        conn.execute(
            "INSERT INTO tasks
             (id,name,status,goal,target_files_json,mode,project_id,workspace_id,workflow_id,
              workspace_root,qa_targets_json,ticket_dir,execution_plan_json,loop_mode,
              current_cycle,init_done,created_at,updated_at,spawn_depth,step_filter_json,
              initial_vars_json,artifacts_dir)
             VALUES ('task-1','fixture','created','fixture','[]','','default','default','default',
              '.','[]','docs/ticket','{}','once',0,0,datetime('now'),datetime('now'),0,'','',''),
             ('task-2','fixture-2','created','fixture','[]','','default','default','default',
              '.','[]','docs/ticket','{}','once',0,0,datetime('now'),datetime('now'),0,'','','')",
            [],
        )
        .expect("seed task");
        let db = Arc::new(AsyncDatabase::open(&path).await.expect("database"));
        (temp, AsyncSourceRepository::new(db))
    }

    #[tokio::test]
    async fn duplicate_ingest_returns_one_row() {
        let (_temp, repo) = repository().await;
        let first = repo.ingest(fixture("event-1")).await.expect("first ingest");
        let second = repo
            .ingest(fixture("event-1"))
            .await
            .expect("replay ingest");
        assert!(first.inserted);
        assert!(!second.inserted);
        assert_eq!(first.event.id, second.event.id);
        assert_eq!(
            repo.list(None, None, None, 10).await.expect("list").len(),
            1
        );
    }

    #[tokio::test]
    async fn reaction_ingest_round_trips_provider_neutral_metadata() {
        let (_temp, repo) = repository().await;
        let first = repo
            .ingest(reaction_fixture("reaction-1", "message"))
            .await
            .expect("reaction ingest");
        let duplicate = repo
            .ingest(reaction_fixture("reaction-1", "message"))
            .await
            .expect("reaction replay");
        assert!(first.inserted);
        assert!(!duplicate.inserted);
        assert_eq!(first.event.event_type, "reaction_added");
        assert_eq!(first.event.normalized.kind, SourceEventKind::ReactionAdded);
        assert_eq!(
            first
                .event
                .normalized
                .reaction
                .as_ref()
                .expect("reaction")
                .target
                .kind,
            "message"
        );
        let json = serde_json::to_value(&first.event.normalized).expect("normalized JSON");
        assert_eq!(json["reaction"]["name"], "agent_docs");
        assert!(json.get("slack").is_none());
    }

    #[tokio::test]
    async fn reaction_contract_rejects_missing_mismatched_or_unsafe_metadata() {
        let (_temp, repo) = repository().await;

        let mut missing = reaction_fixture("reaction-missing", "message");
        missing.event.reaction = None;
        assert!(
            repo.ingest(missing)
                .await
                .expect_err("missing reaction")
                .to_string()
                .contains("requires reaction metadata")
        );

        let mut mismatched = fixture("reaction-mismatched");
        mismatched.event.reaction = reaction_fixture("unused", "message").event.reaction;
        assert!(
            repo.ingest(mismatched)
                .await
                .expect_err("mismatched reaction")
                .to_string()
                .contains("requires kind=reaction_added")
        );

        let mut unsafe_name = reaction_fixture("reaction-unsafe-name", "message");
        unsafe_name.event.reaction.as_mut().expect("reaction").name = ":agent docs:".into();
        assert!(
            repo.ingest(unsafe_name)
                .await
                .expect_err("unsafe name")
                .to_string()
                .contains("source reaction name")
        );

        let mut pre_resolved_url = reaction_fixture("reaction-url", "message");
        pre_resolved_url
            .event
            .reaction
            .as_mut()
            .expect("reaction")
            .target
            .url = Some("https://example.invalid/message".into());
        assert!(
            repo.ingest(pre_resolved_url)
                .await
                .expect_err("pre-resolved URL")
                .to_string()
                .contains("must be resolved by a provider adapter")
        );
    }

    #[test]
    fn normalized_message_without_reaction_field_remains_compatible() {
        let input = fixture("legacy-message");
        let mut json = serde_json::to_value(&input.event).expect("serialize");
        json.as_object_mut().expect("object").remove("reaction");
        let decoded: NormalizedSourceEvent = serde_json::from_value(json).expect("legacy decode");
        assert_eq!(decoded.kind, SourceEventKind::Message);
        assert!(decoded.reaction.is_none());
    }

    #[tokio::test]
    async fn reused_external_id_with_new_payload_fails_closed() {
        let (_temp, repo) = repository().await;
        repo.ingest(fixture("event-1")).await.expect("first ingest");
        let mut changed = fixture("event-1");
        changed.payload_hash = "different".into();
        let error = repo.ingest(changed).await.expect_err("payload mismatch");
        assert!(error.to_string().contains("different payload"));
    }

    #[tokio::test]
    async fn binding_correlation_is_idempotent_and_task_safe() {
        let (_temp, repo) = repository().await;
        let event = repo.ingest(fixture("event-1")).await.expect("ingest").event;
        let input = CreateSourceBinding {
            project_id: "default".into(),
            task_id: "task-1".into(),
            provider: "fixture".into(),
            installation_id: "test-installation".into(),
            conversation_id: Some("conversation-1".into()),
            thread_id: Some("thread-1".into()),
            binding_type: "primary".into(),
            created_by_event_id: event.id,
        };
        let first = repo.create_binding(input.clone()).await.expect("binding");
        let second = repo.create_binding(input).await.expect("repeat binding");
        assert_eq!(first.id, second.id);
        let found = repo
            .find_bindings(
                "fixture",
                "test-installation",
                Some("conversation-1"),
                Some("thread-1"),
            )
            .await
            .expect("find");
        assert_eq!(found.len(), 1);
    }

    #[tokio::test]
    async fn automation_bindings_allow_distinct_badges_on_one_message() {
        let (_temp, repo) = repository().await;
        let first_event = repo
            .ingest(reaction_fixture("reaction-eyes", "message"))
            .await
            .expect("first reaction")
            .event;
        let second_event = repo
            .ingest(reaction_fixture("reaction-check", "message"))
            .await
            .expect("second reaction")
            .event;
        let first_input = CreateSourceBinding {
            project_id: "default".into(),
            task_id: "task-1".into(),
            provider: "fixture".into(),
            installation_id: "test-installation".into(),
            conversation_id: Some("conversation-1".into()),
            thread_id: Some("thread-1".into()),
            binding_type: "automation".into(),
            created_by_event_id: first_event.id,
        };
        let second_input = CreateSourceBinding {
            task_id: "task-2".into(),
            created_by_event_id: second_event.id,
            ..first_input.clone()
        };

        let first = repo
            .create_binding(first_input.clone())
            .await
            .expect("first automation binding");
        let repeated = repo
            .create_binding(first_input)
            .await
            .expect("idempotent automation binding");
        let second = repo
            .create_binding(second_input)
            .await
            .expect("second automation binding");

        assert_eq!(first.id, repeated.id);
        assert_ne!(first.id, second.id);
        let found = repo
            .find_bindings(
                "fixture",
                "test-installation",
                Some("conversation-1"),
                Some("thread-1"),
            )
            .await
            .expect("find same-message automations");
        assert_eq!(found.len(), 2);
        assert_eq!(
            found
                .iter()
                .map(|binding| binding.task_id.as_str())
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["task-1", "task-2"])
        );
    }

    #[tokio::test]
    async fn routing_claim_and_completion_are_stateful() {
        let (_temp, repo) = repository().await;
        let event = repo.ingest(fixture("event-1")).await.expect("ingest").event;
        let claimed = repo.claim_pending(10).await.expect("claim");
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].routing_state, "routing");
        repo.complete_routing(&event.id, "routed", Some("task-1"), None)
            .await
            .expect("complete");
        let loaded = repo.get(&event.id).await.expect("get").expect("event");
        assert_eq!(loaded.routing_state, "routed");
        assert_eq!(loaded.routed_task_id.as_deref(), Some("task-1"));
        assert!(
            repo.claim_pending(10)
                .await
                .expect("second claim")
                .is_empty()
        );
    }

    #[test]
    fn deterministic_task_identity_is_stable() {
        assert_eq!(
            deterministic_task_id("src-1"),
            deterministic_task_id("src-1")
        );
        assert_ne!(
            deterministic_task_id("src-1"),
            deterministic_task_id("src-2")
        );
    }
}
