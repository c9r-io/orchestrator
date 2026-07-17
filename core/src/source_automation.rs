//! Durable source automation reservation and provenance.

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    /// Route creation timestamp.
    pub created_at: String,
    /// Route completion timestamp.
    pub completed_at: Option<String>,
}

/// Result of reserving an automation identity.
#[derive(Debug, Clone)]
pub struct SourceAutomationReservation {
    /// Durable route.
    pub route: SourceAutomationRoute,
    /// True only for the worker that owns mutation execution.
    pub should_execute: bool,
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

    /// Reclaims a failed or stale non-terminal route after daemon restart.
    pub async fn claim_existing(&self, id: &str) -> Result<SourceAutomationReservation> {
        let id = id.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                (|| -> Result<SourceAutomationReservation> {
                    let now = now_ts();
                    let should_execute = conn.execute(
                        "UPDATE source_automation_routes SET status='reserved',lease_claimed_at=?2,
                         updated_at=?2,error_code=NULL WHERE id=?1 AND
                         (status='failed' OR (status!='completed' AND lease_claimed_at < datetime('now','-5 minutes')))",
                        params![id, now],
                    )? == 1;
                    let route = read_route(conn, &id)?.context("automation route missing")?;
                    Ok(SourceAutomationReservation {
                        route,
                        should_execute,
                    })
                })()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Stores a validated permalink before rendering.
    pub async fn record_permalink(&self, id: &str, permalink: &str) -> Result<()> {
        let id = id.to_owned();
        let permalink = permalink.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE source_automation_routes SET permalink_status='resolved',permalink=?2,
                     status='rendering',error_code=NULL,updated_at=?3 WHERE id=?1 AND status IN ('reserved','resolving','failed')",
                    params![id, permalink, now_ts()],
                )?;
                if changed != 1 {
                    return Err(other(anyhow::anyhow!("automation route is not awaiting permalink")));
                }
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    /// Completes a route with its canonical task.
    pub async fn complete(&self, id: &str, task_id: &str) -> Result<()> {
        let id = id.to_owned();
        let task_id = task_id.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE source_automation_routes SET task_id=?2,status='completed',error_code=NULL,
                     lease_claimed_at=NULL,updated_at=?3,completed_at=?3 WHERE id=?1",
                    params![id, task_id, now_ts()],
                )?;
                if changed != 1 {
                    return Err(other(anyhow::anyhow!("automation route missing")));
                }
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    /// Records a stable provider or mutation error and releases the lease.
    pub async fn fail(&self, id: &str, error_code: &str, retry_after: Option<&str>) -> Result<()> {
        let id = id.to_owned();
        let error_code = error_code.to_owned();
        let retry_after = retry_after.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE source_automation_routes SET status='failed',error_code=?2,retry_after=?3,
                     lease_claimed_at=NULL,updated_at=?4 WHERE id=?1 AND status!='completed'",
                    params![id, error_code, retry_after, now_ts()],
                )?;
                if changed != 1 {
                    return Err(other(anyhow::anyhow!("automation route missing or completed")));
                }
                Ok(())
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
          deterministic_task_id,status,lease_claimed_at,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,
                 'reserved',?22,?22,?22)",
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
    let should_execute = if inserted {
        true
    } else {
        tx.execute(
            "UPDATE source_automation_routes SET status='reserved',lease_claimed_at=?2,updated_at=?2,
             error_code=NULL WHERE id=?1 AND status='failed'",
            params![id, now],
        )? == 1
            || tx.execute(
                "UPDATE source_automation_routes SET lease_claimed_at=?2,updated_at=?2
                 WHERE id=?1 AND status!='completed' AND lease_claimed_at < datetime('now','-5 minutes')",
                params![id, now],
            )? == 1
    };
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
         deterministic_task_id,task_id,status,error_code,created_at,completed_at
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
                created_at: row.get(22)?,
                completed_at: row.get(23)?,
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
        "SELECT binding_snapshot_json,template_snapshot_json,credential_store,credential_key
         FROM source_automation_routes WHERE id=?1",
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
    use crate::source::{
        ExternalActorRef, IngestSourceEvent, NormalizedSourceEvent, SourceEventKind,
    };
    use crate::test_utils::TestState;

    fn input(event_id: String) -> ReserveSourceAutomationRoute {
        ReserveSourceAutomationRoute {
            project_id: "demo".into(),
            source_event_id: event_id,
            provider: "slack".into(),
            installation_id: "T1".into(),
            message_identity: "C1:1.23".into(),
            channel_id: "C1".into(),
            message_ts: "1.23".into(),
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

    #[tokio::test]
    async fn duplicate_delivery_reserves_one_automation_identity() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let source = crate::source::AsyncSourceRepository::new(state.async_database.clone());
        let event = source
            .ingest(IngestSourceEvent {
                project_id: "demo".into(),
                event: NormalizedSourceEvent {
                    provider: "slack".into(),
                    installation_id: "T1".into(),
                    external_event_id: "Ev1".into(),
                    kind: SourceEventKind::ReactionAdded,
                    reaction: Some(crate::source::SourceReactionRef {
                        name: "agent-analyze".into(),
                        target: crate::source::ExternalArtifactRef {
                            kind: "message".into(),
                            external_id: "1.23".into(),
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
                payload_hash: "hash".into(),
                raw_payload_ref: None,
            })
            .await
            .expect("ingest")
            .event;
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
}
