//! Connection teardown and ownership movement, plus the default Trigger it needs.

use super::projection::*;
use super::*;

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
    .map_err(crate::server::map_core_error)?;
    if !applied.errors.is_empty() {
        return Err(Status::failed_precondition(applied.errors.join("; ")));
    }
    Ok(trigger_name)
}
