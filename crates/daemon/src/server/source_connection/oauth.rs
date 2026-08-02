//! Managed OAuth intents: connect, poll, cancel, reauthorize, and mode migration.

use super::dedicated::resolve_dedicated_attention;
use super::projection::*;
use super::transfer::ensure_default_trigger;
use super::*;

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
        None,
        None,
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
    crate::server::authorize(server, &request, "SourceConnectionIntentGet")
        .map_err(Status::from)?;
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
    let dedicated_identity = if current.provisioning_mode == SourceConnectionMode::ManagedDedicated
    {
        Some(
            repository(server)
                .dedicated_app_identity_for_connection(&req.project_id, &req.id)
                .await
                .map_err(internal)?
                .ok_or_else(|| {
                    Status::failed_precondition("dedicated App lifecycle identity is missing")
                })?,
        )
    } else {
        None
    };
    let result = create_intent(
        server,
        &req.project_id,
        &current.display_label,
        &attempt.request_id,
        dedicated_identity
            .as_ref()
            .map(|identity| identity.provisioning_id.as_str()),
        None,
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

pub(crate) async fn migrate_to_shared(
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
    if current.provisioning_mode != SourceConnectionMode::ManagedDedicated
        || current.state != SourceConnectionState::Active
    {
        return Err(Status::failed_precondition(
            "dedicated to shared migration requires one active managed_dedicated connection",
        ));
    }
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionMigrateToShared",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection",
            target_id: &req.id,
            action: "source.connection.migrate_to_shared",
            expected_version: Some(req.expected_version.to_string()),
            fencing_token: Some(current.generation),
            canonical_request: serde_json::json!({
                "connection_id": req.id,
                "source_mode": "managed_dedicated",
                "target_mode": "managed_shared",
            }),
            fallback_reason_code: "managed_connection_migration",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("migration already processed")));
    }
    match create_intent(
        server,
        &req.project_id,
        &current.display_label,
        &attempt.request_id,
        None,
        Some(&current),
    )
    .await
    {
        Ok(response) => {
            attempt
                .succeeded(server, Some("source_connection_intent"), Some(&response.id))
                .await?;
            Ok(attempt.response(response))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

pub(super) async fn create_intent(
    server: &OrchestratorServer,
    project_id: &str,
    display_label: &str,
    request_id: &str,
    dedicated_provisioning_id: Option<&str>,
    migration_target: Option<&CoreConnection>,
) -> Result<SourceConnectionIntentResponse, Status> {
    let gateway = server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?;
    let repository = repository(server);
    let daemon_id = repository.daemon_id().await.map_err(internal)?;
    let migration_fence =
        migration_target.map(|target| crate::slack_gateway::GatewayMigrationFence {
            installation_id: &target.installation_id,
            expected_version: target.version,
            source_mode: target.provisioning_mode.as_str(),
        });
    let created = match dedicated_provisioning_id {
        Some(connection_id) => {
            gateway
                .create_dedicated_intent(
                    connection_id,
                    &daemon_id,
                    project_id,
                    request_id,
                    migration_fence.as_ref(),
                )
                .await
        }
        None => {
            gateway
                .create_intent(&daemon_id, project_id, request_id, migration_fence.as_ref())
                .await
        }
    }
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
            provisioning_mode: if dedicated_provisioning_id.is_some() {
                SourceConnectionMode::ManagedDedicated
            } else {
                SourceConnectionMode::ManagedShared
            },
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
        let terminal = local_terminal_intent_status(&status.status, status.error_code.as_deref());
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
    server
        .state
        .attention_repo
        .resolve_external_candidate(
            project_id,
            &format!("source-connection-revoked:{connection_id}"),
            &format!("source-connection-reauthorized:{intent_id}"),
            "source_connection_reauthorized",
        )
        .await
        .map_err(internal)?;
    server
        .state
        .attention_repo
        .resolve_external_candidate(
            project_id,
            &format!("source-connection-lifecycle:{connection_id}"),
            &format!("source-connection-lifecycle-reauthorized:{intent_id}"),
            "source_connection_manifest_reauthorized",
        )
        .await
        .map_err(internal)?;
    if let Some(provisioning_id) = installation.app_connection_id.as_deref()
        && let Some(checkpoint) = repository
            .dedicated_provisioning(project_id, provisioning_id)
            .await
            .map_err(internal)?
        && checkpoint.status == "oauth_pending"
    {
        let completed = repository
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
        resolve_dedicated_attention(
            server,
            project_id,
            &completed.id,
            "dedicated_connection_activated",
        )
        .await?;
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
