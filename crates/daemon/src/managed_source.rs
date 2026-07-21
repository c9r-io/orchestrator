//! Outbound durable delivery reconciliation for managed SourceConnections.

use agent_orchestrator::attention::{AttentionCandidate, AttentionSeverity};
use agent_orchestrator::source::{
    AsyncSourceRepository, ConversationRef, ExternalActorRef, ExternalArtifactRef,
    IngestSourceEvent, NormalizedSourceEvent, SourceEventKind, SourceReactionRef,
};
use agent_orchestrator::source_connection::{
    ActivateSourceConnection, AsyncSourceConnectionRepository, SourceConnection,
    SourceConnectionMode, SourceConnectionState,
};
use agent_orchestrator::state::InnerState;
use anyhow::{Context, Result, bail};
use chrono::SecondsFormat;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::slack_gateway::{GatewayDelivery, GatewayEvent, SlackGatewayClient};

/// Runs the optional managed source delivery loop until daemon shutdown.
pub(crate) async fn run(
    state: Arc<InnerState>,
    gateway: Arc<SlackGatewayClient>,
    config_mutation_lock: Arc<tokio::sync::Mutex<()>>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(error) = reconcile_all(&state, &gateway, &config_mutation_lock).await {
                    tracing::warn!(error_code = %stable_error(&error), "managed Slack delivery reconciliation failed");
                }
            }
            _ = shutdown.changed() => return,
        }
    }
}

async fn reconcile_all(
    state: &Arc<InnerState>,
    gateway: &SlackGatewayClient,
    config_mutation_lock: &Arc<tokio::sync::Mutex<()>>,
) -> Result<()> {
    adopt_ownership_transfers(state, gateway, config_mutation_lock).await?;
    let active = agent_orchestrator::config_load::read_active_config(state)?;
    let repository = AsyncSourceConnectionRepository::new(state.async_database.clone());
    for project_id in active.config.projects.keys() {
        let connections = repository
            .list(project_id, Some("slack"), false, 500)
            .await?;
        for connection in connections
            .into_iter()
            .filter(|value| value.state == SourceConnectionState::Active)
        {
            if let Err(error) = reconcile_connection(state, gateway, &repository, &connection).await
            {
                tracing::warn!(
                    connection_id = %connection.id,
                    project_id = %connection.project_id,
                    error_code = %stable_error(&error),
                    "managed Slack connection delivery failed"
                );
            }
        }
    }
    Ok(())
}

async fn adopt_ownership_transfers(
    state: &Arc<InnerState>,
    gateway: &SlackGatewayClient,
    config_mutation_lock: &Arc<tokio::sync::Mutex<()>>,
) -> Result<()> {
    let repository = AsyncSourceConnectionRepository::new(state.async_database.clone());
    let daemon_id = repository.daemon_id().await?;
    let transfers = gateway.claim_ownership_transfers(&daemon_id).await?;
    if transfers.is_empty() {
        return Ok(());
    }
    let keyring =
        agent_orchestrator::secret_key_lifecycle::load_keyring(&state.data_dir, &state.db_path)?;
    let encryption =
        agent_orchestrator::secret_store_crypto::SecretEncryption::from_keyring(&keyring)?;
    for transfer in transfers {
        let installation = transfer.installation;
        if installation.owner_daemon_id != daemon_id
            || installation.state != "active"
            || installation.version < 1
            || installation.generation < 1
            || installation.last_acked_cursor < 0
        {
            tracing::warn!(
                installation_id = %installation.id,
                error_code = "ownership_transfer_projection_invalid",
                "managed Slack ownership transfer rejected"
            );
            continue;
        }
        let connection_id = format!("conn-{}", installation.id);
        let trigger_name = match crate::server::ensure_default_trigger(
            state,
            config_mutation_lock,
            &installation.owner_project_id,
            &connection_id,
            &installation.id,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    installation_id = %installation.id,
                    project_id = %installation.owner_project_id,
                    error_code = "ownership_transfer_trigger_failed",
                    error = %error,
                    "managed Slack ownership transfer adoption deferred"
                );
                continue;
            }
        };
        let pairing_secret_ciphertext = encryption.encrypt_source_connection_credential(
            &installation.owner_project_id,
            &connection_id,
            &transfer.pairing_secret,
        )?;
        repository
            .activate(ActivateSourceConnection {
                id: connection_id,
                project_id: installation.owner_project_id.clone(),
                provider: "slack".to_string(),
                display_label: "Transferred Slack workspace".to_string(),
                provisioning_mode: if installation.provisioning_mode == "managed_dedicated" {
                    SourceConnectionMode::ManagedDedicated
                } else {
                    SourceConnectionMode::ManagedShared
                },
                app_ownership: if installation.provisioning_mode == "managed_dedicated" {
                    "workspace".into()
                } else {
                    "orchestrator".into()
                },
                app_id_digest: installation.app_id_digest,
                manifest_version: installation.manifest_version,
                provision_state: (installation.provisioning_mode == "managed_dedicated")
                    .then(|| "completed".into()),
                provision_error_code: None,
                installation_id: installation.id.clone(),
                installation_id_digest: installation.team_digest,
                enterprise_id_digest: installation.enterprise_digest,
                owner_daemon_id: daemon_id.clone(),
                generation: installation.generation,
                version: installation.version,
                last_acked_cursor: installation.last_acked_cursor,
                capabilities: vec!["delivery_v1".into(), "permalink_proxy".into()],
                scopes: installation.scopes,
                trigger_name: Some(trigger_name),
                gateway_origin: Some(gateway.origin().to_string()),
                pairing_secret_ciphertext: Some(pairing_secret_ciphertext),
                request_id: format!("req-transfer-adopt-{}", installation.id),
            })
            .await?;
        gateway
            .acknowledge_ownership_transfer(&installation.id, &daemon_id, &transfer.pairing_secret)
            .await?;
        tracing::info!(
            installation_id = %installation.id,
            project_id = %installation.owner_project_id,
            "managed Slack ownership transfer adopted"
        );
    }
    Ok(())
}

async fn reconcile_connection(
    state: &Arc<InnerState>,
    gateway: &SlackGatewayClient,
    repository: &AsyncSourceConnectionRepository,
    connection: &SourceConnection,
) -> Result<()> {
    let daemon_id = repository.daemon_id().await?;
    if connection.owner_daemon_id != daemon_id {
        bail!("source_connection_owner_mismatch");
    }
    let credential = repository
        .credential(&connection.project_id, &connection.id, &daemon_id)
        .await?
        .context("source_connection_credential_missing")?;
    let keyring =
        agent_orchestrator::secret_key_lifecycle::load_keyring(&state.data_dir, &state.db_path)?;
    let encryption =
        agent_orchestrator::secret_store_crypto::SecretEncryption::from_keyring(&keyring)?;
    let pairing = encryption.decrypt_source_connection_credential(
        &connection.project_id,
        &connection.id,
        &credential.pairing_secret_ciphertext,
    )?;
    let deliveries = gateway
        .claim(
            &connection.installation_id,
            &daemon_id,
            &pairing,
            connection.last_acked_cursor,
        )
        .await?;
    if deliveries.is_empty() {
        return Ok(());
    }
    let source = AsyncSourceRepository::new(state.async_database.clone());
    let mut revoked = false;
    let mut cursors = Vec::with_capacity(deliveries.len());
    for delivery in &deliveries {
        validate_delivery(connection, delivery)?;
        if delivery.event.event_type == "installation_revoked" {
            revoked = true;
        } else {
            ingest_reaction(&source, connection, &delivery.event).await?;
        }
        cursors.push(delivery.cursor);
    }
    let cursor = gateway
        .acknowledge(&connection.installation_id, &daemon_id, &pairing, &cursors)
        .await?;
    repository
        .record_delivery(&connection.project_id, &connection.id, cursor, 0)
        .await?;
    if revoked {
        let latest = repository
            .get(&connection.project_id, &connection.id)
            .await?
            .context("SourceConnection disappeared after revocation delivery")?;
        repository
            .transition(
                &connection.project_id,
                &connection.id,
                latest.version,
                SourceConnectionState::Revoked,
                Some("slack_credential_revoked"),
                &format!("req-gateway-revocation-{cursor}"),
            )
            .await?;
        state
            .attention_repo
            .upsert_external_candidate(revocation_attention(&latest, cursor))
            .await?;
    }
    Ok(())
}

fn revocation_attention(connection: &SourceConnection, cursor: i64) -> AttentionCandidate {
    let dedupe_key = format!("source-connection-revoked:{}", connection.id);
    let digest = hex::encode(Sha256::digest(dedupe_key.as_bytes()));
    let occurred_at = agent_orchestrator::config_load::now_ts();
    AttentionCandidate {
        id: format!("attention-source-connection-{}", &digest[..24]),
        project_id: connection.project_id.clone(),
        task_id: String::new(),
        task_item_id: None,
        step_id: None,
        session_id: None,
        kind: "source_connection_revoked".into(),
        severity: AttentionSeverity::Intervention,
        title: "Slack connection needs reauthorization".into(),
        summary: "Slack revoked this managed connection. Reauthorize it before resuming source automation.".into(),
        requested_decision: Some(serde_json::json!({
            "connection_id": connection.id,
            "safe_error_code": "slack_credential_revoked"
        })),
        actions: vec![],
        dedupe_key,
        source_event_id: format!("gateway-revocation:{cursor}"),
        source_route_id: None,
        source_binding_name: None,
        occurred_at,
        sla_deadline: None,
    }
}

fn validate_delivery(connection: &SourceConnection, delivery: &GatewayDelivery) -> Result<()> {
    if delivery.cursor <= connection.last_acked_cursor
        || delivery.delivery_id.is_empty()
        || delivery.lease_expires_at.is_empty()
        || delivery.event.installation_id != connection.installation_id
        || delivery.event.team_digest != connection.installation_id_digest
        || delivery.event.enterprise_digest != connection.enterprise_id_digest
    {
        bail!("managed_delivery_identity_mismatch");
    }
    if !matches!(
        delivery.event.event_type.as_str(),
        "reaction_added" | "installation_revoked"
    ) {
        bail!("managed_delivery_event_unsupported");
    }
    Ok(())
}

async fn ingest_reaction(
    repository: &AsyncSourceRepository,
    connection: &SourceConnection,
    event: &GatewayEvent,
) -> Result<()> {
    let normalized = normalize_reaction(&connection.installation_id, event)?;
    let encoded = serde_json::to_vec(&normalized)?;
    repository
        .ingest(IngestSourceEvent {
            project_id: connection.project_id.clone(),
            event: normalized,
            payload_hash: hex::encode(Sha256::digest(encoded)),
            raw_payload_ref: None,
        })
        .await?;
    Ok(())
}

fn normalize_reaction(
    installation_id: &str,
    event: &GatewayEvent,
) -> Result<NormalizedSourceEvent> {
    let actor = event
        .external_actor_id
        .as_ref()
        .context("managed_reaction_actor_missing")?;
    let reaction = event
        .reaction
        .as_ref()
        .context("managed_reaction_name_missing")?;
    let channel = event
        .channel_id
        .as_ref()
        .context("managed_reaction_channel_missing")?;
    let message_ts = event
        .message_ts
        .as_ref()
        .context("managed_reaction_timestamp_missing")?;
    let normalized = NormalizedSourceEvent {
        provider: "slack".to_string(),
        installation_id: installation_id.to_string(),
        external_event_id: event.external_event_id.clone(),
        kind: SourceEventKind::ReactionAdded,
        reaction: Some(SourceReactionRef {
            name: reaction.clone(),
            target: ExternalArtifactRef {
                kind: "message".to_string(),
                external_id: format!("{channel}:{message_ts}"),
                url: None,
            },
        }),
        actor: ExternalActorRef {
            external_id: actor.clone(),
            display_name: None,
        },
        conversation: Some(ConversationRef {
            conversation_id: channel.clone(),
            thread_id: Some(message_ts.clone()),
            top_level: true,
        }),
        text_summary: None,
        command: None,
        attachments: vec![],
        occurred_at: slack_timestamp_to_rfc3339(&event.event_ts)?,
    };
    Ok(normalized)
}

fn slack_timestamp_to_rfc3339(value: &str) -> Result<String> {
    let (seconds, fraction) = value
        .split_once('.')
        .map_or((value, ""), |(seconds, fraction)| (seconds, fraction));
    if seconds.is_empty()
        || !seconds.bytes().all(|value| value.is_ascii_digit())
        || fraction.len() > 9
        || !fraction.bytes().all(|value| value.is_ascii_digit())
    {
        bail!("managed_reaction_event_timestamp_invalid");
    }
    let seconds = seconds
        .parse::<i64>()
        .map_err(|_| anyhow::anyhow!("managed_reaction_event_timestamp_invalid"))?;
    let nanos = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<9}")
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("managed_reaction_event_timestamp_invalid"))?
    };
    chrono::DateTime::from_timestamp(seconds, nanos)
        .context("managed_reaction_event_timestamp_invalid")
        .map(|value| value.to_rfc3339_opts(SecondsFormat::AutoSi, true))
}

fn stable_error(error: &anyhow::Error) -> &'static str {
    let value = error.to_string();
    for code in [
        "source_connection_owner_mismatch",
        "source_connection_credential_missing",
        "managed_delivery_identity_mismatch",
        "managed_delivery_event_unsupported",
        "managed_reaction_actor_missing",
        "managed_reaction_name_missing",
        "managed_reaction_channel_missing",
        "managed_reaction_timestamp_missing",
        "managed_reaction_event_timestamp_invalid",
        "ownership_transfer_projection_invalid",
        "ownership_transfer_trigger_failed",
    ] {
        if value.contains(code) {
            return code;
        }
    }
    "managed_delivery_failed"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_reaction_preserves_canonical_message_identity() {
        let event = GatewayEvent {
            external_event_id: "event-1".into(),
            event_type: "reaction_added".into(),
            installation_id: "installation-1".into(),
            external_actor_id: Some("actor-1".into()),
            reaction: Some("eyes".into()),
            channel_id: Some("channel-1".into()),
            message_ts: Some("1700000000.000100".into()),
            event_ts: "1700000001.000100".into(),
            team_digest: "team-digest".into(),
            enterprise_digest: None,
        };

        let normalized = normalize_reaction("installation-1", &event).expect("normalize");
        let conversation = normalized.conversation.expect("conversation");
        assert_eq!(conversation.conversation_id, "channel-1");
        assert_eq!(conversation.thread_id.as_deref(), Some("1700000000.000100"));
        assert_eq!(
            normalized.reaction.expect("reaction").target.external_id,
            "channel-1:1700000000.000100"
        );
    }

    #[test]
    fn connection_revocation_attention_is_stable_and_private() {
        let connection = SourceConnection {
            id: "connection-1".into(),
            project_id: "project-1".into(),
            provider: "slack".into(),
            display_label: "private workspace label".into(),
            provisioning_mode: SourceConnectionMode::ManagedShared,
            app_ownership: "orchestrator".into(),
            app_id_digest: None,
            manifest_version: None,
            provision_state: None,
            provision_error_code: None,
            installation_id: "installation-1".into(),
            installation_id_digest: "team-digest".into(),
            enterprise_id_digest: None,
            owner_daemon_id: "daemon-1".into(),
            generation: 1,
            version: 2,
            state: SourceConnectionState::Revoked,
            capabilities: vec![],
            scopes: vec!["reactions:read".into()],
            trigger_name: Some("trigger-1".into()),
            last_delivery_at: None,
            last_acked_cursor: 7,
            delivery_lag: 0,
            last_error_code: Some("slack_credential_revoked".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:01Z".into(),
            reauthorized_at: None,
            disconnected_at: None,
        };

        let first = revocation_attention(&connection, 7);
        let replay = revocation_attention(&connection, 8);
        assert_eq!(first.id, replay.id);
        assert_eq!(first.dedupe_key, replay.dedupe_key);
        assert_eq!(first.kind, "source_connection_revoked");
        assert!(!first.summary.contains(&connection.display_label));
    }

    #[test]
    fn converts_slack_decimal_timestamp_without_losing_fraction() {
        assert_eq!(
            slack_timestamp_to_rfc3339("1700000001.000100").expect("timestamp"),
            "2023-11-14T22:13:21.000100Z"
        );
        assert!(slack_timestamp_to_rfc3339("not-a-slack-timestamp").is_err());
        assert!(slack_timestamp_to_rfc3339("1700000001.1234567890").is_err());
    }
}
