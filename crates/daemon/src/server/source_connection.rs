//! Managed SourceConnection gRPC surface and OAuth intent reconciliation.

use agent_orchestrator::source_connection::{
    ActivateSourceConnection, AsyncSourceConnectionRepository, SourceConnection as CoreConnection,
    SourceConnectionIntent as CoreIntent, SourceConnectionMode, SourceConnectionState,
    StoreSourceConnectionIntent, TransferSourceConnectionOwner,
};
use futures::Stream;
use orchestrator_proto::*;
use sha2::{Digest, Sha256};
use std::pin::Pin;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::OrchestratorServer;
use super::action_audit::{self, ActionDescriptor};

pub(crate) type SourceConnectionWatchStream =
    Pin<Box<dyn Stream<Item = Result<SourceConnectionDelta, Status>> + Send>>;

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
                mode_capability("managed_dedicated", false, Some("fr_115_not_implemented")),
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
            mode_capability("managed_dedicated", false, Some("fr_115_not_implemented")),
            mode_capability("manual", true, None),
        ],
        gateway_configured: true,
        permalink_proxy: capabilities.permalink_proxy,
    }))
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
    let trigger_name =
        ensure_default_trigger(server, project_id, &connection_id, &installation.id).await?;
    let connection = repository
        .activate(ActivateSourceConnection {
            id: connection_id.clone(),
            project_id: project_id.to_string(),
            provider: "slack".to_string(),
            display_label: stored.display_label,
            provisioning_mode: SourceConnectionMode::ManagedShared,
            installation_id: installation.id,
            installation_id_digest: installation.team_digest,
            enterprise_id_digest: installation.enterprise_digest,
            owner_daemon_id: daemon_id,
            generation: installation.generation,
            capabilities: vec!["delivery_v1".into(), "permalink_proxy".into()],
            scopes: installation.scopes,
            trigger_name: Some(trigger_name),
            gateway_origin: Some(gateway.origin().to_string()),
            pairing_secret_ciphertext: Some(pairing_secret_ciphertext),
            request_id: format!("req-oauth-{intent_id}"),
        })
        .await
        .map_err(internal)?;
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
    let encryption = encryption(server)?;
    let pairing = encryption
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
    let replacement_ciphertext = encryption
        .encrypt_source_connection_credential(&req.project_id, &req.id, &transferred.pairing_secret)
        .map_err(internal)?;
    repository
        .transfer_owner(TransferSourceConnectionOwner {
            project_id: req.project_id.clone(),
            id: req.id.clone(),
            expected_version: req.expected_version,
            target_daemon_id: req.target_daemon_id.clone(),
            generation: transferred.installation.generation,
            pairing_secret_ciphertext: replacement_ciphertext,
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

async fn ensure_default_trigger(
    server: &OrchestratorServer,
    project_id: &str,
    connection_id: &str,
    installation_id: &str,
) -> Result<String, Status> {
    let _guard = server.config_mutation_lock.lock().await;
    let active = agent_orchestrator::config_load::read_active_config(&server.state)
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
        &server.state,
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
