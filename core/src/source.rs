//! Provider-neutral external source event and process-binding persistence.

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
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
    /// Provider lifecycle notification with no process mutation.
    System,
}

impl SourceEventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Command => "command",
            Self::Artifact => "artifact",
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
    /// Binding type (`primary`, `related`, or `notification_target`).
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
        self.db
            .writer()
            .call(move |conn| ingest(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Returns one source event.
    pub async fn get(&self, id: &str) -> Result<Option<SourceEventRecord>> {
        let id = id.to_owned();
        self.db
            .reader()
            .call(move |conn| read_event(conn, &id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Lists recent events, optionally scoped to a task or routing state.
    pub async fn list(
        &self,
        project_id: Option<&str>,
        task_id: Option<&str>,
        routing_state: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SourceEventRecord>> {
        let project_id = project_id.map(str::to_owned);
        let task_id = task_id.map(str::to_owned);
        let routing_state = routing_state.map(str::to_owned);
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM source_events
                     WHERE (?1 IS NULL OR project_id=?1)
                       AND (?2 IS NULL OR routed_task_id=?2)
                       AND (?3 IS NULL OR routing_state=?3)
                     ORDER BY received_at DESC, id DESC LIMIT ?4",
                )?;
                let ids = stmt
                    .query_map(
                        params![
                            project_id,
                            task_id,
                            routing_state,
                            limit.clamp(1, 500) as i64
                        ],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ids.into_iter()
                    .map(|id| read_event(conn, &id)?.context("source event missing"))
                    .collect::<Result<Vec<_>>>()
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Atomically claims a bounded routing batch.
    pub async fn claim_pending(&self, limit: usize) -> Result<Vec<SourceEventRecord>> {
        self.db
            .writer()
            .call(move |conn| claim_pending(conn, limit).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Completes a claimed routing attempt.
    pub async fn complete_routing(
        &self,
        id: &str,
        state: &str,
        task_id: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<()> {
        let id = id.to_owned();
        let state = state.to_owned();
        let task_id = task_id.map(str::to_owned);
        let error_code = error_code.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                complete_routing(conn, &id, &state, task_id.as_deref(), error_code.as_deref())
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Requeues a failed or attention-blocked event for administrator replay.
    pub async fn replay(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE source_events SET routing_state='received', last_error_code=NULL,
                     next_attempt_at=NULL, routing_attempts=0, routing_claimed_at=NULL
                     WHERE id=?1 AND routing_state IN ('failed','needs_attention')",
                    [&id],
                )?;
                if changed == 0 {
                    return Err(other(anyhow::anyhow!("source event is not replayable")));
                }
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    /// Creates or returns a null-safe correlation binding.
    pub async fn create_binding(&self, input: CreateSourceBinding) -> Result<SourceBinding> {
        self.db
            .writer()
            .call(move |conn| create_binding(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Finds bindings for exact provider conversation coordinates.
    pub async fn find_bindings(
        &self,
        provider: &str,
        installation_id: &str,
        conversation_id: Option<&str>,
        thread_id: Option<&str>,
    ) -> Result<Vec<SourceBinding>> {
        let provider = provider.to_owned();
        let installation_id = installation_id.to_owned();
        let key = correlation_key(conversation_id, thread_id);
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM source_bindings WHERE provider=?1 AND installation_id=?2
                     AND correlation_key=?3 ORDER BY created_at ASC, id ASC",
                )?;
                let ids = stmt
                    .query_map(params![provider, installation_id, key], |row| {
                        row.get::<_, String>(0)
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ids.into_iter()
                    .map(|id| {
                        read_binding(conn, &id)
                            .map_err(other)?
                            .context("binding missing")
                    })
                    .collect::<Result<Vec<_>>>()
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists bindings for one task/process.
    pub async fn list_bindings(&self, task_id: &str) -> Result<Vec<SourceBinding>> {
        let task_id = task_id.to_owned();
        self.db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM source_bindings WHERE task_id=?1 ORDER BY created_at DESC",
                )?;
                let ids = stmt
                    .query_map([task_id], |row| row.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ids.into_iter()
                    .map(|id| {
                        read_binding(conn, &id)
                            .map_err(other)?
                            .context("binding missing")
                    })
                    .collect::<Result<Vec<_>>>()
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Reserves a command audit row. Returns false when the same command already succeeded.
    pub async fn begin_command_action(&self, input: SourceCommandActionInput) -> Result<bool> {
        self.db
            .writer()
            .call(move |conn| begin_command_action(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
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
        let source_event_id = source_event_id.to_owned();
        let idempotency_key = idempotency_key.to_owned();
        let status = status.to_owned();
        let result_json = result.map(serde_json::to_string).transpose()?;
        let error_code = error_code.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE source_command_actions SET status=?3,result_json=?4,error_code=?5,
                     completed_at=?6 WHERE source_event_id=?1 AND idempotency_key=?2",
                    params![
                        source_event_id,
                        idempotency_key,
                        status,
                        result_json,
                        error_code,
                        now_ts()
                    ],
                )?;
                if changed != 1 {
                    return Err(other(anyhow::anyhow!(
                        "source command audit reservation missing"
                    )));
                }
                Ok(())
            })
            .await
            .map_err(flatten_err)
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

fn ingest(conn: &Connection, input: IngestSourceEvent) -> Result<IngestResult> {
    validate_identifier("project_id", &input.project_id)?;
    validate_identifier("provider", &input.event.provider)?;
    validate_identifier("installation_id", &input.event.installation_id)?;
    validate_identifier("external_event_id", &input.event.external_event_id)?;
    validate_identifier("external_actor_id", &input.event.actor.external_id)?;
    if input.event.command.is_some() && input.event.kind != SourceEventKind::Command {
        bail!("source command requires kind=command");
    }
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
    let received_at = now_ts();
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO source_events
         (id,project_id,provider,installation_id,external_event_id,event_type,
          external_actor_id,conversation_id,thread_id,occurred_at,received_at,
          normalized_payload_json,raw_payload_ref,payload_hash,routing_state)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'received')",
        params![
            id,
            input.project_id,
            input.event.provider,
            input.event.installation_id,
            input.event.external_event_id,
            input.event.kind.as_str(),
            input.event.actor.external_id,
            conversation_id,
            thread_id,
            input.event.occurred_at,
            received_at,
            normalized_payload_json,
            input.raw_payload_ref,
            input.payload_hash,
        ],
    )? == 1;
    let event = read_event(conn, &id)?.context("inserted source event missing")?;
    if event.payload_hash != input.payload_hash {
        bail!("external event id was reused with a different payload");
    }
    Ok(IngestResult { event, inserted })
}

fn claim_pending(conn: &Connection, limit: usize) -> Result<Vec<SourceEventRecord>> {
    let tx = conn.unchecked_transaction()?;
    let stale_before = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
    let ids = {
        let mut stmt = tx.prepare(
            "SELECT id FROM source_events
             WHERE (routing_state IN ('received','failed')
                    OR (routing_state='routing' AND routing_claimed_at <= ?2))
               AND routing_attempts < 5
               AND (next_attempt_at IS NULL OR next_attempt_at <= datetime('now'))
             ORDER BY received_at ASC, id ASC LIMIT ?1",
        )?;
        stmt.query_map(params![limit.clamp(1, 100) as i64, stale_before], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for id in &ids {
        tx.execute(
            "UPDATE source_events SET routing_state='routing', routing_attempts=routing_attempts+1,
             last_error_code=NULL,routing_claimed_at=?2 WHERE id=?1",
            params![id, now_ts()],
        )?;
        tx.execute(
            "INSERT INTO source_routing_attempts(source_event_id,attempt_no,result,created_at)
             SELECT id,routing_attempts,'routing',?2 FROM source_events WHERE id=?1",
            params![id, now_ts()],
        )?;
    }
    tx.commit()?;
    ids.into_iter()
        .map(|id| read_event(conn, &id)?.context("claimed source event missing"))
        .collect()
}

fn complete_routing(
    conn: &Connection,
    id: &str,
    state: &str,
    task_id: Option<&str>,
    error_code: Option<&str>,
) -> Result<()> {
    if !matches!(state, "routed" | "ignored" | "needs_attention" | "failed") {
        bail!("invalid terminal routing state: {state}");
    }
    let tx = conn.unchecked_transaction()?;
    let next_attempt_at = (state == "failed").then(|| {
        chrono::Utc::now()
            .checked_add_signed(chrono::Duration::seconds(30))
            .unwrap_or_else(chrono::Utc::now)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    });
    let changed = tx.execute(
        "UPDATE source_events SET routing_state=?2, routed_task_id=COALESCE(?3,routed_task_id),
         last_error_code=?4, next_attempt_at=?5, routing_claimed_at=NULL,
         routed_at=CASE WHEN ?2 IN ('routed','ignored','needs_attention') THEN ?6 ELSE routed_at END
         WHERE id=?1 AND routing_state='routing'",
        params![id, state, task_id, error_code, next_attempt_at, now_ts()],
    )?;
    if changed == 0 {
        bail!("source event is not in routing state");
    }
    tx.execute(
        "UPDATE source_routing_attempts SET result=?2,task_id=?3,error_code=?4,completed_at=?5
         WHERE source_event_id=?1 AND attempt_no=(SELECT routing_attempts FROM source_events WHERE id=?1)",
        params![id, state, task_id, error_code, now_ts()],
    )?;
    tx.commit()?;
    Ok(())
}

fn create_binding(conn: &Connection, input: CreateSourceBinding) -> Result<SourceBinding> {
    if !matches!(
        input.binding_type.as_str(),
        "primary" | "related" | "notification_target"
    ) {
        bail!("unsupported binding_type");
    }
    let key = correlation_key(input.conversation_id.as_deref(), input.thread_id.as_deref());
    let digest = digest_hex(
        format!(
            "{}:{}:{}:{}",
            input.provider, input.installation_id, key, input.binding_type
        )
        .as_bytes(),
    );
    let id = format!("bind-{}", &digest[..24]);
    conn.execute(
        "INSERT OR IGNORE INTO source_bindings
         (id,project_id,task_id,provider,installation_id,conversation_id,thread_id,
          correlation_key,binding_type,created_by_event_id,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id,
            input.project_id,
            input.task_id,
            input.provider,
            input.installation_id,
            input.conversation_id,
            input.thread_id,
            key,
            input.binding_type,
            input.created_by_event_id,
            now_ts(),
        ],
    )?;
    let binding = read_binding(conn, &id)?.context("source binding missing")?;
    if binding.task_id != input.task_id {
        bail!("source correlation is already bound to another task");
    }
    Ok(binding)
}

fn begin_command_action(conn: &Connection, input: SourceCommandActionInput) -> Result<bool> {
    let existing = conn
        .query_row(
            "SELECT request_hash,status FROM source_command_actions
             WHERE source_event_id=?1 AND idempotency_key=?2",
            params![input.source_event_id, input.idempotency_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((request_hash, status)) = existing {
        if request_hash != input.request_hash {
            bail!("source command idempotency key was reused with a different request");
        }
        if status == "succeeded" {
            return Ok(false);
        }
        conn.execute(
            "UPDATE source_command_actions SET status='running',result_json=NULL,error_code=NULL,
             completed_at=NULL WHERE source_event_id=?1 AND idempotency_key=?2",
            params![input.source_event_id, input.idempotency_key],
        )?;
        return Ok(true);
    }
    let digest = digest_hex(
        format!(
            "source-command:{}:{}",
            input.source_event_id, input.idempotency_key
        )
        .as_bytes(),
    );
    conn.execute(
        "INSERT INTO source_command_actions
         (id,source_event_id,actor,resolved_role,target_type,target_id,action,idempotency_key,
          request_hash,status,created_at,request_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'running',?10,?11)",
        params![
            format!("source-action-{}", &digest[..24]),
            input.source_event_id,
            input.actor,
            input.resolved_role,
            input.target_type,
            input.target_id,
            input.action,
            input.idempotency_key,
            input.request_hash,
            now_ts(),
            input.request_id,
        ],
    )?;
    Ok(true)
}

fn read_event(conn: &Connection, id: &str) -> Result<Option<SourceEventRecord>> {
    conn.query_row(
        "SELECT id,project_id,provider,installation_id,external_event_id,event_type,
         external_actor_id,conversation_id,thread_id,occurred_at,received_at,
         normalized_payload_json,payload_hash,routing_state,routing_attempts,
         routed_task_id,last_error_code FROM source_events WHERE id=?1",
        [id],
        |row| {
            let raw: String = row.get(11)?;
            let normalized = serde_json::from_str(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(SourceEventRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                provider: row.get(2)?,
                installation_id: row.get(3)?,
                external_event_id: row.get(4)?,
                event_type: row.get(5)?,
                external_actor_id: row.get(6)?,
                conversation_id: row.get(7)?,
                thread_id: row.get(8)?,
                occurred_at: row.get(9)?,
                received_at: row.get(10)?,
                normalized,
                payload_hash: row.get(12)?,
                routing_state: row.get(13)?,
                routing_attempts: row.get(14)?,
                routed_task_id: row.get(15)?,
                last_error_code: row.get(16)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn read_binding(conn: &Connection, id: &str) -> Result<Option<SourceBinding>> {
    conn.query_row(
        "SELECT id,project_id,task_id,provider,installation_id,conversation_id,thread_id,
         binding_type,created_by_event_id,created_at FROM source_bindings WHERE id=?1",
        [id],
        |row| {
            Ok(SourceBinding {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_id: row.get(2)?,
                provider: row.get(3)?,
                installation_id: row.get(4)?,
                conversation_id: row.get(5)?,
                thread_id: row.get(6)?,
                binding_type: row.get(7)?,
                created_by_event_id: row.get(8)?,
                created_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
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

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
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

    async fn repository() -> (tempfile::TempDir, AsyncSourceRepository) {
        let temp = tempdir().expect("temp dir");
        let path = temp.path().join("source.db");
        init_schema(&path).expect("schema");
        let conn = crate::db::open_conn(&path).expect("connection");
        conn.execute(
            "INSERT INTO tasks
             (id,name,status,goal,target_files_json,mode,project_id,workspace_id,workflow_id,
              workspace_root,qa_targets_json,ticket_dir,execution_plan_json,loop_mode,
              current_cycle,init_done,created_at,updated_at,spawn_depth,step_filter_json,
              initial_vars_json,artifacts_dir)
             VALUES ('task-1','fixture','created','fixture','[]','','default','default','default',
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
