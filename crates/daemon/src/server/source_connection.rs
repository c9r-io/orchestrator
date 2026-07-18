//! Managed SourceConnection gRPC surface and OAuth intent reconciliation.

use agent_orchestrator::source_connection::{
    ActivateSourceConnection, AsyncSourceConnectionRepository,
    DedicatedProvisioning as CoreDedicatedProvisioning, SourceConnection as CoreConnection,
    SourceConnectionIntent as CoreIntent, SourceConnectionMode, SourceConnectionState,
    StoreDedicatedProvisioning, StoreSourceConnectionIntent, TransferSourceConnectionOwner,
    UpdateDedicatedProvisioning,
};
use futures::Stream;
use orchestrator_proto::*;
use orchestrator_slack_gateway::slack::{render_manifest_endpoints, reviewed_manifest_contract};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::OrchestratorServer;
use super::action_audit::{self, ActionDescriptor};

pub(crate) type SourceConnectionWatchStream =
    Pin<Box<dyn Stream<Item = Result<SourceConnectionDelta, Status>> + Send>>;

const DEDICATED_MANIFEST_VERSION: &str = "orchestrator-slack-dedicated-v1";

pub(crate) type DedicatedSessionStore = Arc<Mutex<HashMap<String, DedicatedSession>>>;

pub(crate) struct DedicatedSession {
    project_id: String,
    display_label: String,
    owner_daemon_id: String,
    manifest: serde_json::Value,
    manifest_digest: String,
    config_token: Zeroizing<String>,
    import_secret: Option<Zeroizing<String>>,
    created_credentials: Option<DedicatedCreatedCredentials>,
}

struct DedicatedCreatedCredentials {
    app_id: Zeroizing<String>,
    client_id: Zeroizing<String>,
    client_secret: Zeroizing<String>,
    signing_secret: Zeroizing<String>,
}

pub(crate) async fn list(
    server: &OrchestratorServer,
    request: Request<SourceConnectionListRequest>,
) -> Result<Response<SourceConnectionListResponse>, Status> {
    super::authorize(server, &request, "SourceConnectionList").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    let values = repository(server)
        .list(
            &req.project_id,
            req.provider.as_deref(),
            req.include_disconnected,
            if req.limit == 0 {
                100
            } else {
                req.limit as usize
            },
        )
        .await
        .map_err(internal)?;
    Ok(Response::new(SourceConnectionListResponse {
        connections: values.into_iter().map(connection_to_proto).collect(),
    }))
}

pub(crate) async fn get(
    server: &OrchestratorServer,
    request: Request<SourceConnectionGetRequest>,
) -> Result<Response<SourceConnection>, Status> {
    super::authorize(server, &request, "SourceConnectionGet").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    let connection = repository(server)
        .get(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection not found"))?;
    Ok(Response::new(connection_to_proto(connection)))
}

pub(crate) async fn watch(
    server: &OrchestratorServer,
    request: Request<SourceConnectionWatchRequest>,
) -> Result<Response<SourceConnectionWatchStream>, Status> {
    super::authorize(server, &request, "SourceConnectionWatch").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    let repository = repository(server);
    let interval = std::time::Duration::from_millis(if req.interval_millis == 0 {
        500
    } else {
        req.interval_millis.clamp(250, 5_000) as u64
    });
    let (sender, receiver) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        let mut cursor = req.after_cursor.max(0);
        loop {
            let changes = match repository.changes(&req.project_id, cursor, 200).await {
                Ok(value) => value,
                Err(error) => {
                    let _ = sender.send(Err(internal(error))).await;
                    return;
                }
            };
            for change in changes {
                cursor = change.cursor;
                let connection = match repository.get(&req.project_id, &change.connection_id).await
                {
                    Ok(Some(value)) => value,
                    Ok(None) => continue,
                    Err(error) => {
                        let _ = sender.send(Err(internal(error))).await;
                        return;
                    }
                };
                if sender
                    .send(Ok(SourceConnectionDelta {
                        cursor: change.cursor,
                        connection_version: change.connection_version,
                        state: change.state.as_str().to_string(),
                        error_code: change.error_code,
                        request_id: change.request_id,
                        connection: Some(connection_to_proto(connection)),
                        changed_at: change.created_at,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            tokio::time::sleep(interval).await;
        }
    });
    Ok(Response::new(Box::pin(
        tokio_stream::wrappers::ReceiverStream::new(receiver),
    )))
}

pub(crate) async fn catalog(
    server: &OrchestratorServer,
    request: Request<SourceConnectionCatalogRequest>,
) -> Result<Response<SourceConnectionCatalogResponse>, Status> {
    super::authorize(server, &request, "SourceConnectionCatalogGet").map_err(Status::from)?;
    let Some(gateway) = server.slack_gateway.as_ref() else {
        return Ok(Response::new(SourceConnectionCatalogResponse {
            protocol_version: 1,
            modes: vec![
                mode_capability("managed_shared", false, Some("gateway_not_configured")),
                mode_capability("managed_dedicated", false, Some("gateway_not_configured")),
                mode_capability("manual", true, None),
            ],
            gateway_configured: false,
            permalink_proxy: false,
        }));
    };
    let capabilities = gateway.capabilities().await.map_err(unavailable)?;
    if capabilities.protocol_version != 1 || capabilities.max_delivery_batch == 0 {
        return Err(Status::failed_precondition(
            "Slack Gateway protocol capability mismatch",
        ));
    }
    Ok(Response::new(SourceConnectionCatalogResponse {
        protocol_version: capabilities.protocol_version,
        modes: vec![
            mode_capability(
                "managed_shared",
                capabilities
                    .supported_modes
                    .iter()
                    .any(|value| value == "managed_shared"),
                None,
            ),
            mode_capability(
                "managed_dedicated",
                capabilities
                    .supported_modes
                    .iter()
                    .any(|value| value == "managed_dedicated"),
                None,
            ),
            mode_capability("manual", true, None),
        ],
        gateway_configured: true,
        permalink_proxy: capabilities.permalink_proxy,
    }))
}

pub(crate) async fn dedicated_preview(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionDedicatedPreviewRequest>,
) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
    let config_token = Zeroizing::new(std::mem::take(&mut request.get_mut().config_token));
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_label(&req.display_label)?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    if config_token.trim().is_empty() || config_token.len() > 8192 {
        return Err(Status::invalid_argument(
            "Configuration Token must contain 1-8192 characters",
        ));
    }
    let gateway = server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?;
    let capabilities = gateway.capabilities().await.map_err(unavailable)?;
    if !capabilities
        .supported_modes
        .iter()
        .any(|mode| mode == "managed_dedicated")
    {
        return Err(Status::failed_precondition(
            "Slack Gateway does not support managed_dedicated",
        ));
    }
    let provisioning_id = format!("dedicated-{}", Uuid::new_v4());
    let (oauth_callback_url, events_url) = dedicated_urls(gateway.origin(), &provisioning_id)?;
    let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../deploy/slack/dedicated-app-manifest.json"
    ))
    .map_err(internal)?;
    render_manifest_endpoints(&mut manifest, &oauth_callback_url, &events_url).map_err(internal)?;
    let contract = reviewed_manifest_contract(&manifest).map_err(internal)?;
    server
        .slack_manifest_client
        .validate_manifest(&config_token, &manifest)
        .await
        .map_err(|error| unavailable(anyhow::Error::new(error)))?;
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(internal)?;
    let manifest_digest = hex::encode(Sha256::digest(&manifest_bytes));
    let daemon_id = repository(server).daemon_id().await.map_err(internal)?;
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionDedicatedPreview",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection_provisioning",
            target_id: &provisioning_id,
            action: "source.connection.dedicated.preview",
            expected_version: None,
            fencing_token: None,
            canonical_request: serde_json::json!({
                "project_id": req.project_id,
                "display_label": req.display_label,
                "manifest_version": DEDICATED_MANIFEST_VERSION,
                "manifest_digest": manifest_digest,
            }),
            fallback_reason_code: "dedicated_slack_app",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching dedicated preview was already processed",
        )));
    }
    let checkpoint = repository(server)
        .store_dedicated_provisioning(StoreDedicatedProvisioning {
            id: provisioning_id.clone(),
            project_id: req.project_id.clone(),
            display_label: req.display_label.clone(),
            owner_daemon_id: daemon_id.clone(),
            manifest_version: DEDICATED_MANIFEST_VERSION.to_string(),
            manifest_digest: manifest_digest.clone(),
            expires_at,
        })
        .await
        .map_err(internal)?;
    server.dedicated_sessions.lock().await.insert(
        provisioning_id.clone(),
        DedicatedSession {
            project_id: req.project_id,
            display_label: req.display_label,
            owner_daemon_id: daemon_id,
            manifest,
            manifest_digest,
            config_token,
            import_secret: None,
            created_credentials: None,
        },
    );
    attempt
        .succeeded(
            server,
            Some("source_connection_provisioning"),
            Some(&provisioning_id),
        )
        .await?;
    Ok(attempt.response(dedicated_response(
        checkpoint,
        Some(manifest_diff(&contract)),
        None,
    )))
}

pub(crate) async fn dedicated_get(
    server: &OrchestratorServer,
    request: Request<SourceConnectionDedicatedGetRequest>,
) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
    super::authorize(server, &request, "SourceConnectionDedicatedGet").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    validate_id(&req.provisioning_id, "provisioning id")?;
    let mut checkpoint = repository(server)
        .dedicated_provisioning(&req.project_id, &req.provisioning_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("dedicated provisioning not found"))?;
    if matches!(checkpoint.status.as_str(), "creating" | "handoff_pending")
        && !server
            .dedicated_sessions
            .lock()
            .await
            .contains_key(&checkpoint.id)
    {
        checkpoint = repository(server)
            .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                project_id: checkpoint.project_id.clone(),
                id: checkpoint.id.clone(),
                expected_status: checkpoint.status.clone(),
                status: "attention".into(),
                app_id_ciphertext: None,
                app_id_digest: None,
                oauth_intent_id: None,
                error_code: Some("provisioning_session_lost".into()),
            })
            .await
            .map_err(internal)?;
    }
    Ok(Response::new(dedicated_response(checkpoint, None, None)))
}

pub(crate) async fn dedicated_abandon(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionDedicatedMutationRequest>,
) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.provisioning_id, "provisioning id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    let current = repository(server)
        .dedicated_provisioning(&req.project_id, &req.provisioning_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("dedicated provisioning not found"))?;
    if matches!(current.status.as_str(), "completed" | "abandoned") {
        return Err(Status::failed_precondition(
            "dedicated provisioning is already terminal",
        ));
    }
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionDedicatedAbandon",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection_provisioning",
            target_id: &req.provisioning_id,
            action: "source.connection.dedicated.abandon",
            expected_version: Some(current.status.clone()),
            fencing_token: None,
            canonical_request: serde_json::json!({"provisioning_id": req.provisioning_id}),
            fallback_reason_code: "dedicated_slack_app",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("abandon already processed")));
    }
    server
        .dedicated_sessions
        .lock()
        .await
        .remove(&req.provisioning_id);
    let checkpoint = repository(server)
        .update_dedicated_provisioning(UpdateDedicatedProvisioning {
            project_id: req.project_id,
            id: req.provisioning_id.clone(),
            expected_status: current.status,
            status: "abandoned".into(),
            app_id_ciphertext: None,
            app_id_digest: None,
            oauth_intent_id: None,
            error_code: Some("provisioning_abandoned".into()),
        })
        .await
        .map_err(internal)?;
    attempt
        .succeeded(
            server,
            Some("source_connection_provisioning"),
            Some(&req.provisioning_id),
        )
        .await?;
    Ok(attempt.response(dedicated_response(checkpoint, None, None)))
}

pub(crate) async fn dedicated_approve(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionDedicatedMutationRequest>,
) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.provisioning_id, "provisioning id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    let current = repository(server)
        .dedicated_provisioning(&req.project_id, &req.provisioning_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("dedicated provisioning not found"))?;
    if !matches!(
        current.status.as_str(),
        "awaiting_approval" | "handoff_pending"
    ) {
        return Err(Status::failed_precondition(
            "dedicated provisioning cannot be approved from its current state",
        ));
    }
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionDedicatedApprove",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection_provisioning",
            target_id: &req.provisioning_id,
            action: "source.connection.dedicated.approve",
            expected_version: Some(current.status.clone()),
            fencing_token: None,
            canonical_request: serde_json::json!({
                "provisioning_id": req.provisioning_id,
                "manifest_version": current.manifest_version,
                "manifest_digest": current.manifest_digest,
            }),
            fallback_reason_code: "dedicated_slack_app",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("approval already processed")));
    }
    match perform_dedicated_approve(server, &req, current, &attempt.request_id).await {
        Ok(response) => {
            attempt
                .succeeded(
                    server,
                    Some("source_connection_provisioning"),
                    Some(&req.provisioning_id),
                )
                .await?;
            Ok(attempt.response(response))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

async fn perform_dedicated_approve(
    server: &OrchestratorServer,
    req: &SourceConnectionDedicatedMutationRequest,
    current: CoreDedicatedProvisioning,
    request_id: &str,
) -> Result<SourceConnectionDedicatedProvisioningResponse, Status> {
    let mut session = server
        .dedicated_sessions
        .lock()
        .await
        .remove(&req.provisioning_id)
        .ok_or_else(|| Status::failed_precondition("provisioning_session_lost"))?;
    if session.project_id != req.project_id
        || session.owner_daemon_id != current.owner_daemon_id
        || session.manifest_digest != current.manifest_digest
    {
        return Err(Status::permission_denied(
            "dedicated provisioning session boundary mismatch",
        ));
    }
    let gateway = server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?;
    let mut checkpoint = current;
    if checkpoint.status == "awaiting_approval" {
        checkpoint = repository(server)
            .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                project_id: req.project_id.clone(),
                id: req.provisioning_id.clone(),
                expected_status: "awaiting_approval".into(),
                status: "creating".into(),
                app_id_ciphertext: None,
                app_id_digest: None,
                oauth_intent_id: None,
                error_code: None,
            })
            .await
            .map_err(internal)?;
        let slot = match gateway
            .create_dedicated_import_slot(
                &req.provisioning_id,
                &session.owner_daemon_id,
                &req.project_id,
                DEDICATED_MANIFEST_VERSION,
                &session.manifest_digest,
            )
            .await
        {
            Ok(slot) => slot,
            Err(error) => {
                mark_dedicated_attention(server, &checkpoint, "gateway_import_slot_failed").await?;
                return Err(unavailable(error));
            }
        };
        let contract = reviewed_manifest_contract(&session.manifest).map_err(internal)?;
        if contract.redirect_url != slot.oauth_callback_url
            || contract.events_url != slot.events_url
            || slot.connection_id != req.provisioning_id
            || slot.expires_at.trim().is_empty()
        {
            mark_dedicated_attention(server, &checkpoint, "gateway_endpoint_mismatch").await?;
            return Err(Status::data_loss("Gateway dedicated endpoint mismatch"));
        }
        session.import_secret = Some(Zeroizing::new(slot.import_secret));
        let created = match server
            .slack_manifest_client
            .provision_manifest(&session.config_token, &session.manifest)
            .await
        {
            Ok(created) => created,
            Err(error) => {
                mark_dedicated_attention(server, &checkpoint, "slack_manifest_create_uncertain")
                    .await?;
                return Err(unavailable(anyhow::Error::new(error)));
            }
        };
        let app_id_digest = hex::encode(Sha256::digest(created.app_id.as_bytes()));
        let app_id_ciphertext = encryption(server)?
            .encrypt_source_connection_credential(
                &req.project_id,
                &req.provisioning_id,
                &created.app_id,
            )
            .map_err(internal)?;
        session.created_credentials = Some(DedicatedCreatedCredentials {
            app_id: Zeroizing::new(created.credentials.app_id),
            client_id: Zeroizing::new(created.credentials.client_id),
            client_secret: Zeroizing::new(created.credentials.client_secret),
            signing_secret: Zeroizing::new(created.credentials.signing_secret),
        });
        checkpoint = repository(server)
            .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                project_id: req.project_id.clone(),
                id: req.provisioning_id.clone(),
                expected_status: "creating".into(),
                status: "handoff_pending".into(),
                app_id_ciphertext: Some(app_id_ciphertext),
                app_id_digest: Some(app_id_digest),
                oauth_intent_id: None,
                error_code: None,
            })
            .await
            .map_err(internal)?;
    }
    let credentials = session
        .created_credentials
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("dedicated_app_credentials_unavailable"))?;
    let import_secret = session
        .import_secret
        .as_deref()
        .ok_or_else(|| Status::failed_precondition("dedicated_import_capability_unavailable"))?;
    let imported = match gateway
        .import_dedicated_app(
            &req.provisioning_id,
            &session.owner_daemon_id,
            &req.project_id,
            request_id,
            &session.manifest_digest,
            import_secret,
            &crate::slack_gateway::GatewayDedicatedCredentials {
                app_id: &credentials.app_id,
                client_id: &credentials.client_id,
                client_secret: &credentials.client_secret,
                signing_secret: &credentials.signing_secret,
            },
        )
        .await
    {
        Ok(imported) => imported,
        Err(error) => {
            server
                .dedicated_sessions
                .lock()
                .await
                .insert(req.provisioning_id.clone(), session);
            return Err(unavailable(error));
        }
    };
    let local_intent_id = format!("intent-{}", Uuid::new_v4());
    let encryption = encryption(server)?;
    let authorize_url_ciphertext = encryption
        .encrypt_source_connection_credential(
            &req.project_id,
            &local_intent_id,
            &imported.authorize_url,
        )
        .map_err(internal)?;
    let poll_secret_ciphertext = encryption
        .encrypt_source_connection_credential(
            &req.project_id,
            &local_intent_id,
            &imported.poll_secret,
        )
        .map_err(internal)?;
    repository(server)
        .store_intent(StoreSourceConnectionIntent {
            id: local_intent_id.clone(),
            project_id: req.project_id.clone(),
            provider: "slack".into(),
            display_label: session.display_label,
            provisioning_mode: SourceConnectionMode::ManagedDedicated,
            owner_daemon_id: session.owner_daemon_id,
            actor_digest: hex::encode(Sha256::digest(request_id.as_bytes())),
            gateway_intent_id: imported.intent_id,
            authorize_url_ciphertext,
            poll_secret_ciphertext,
            expires_at: imported.expires_at.clone(),
        })
        .await
        .map_err(internal)?;
    checkpoint = repository(server)
        .update_dedicated_provisioning(UpdateDedicatedProvisioning {
            project_id: req.project_id.clone(),
            id: req.provisioning_id.clone(),
            expected_status: checkpoint.status,
            status: "oauth_pending".into(),
            app_id_ciphertext: None,
            app_id_digest: Some(imported.app_id_digest),
            oauth_intent_id: Some(local_intent_id.clone()),
            error_code: None,
        })
        .await
        .map_err(internal)?;
    Ok(dedicated_response(
        checkpoint,
        None,
        Some((local_intent_id, imported.authorize_url)),
    ))
}

async fn mark_dedicated_attention(
    server: &OrchestratorServer,
    checkpoint: &CoreDedicatedProvisioning,
    error_code: &str,
) -> Result<(), Status> {
    repository(server)
        .update_dedicated_provisioning(UpdateDedicatedProvisioning {
            project_id: checkpoint.project_id.clone(),
            id: checkpoint.id.clone(),
            expected_status: checkpoint.status.clone(),
            status: "attention".into(),
            app_id_ciphertext: None,
            app_id_digest: None,
            oauth_intent_id: None,
            error_code: Some(error_code.into()),
        })
        .await
        .map(|_| ())
        .map_err(internal)
}

pub(crate) async fn connect(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionConnectRequest>,
) -> Result<Response<SourceConnectionIntentResponse>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    if req.provider != "slack" || req.provisioning_mode != "managed_shared" {
        return Err(Status::failed_precondition(
            "this capability version only connects slack/managed_shared",
        ));
    }
    validate_mutation(&req.reason, &req.idempotency_key)?;
    validate_label(&req.display_label)?;
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionConnect",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection_intent",
            target_id: &req.idempotency_key,
            action: "source.connection.connect",
            expected_version: None,
            fencing_token: None,
            canonical_request: serde_json::json!({
                "project_id": req.project_id,
                "provider": req.provider,
                "mode": req.provisioning_mode,
                "display_label": req.display_label,
            }),
            fallback_reason_code: "managed_connection",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching SourceConnection connect was already processed",
        )));
    }
    let result = create_intent(
        server,
        &req.project_id,
        &req.display_label,
        &attempt.request_id,
    )
    .await;
    match result {
        Ok(response) => {
            attempt
                .succeeded(server, Some("source_connection_intent"), Some(&response.id))
                .await?;
            Ok(attempt.response(response))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

pub(crate) async fn intent_get(
    server: &OrchestratorServer,
    request: Request<SourceConnectionIntentGetRequest>,
) -> Result<Response<SourceConnectionIntentResponse>, Status> {
    super::authorize(server, &request, "SourceConnectionIntentGet").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    validate_id(&req.intent_id, "intent id")?;
    reconcile_intent(server, &req.project_id, &req.intent_id)
        .await
        .map(Response::new)
}

pub(crate) async fn cancel(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionIntentMutationRequest>,
) -> Result<Response<SourceConnectionIntentResponse>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.intent_id, "intent id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionCancel",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection_intent",
            target_id: &req.intent_id,
            action: "source.connection.cancel",
            expected_version: None,
            fencing_token: None,
            canonical_request: serde_json::json!({"intent_id": req.intent_id}),
            fallback_reason_code: "managed_connection",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("intent cancel already processed")));
    }
    let result = cancel_intent(server, &req.project_id, &req.intent_id).await;
    match result {
        Ok(response) => {
            attempt
                .succeeded(server, Some("source_connection_intent"), Some(&response.id))
                .await?;
            Ok(attempt.response(response))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

pub(crate) async fn reauthorize(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionMutationRequest>,
) -> Result<Response<SourceConnectionIntentResponse>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    let current = repository(server)
        .get(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection not found"))?;
    if current.version != req.expected_version {
        return Err(Status::aborted("SourceConnection version conflict"));
    }
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionReauthorize",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection",
            target_id: &req.id,
            action: "source.connection.reauthorize",
            expected_version: Some(req.expected_version.to_string()),
            fencing_token: Some(current.generation),
            canonical_request: serde_json::json!({"connection_id": req.id}),
            fallback_reason_code: "managed_connection",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("reauthorization already processed")));
    }
    let result = create_intent(
        server,
        &req.project_id,
        &current.display_label,
        &attempt.request_id,
    )
    .await;
    match result {
        Ok(response) => {
            attempt
                .succeeded(server, Some("source_connection_intent"), Some(&response.id))
                .await?;
            Ok(attempt.response(response))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

pub(crate) async fn disconnect(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionMutationRequest>,
) -> Result<Response<SourceConnection>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionDisconnect",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection",
            target_id: &req.id,
            action: "source.connection.disconnect",
            expected_version: Some(req.expected_version.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({"connection_id": req.id}),
            fallback_reason_code: "managed_connection",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("disconnect already processed")));
    }
    let result = disconnect_connection(server, &req, &attempt.request_id).await;
    match result {
        Ok(connection) => {
            attempt
                .succeeded(server, Some("source_connection"), Some(&connection.id))
                .await?;
            Ok(attempt.response(connection_to_proto(connection)))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

pub(crate) async fn transfer(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionTransferRequest>,
) -> Result<Response<SourceConnection>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    validate_id(&req.target_daemon_id, "target daemon id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionTransfer",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection",
            target_id: &req.id,
            action: "source.connection.transfer",
            expected_version: Some(req.expected_version.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({
                "connection_id": req.id,
                "target_daemon_id": req.target_daemon_id,
            }),
            fallback_reason_code: "managed_connection",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("transfer already processed")));
    }
    let result = transfer_connection(server, &req, &attempt.request_id).await;
    match result {
        Ok(connection) => {
            attempt
                .succeeded(server, Some("source_connection"), Some(&connection.id))
                .await?;
            Ok(attempt.response(connection_to_proto(connection)))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

async fn create_intent(
    server: &OrchestratorServer,
    project_id: &str,
    display_label: &str,
    request_id: &str,
) -> Result<SourceConnectionIntentResponse, Status> {
    let gateway = server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?;
    let repository = repository(server);
    let daemon_id = repository.daemon_id().await.map_err(internal)?;
    let created = gateway
        .create_intent(&daemon_id, project_id, request_id)
        .await
        .map_err(unavailable)?;
    let local_id = format!("intent-{}", Uuid::new_v4());
    let encryption = encryption(server)?;
    let authorize_url_ciphertext = encryption
        .encrypt_source_connection_credential(project_id, &local_id, &created.authorize_url)
        .map_err(internal)?;
    let poll_secret_ciphertext = encryption
        .encrypt_source_connection_credential(project_id, &local_id, &created.poll_secret)
        .map_err(internal)?;
    let actor_digest = hex::encode(Sha256::digest(request_id.as_bytes()));
    let intent = repository
        .store_intent(StoreSourceConnectionIntent {
            id: local_id,
            project_id: project_id.to_string(),
            provider: "slack".to_string(),
            display_label: display_label.to_string(),
            provisioning_mode: SourceConnectionMode::ManagedShared,
            owner_daemon_id: daemon_id,
            actor_digest,
            gateway_intent_id: created.intent_id,
            authorize_url_ciphertext,
            poll_secret_ciphertext,
            expires_at: created.expires_at,
        })
        .await
        .map_err(internal)?;
    let mut response = intent_to_proto(intent);
    response.authorize_url = Some(created.authorize_url);
    Ok(response)
}

async fn reconcile_intent(
    server: &OrchestratorServer,
    project_id: &str,
    intent_id: &str,
) -> Result<SourceConnectionIntentResponse, Status> {
    let repository = repository(server);
    let daemon_id = repository.daemon_id().await.map_err(internal)?;
    let stored = repository
        .intent_credential(project_id, intent_id, &daemon_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection intent not found"))?;
    let encryption = encryption(server)?;
    let authorize_url = encryption
        .decrypt_source_connection_credential(
            project_id,
            intent_id,
            &stored.authorize_url_ciphertext,
        )
        .map_err(internal)?;
    if stored.intent.status != "pending" {
        let mut response = intent_to_proto(stored.intent.clone());
        response.authorize_url = Some(authorize_url);
        if let Some(connection_id) = stored.intent.connection_id.as_deref() {
            response.connection = repository
                .get(project_id, connection_id)
                .await
                .map_err(internal)?
                .map(connection_to_proto);
        }
        return Ok(response);
    }
    let poll_secret = encryption
        .decrypt_source_connection_credential(project_id, intent_id, &stored.poll_secret_ciphertext)
        .map_err(internal)?;
    let gateway = server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?;
    let status = gateway
        .intent_status(&stored.gateway_intent_id, &poll_secret)
        .await
        .map_err(unavailable)?;
    if status.intent_id != stored.gateway_intent_id || status.expires_at != stored.intent.expires_at
    {
        return Err(Status::data_loss("Slack Gateway intent identity mismatch"));
    }
    if status.status == "pending" {
        let mut response = intent_to_proto(stored.intent);
        response.authorize_url = Some(authorize_url);
        return Ok(response);
    }
    if status.status != "completed" {
        let terminal = if status.status == "cancelled" {
            "cancelled"
        } else {
            "failed"
        };
        let intent = repository
            .complete_intent(
                project_id,
                intent_id,
                terminal,
                None,
                status.error_code.as_deref(),
            )
            .await
            .map_err(internal)?;
        return Ok(intent_to_proto(intent));
    }
    let installation = status
        .installation
        .ok_or_else(|| Status::data_loss("completed Gateway intent has no installation"))?;
    let pairing_secret = status
        .pairing_secret
        .ok_or_else(|| Status::data_loss("completed Gateway intent has no pairing credential"))?;
    if installation.owner_daemon_id != daemon_id || installation.owner_project_id != project_id {
        return Err(Status::permission_denied(
            "Gateway installation owner boundary mismatch",
        ));
    }
    if installation.state != "active" || installation.version < 1 {
        return Err(Status::failed_precondition(
            "Gateway installation is not active",
        ));
    }
    let connection_id = format!("conn-{}", installation.id);
    let pairing_secret_ciphertext = encryption
        .encrypt_source_connection_credential(project_id, &connection_id, &pairing_secret)
        .map_err(internal)?;
    let trigger_name = ensure_default_trigger(
        &server.state,
        &server.config_mutation_lock,
        project_id,
        &connection_id,
        &installation.id,
    )
    .await?;
    let connection = repository
        .activate(ActivateSourceConnection {
            id: connection_id.clone(),
            project_id: project_id.to_string(),
            provider: "slack".to_string(),
            display_label: stored.display_label,
            provisioning_mode: stored.intent.provisioning_mode,
            app_ownership: if installation.provisioning_mode == "managed_dedicated" {
                "workspace".into()
            } else {
                "orchestrator".into()
            },
            app_id_digest: installation.app_id_digest.clone(),
            manifest_version: installation.manifest_version.clone(),
            provision_state: (installation.provisioning_mode == "managed_dedicated")
                .then(|| "completed".into()),
            provision_error_code: None,
            installation_id: installation.id,
            installation_id_digest: installation.team_digest,
            enterprise_id_digest: installation.enterprise_digest,
            owner_daemon_id: daemon_id,
            generation: installation.generation,
            version: installation.version,
            last_acked_cursor: installation.last_acked_cursor,
            capabilities: vec!["delivery_v1".into(), "permalink_proxy".into()],
            scopes: installation.scopes,
            trigger_name: Some(trigger_name),
            gateway_origin: Some(gateway.origin().to_string()),
            pairing_secret_ciphertext: Some(pairing_secret_ciphertext),
            request_id: format!("req-oauth-{intent_id}"),
        })
        .await
        .map_err(internal)?;
    if let Some(provisioning_id) = installation.app_connection_id.as_deref() {
        if let Some(checkpoint) = repository
            .dedicated_provisioning(project_id, provisioning_id)
            .await
            .map_err(internal)?
        {
            if checkpoint.status == "oauth_pending" {
                repository
                    .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                        project_id: project_id.to_string(),
                        id: provisioning_id.to_string(),
                        expected_status: "oauth_pending".into(),
                        status: "completed".into(),
                        app_id_ciphertext: None,
                        app_id_digest: installation.app_id_digest.clone(),
                        oauth_intent_id: Some(intent_id.to_string()),
                        error_code: None,
                    })
                    .await
                    .map_err(internal)?;
            }
        }
    }
    let intent = repository
        .complete_intent(
            project_id,
            intent_id,
            "completed",
            Some(&connection_id),
            installation.last_error_code.as_deref(),
        )
        .await
        .map_err(internal)?;
    let mut response = intent_to_proto(intent);
    response.connection = Some(connection_to_proto(connection));
    Ok(response)
}

async fn cancel_intent(
    server: &OrchestratorServer,
    project_id: &str,
    intent_id: &str,
) -> Result<SourceConnectionIntentResponse, Status> {
    let repository = repository(server);
    let daemon_id = repository.daemon_id().await.map_err(internal)?;
    let stored = repository
        .intent_credential(project_id, intent_id, &daemon_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection intent not found"))?;
    if stored.intent.status != "pending" {
        return Err(Status::failed_precondition("OAuth intent is not pending"));
    }
    let poll_secret = encryption(server)?
        .decrypt_source_connection_credential(project_id, intent_id, &stored.poll_secret_ciphertext)
        .map_err(internal)?;
    server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?
        .cancel_intent(&stored.gateway_intent_id, &poll_secret)
        .await
        .map_err(unavailable)?;
    repository
        .complete_intent(project_id, intent_id, "cancelled", None, None)
        .await
        .map(intent_to_proto)
        .map_err(internal)
}

async fn disconnect_connection(
    server: &OrchestratorServer,
    req: &SourceConnectionMutationRequest,
    request_id: &str,
) -> Result<CoreConnection, Status> {
    let repository = repository(server);
    let daemon_id = repository.daemon_id().await.map_err(internal)?;
    let connection = repository
        .get(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection not found"))?;
    if connection.version != req.expected_version {
        return Err(Status::aborted("SourceConnection version conflict"));
    }
    let credential = repository
        .credential(&req.project_id, &req.id, &daemon_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::failed_precondition("active connection credential missing"))?;
    let pairing = encryption(server)?
        .decrypt_source_connection_credential(
            &req.project_id,
            &req.id,
            &credential.pairing_secret_ciphertext,
        )
        .map_err(internal)?;
    server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?
        .disconnect(
            &credential.installation_id,
            &daemon_id,
            req.expected_version,
            &pairing,
        )
        .await
        .map_err(unavailable)?;
    repository
        .transition(
            &req.project_id,
            &req.id,
            req.expected_version,
            SourceConnectionState::Disconnected,
            None,
            request_id,
        )
        .await
        .map_err(internal)
}

async fn transfer_connection(
    server: &OrchestratorServer,
    req: &SourceConnectionTransferRequest,
    request_id: &str,
) -> Result<CoreConnection, Status> {
    let repository = repository(server);
    let daemon_id = repository.daemon_id().await.map_err(internal)?;
    if req.target_daemon_id == daemon_id {
        return Err(Status::failed_precondition(
            "target daemon already owns this connection",
        ));
    }
    let connection = repository
        .get(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection not found"))?;
    if connection.version != req.expected_version {
        return Err(Status::aborted("SourceConnection version conflict"));
    }
    let credential = repository
        .credential(&req.project_id, &req.id, &daemon_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::failed_precondition("active connection credential missing"))?;
    let pairing = encryption(server)?
        .decrypt_source_connection_credential(
            &req.project_id,
            &req.id,
            &credential.pairing_secret_ciphertext,
        )
        .map_err(internal)?;
    let transferred = server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?
        .transfer(
            &credential.installation_id,
            &daemon_id,
            req.expected_version,
            &req.target_daemon_id,
            &pairing,
        )
        .await
        .map_err(unavailable)?;
    if transferred.installation.id != credential.installation_id
        || transferred.installation.owner_project_id != req.project_id
        || transferred.installation.owner_daemon_id != req.target_daemon_id
        || transferred.installation.version != req.expected_version + 1
        || transferred.installation.state != "active"
        || transferred.installation.last_acked_cursor < connection.last_acked_cursor
    {
        return Err(Status::data_loss(
            "Slack Gateway transfer projection mismatch; operator reconciliation required",
        ));
    }
    repository
        .transfer_owner(TransferSourceConnectionOwner {
            project_id: req.project_id.clone(),
            id: req.id.clone(),
            expected_version: req.expected_version,
            target_daemon_id: req.target_daemon_id.clone(),
            generation: transferred.installation.generation,
            request_id: request_id.to_string(),
        })
        .await
        .map_err(|error| {
            tracing::error!(
                connection_id = %req.id,
                target_daemon_id = %req.target_daemon_id,
                error = %error,
                "Gateway ownership moved but local transfer projection failed"
            );
            Status::data_loss(
                "Gateway ownership moved but local transfer projection failed; operator reconciliation required",
            )
        })
}

pub(crate) async fn ensure_default_trigger(
    state: &std::sync::Arc<agent_orchestrator::state::InnerState>,
    config_mutation_lock: &std::sync::Arc<tokio::sync::Mutex<()>>,
    project_id: &str,
    connection_id: &str,
    installation_id: &str,
) -> Result<String, Status> {
    let _guard = config_mutation_lock.lock().await;
    let active = agent_orchestrator::config_load::read_active_config(state)
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    let project = active
        .config
        .projects
        .get(project_id)
        .ok_or_else(|| Status::not_found("project not found"))?;
    let trigger_name = format!(
        "slack-{}",
        &connection_id[connection_id.len().saturating_sub(24)..]
    );
    if let Some(existing) = project.triggers.get(&trigger_name) {
        let reference = existing
            .event
            .as_ref()
            .and_then(|event| event.webhook.as_ref())
            .and_then(|webhook| webhook.connection_ref.as_deref());
        if reference == Some(connection_id) {
            return Ok(trigger_name);
        }
        return Err(Status::already_exists(
            "default managed Slack Trigger name is occupied",
        ));
    }
    let workflow = project.workflows.keys().min().cloned().ok_or_else(|| {
        Status::failed_precondition("project has no workflow for default Trigger")
    })?;
    let workspace = project.workspaces.keys().min().cloned().ok_or_else(|| {
        Status::failed_precondition("project has no workspace for default Trigger")
    })?;
    let manifest = serde_yaml::to_string(&serde_json::json!({
        "apiVersion": "orchestrator.dev/v2",
        "kind": "Trigger",
        "metadata": {"name": trigger_name, "project": project_id},
        "spec": {
            "event": {"source": "webhook", "webhook": {
                "provider": "slack",
                "installationId": installation_id,
                "connectionRef": connection_id,
                "reactionRouting": "disabled"
            }},
            "action": {"workflow": workflow, "workspace": workspace, "start": true},
            "suspend": false
        }
    }))
    .map_err(internal)?;
    let applied = agent_orchestrator::service::resource::apply_manifests(
        state,
        &manifest,
        false,
        Some(project_id),
        false,
    )
    .map_err(super::map_core_error)?;
    if !applied.errors.is_empty() {
        return Err(Status::failed_precondition(applied.errors.join("; ")));
    }
    Ok(trigger_name)
}

fn encryption(
    server: &OrchestratorServer,
) -> Result<agent_orchestrator::secret_store_crypto::SecretEncryption, Status> {
    let keyring = agent_orchestrator::secret_key_lifecycle::load_keyring(
        &server.state.data_dir,
        &server.state.db_path,
    )
    .map_err(internal)?;
    agent_orchestrator::secret_store_crypto::SecretEncryption::from_keyring(&keyring)
        .map_err(internal)
}

fn repository(server: &OrchestratorServer) -> AsyncSourceConnectionRepository {
    AsyncSourceConnectionRepository::new(server.state.async_database.clone())
}

fn connection_to_proto(value: CoreConnection) -> SourceConnection {
    SourceConnection {
        id: value.id,
        project_id: value.project_id,
        provider: value.provider,
        display_label: value.display_label,
        provisioning_mode: value.provisioning_mode.as_str().to_string(),
        app_ownership: value.app_ownership,
        app_id_digest: value.app_id_digest,
        manifest_version: value.manifest_version,
        provision_state: value.provision_state,
        provision_error_code: value.provision_error_code,
        installation_id: value.installation_id,
        installation_id_digest: value.installation_id_digest,
        enterprise_id_digest: value.enterprise_id_digest,
        owner_daemon_id: value.owner_daemon_id,
        generation: value.generation,
        version: value.version,
        state: value.state.as_str().to_string(),
        capabilities: value.capabilities,
        scopes: value.scopes,
        trigger_name: value.trigger_name,
        last_delivery_at: value.last_delivery_at,
        last_acked_cursor: value.last_acked_cursor,
        delivery_lag: value.delivery_lag,
        last_error_code: value.last_error_code,
        created_at: value.created_at,
        updated_at: value.updated_at,
        reauthorized_at: value.reauthorized_at,
        disconnected_at: value.disconnected_at,
    }
}

fn dedicated_urls(gateway_origin: &str, provisioning_id: &str) -> Result<(String, String), Status> {
    let origin = url::Url::parse(gateway_origin).map_err(internal)?;
    let callback = origin
        .join(&format!(
            "slack/connections/{provisioning_id}/oauth/callback"
        ))
        .map_err(internal)?;
    let events = origin
        .join(&format!("slack/connections/{provisioning_id}/events"))
        .map_err(internal)?;
    Ok((callback.to_string(), events.to_string()))
}

fn manifest_diff(
    contract: &orchestrator_slack_gateway::slack::ReviewedManifestContract,
) -> Vec<SourceConnectionManifestDiffEntry> {
    vec![
        SourceConnectionManifestDiffEntry {
            field: "oauth.scopes.bot".into(),
            change: "add".into(),
            before: vec![],
            after: contract.bot_scopes.clone(),
            permission_expansion: true,
        },
        SourceConnectionManifestDiffEntry {
            field: "events.bot_events".into(),
            change: "add".into(),
            before: vec![],
            after: contract.bot_events.clone(),
            permission_expansion: true,
        },
        SourceConnectionManifestDiffEntry {
            field: "oauth.redirect_url".into(),
            change: "set".into(),
            before: vec![],
            after: vec![safe_url_origin(&contract.redirect_url)],
            permission_expansion: true,
        },
        SourceConnectionManifestDiffEntry {
            field: "events.request_url".into(),
            change: "set".into(),
            before: vec![],
            after: vec![safe_url_origin(&contract.events_url)],
            permission_expansion: true,
        },
        SourceConnectionManifestDiffEntry {
            field: "settings.token_rotation_enabled".into(),
            change: "set".into(),
            before: vec![],
            after: vec!["false".into()],
            permission_expansion: false,
        },
    ]
}

fn safe_url_origin(value: &str) -> String {
    url::Url::parse(value)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?.to_string();
            Some(format!("{}://{host}", url.scheme()))
        })
        .unwrap_or_else(|| "invalid-origin".into())
}

fn dedicated_response(
    value: CoreDedicatedProvisioning,
    diff: Option<Vec<SourceConnectionManifestDiffEntry>>,
    oauth: Option<(String, String)>,
) -> SourceConnectionDedicatedProvisioningResponse {
    let (oauth_intent_id, authorize_url) = oauth
        .map(|(intent, url)| (Some(intent), Some(url)))
        .unwrap_or_else(|| (value.oauth_intent_id.clone(), None));
    SourceConnectionDedicatedProvisioningResponse {
        id: value.id,
        project_id: value.project_id,
        status: value.status,
        manifest_version: value.manifest_version,
        manifest_digest: value.manifest_digest,
        diff: diff.unwrap_or_default(),
        app_id_digest: value.app_id_digest,
        oauth_intent_id,
        authorize_url,
        error_code: value.error_code,
        expires_at: value.expires_at,
    }
}

fn intent_to_proto(value: CoreIntent) -> SourceConnectionIntentResponse {
    SourceConnectionIntentResponse {
        id: value.id,
        project_id: value.project_id,
        provider: value.provider,
        provisioning_mode: value.provisioning_mode.as_str().to_string(),
        status: value.status,
        connection_id: value.connection_id,
        error_code: value.error_code,
        expires_at: value.expires_at,
        authorize_url: None,
        connection: None,
    }
}

fn mode_capability(
    mode: &str,
    available: bool,
    reason: Option<&str>,
) -> SourceConnectionModeCapability {
    SourceConnectionModeCapability {
        mode: mode.to_string(),
        available,
        unavailable_reason: reason.map(str::to_string),
    }
}

fn audit_context(reason: &str, idempotency_key: &str) -> ActionAuditContext {
    ActionAuditContext {
        reason_code: "managed_connection".to_string(),
        operator_reason: Some(reason.to_string()),
        idempotency_key: Some(idempotency_key.to_string()),
    }
}

fn validate_project(value: &str) -> Result<(), Status> {
    validate_id(value, "project_id")
}

fn validate_id(value: &str, label: &str) -> Result<(), Status> {
    if value.trim().is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    {
        return Err(Status::invalid_argument(format!(
            "{label} must contain 1-128 safe characters"
        )));
    }
    Ok(())
}

fn validate_label(value: &str) -> Result<(), Status> {
    if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "display_label must contain 1-128 printable characters",
        ));
    }
    Ok(())
}

fn validate_mutation(reason: &str, idempotency_key: &str) -> Result<(), Status> {
    if reason.trim().is_empty() || reason.len() > 500 {
        return Err(Status::invalid_argument(
            "reason must contain 1-500 characters",
        ));
    }
    validate_id(idempotency_key, "idempotency_key")
}

fn internal(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

fn unavailable(error: impl std::fmt::Display) -> Status {
    Status::unavailable(error.to_string())
}
