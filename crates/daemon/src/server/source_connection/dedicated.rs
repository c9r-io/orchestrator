//! Dedicated Slack App provisioning: preview, checkpoint reads, abandon, approve.

use super::projection::*;
use super::*;

pub(crate) async fn dedicated_preview(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionDedicatedPreviewRequest>,
) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
    let config_token = Zeroizing::new(std::mem::take(&mut request.get_mut().config_token));
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_label(&req.display_label)?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    if let Some(target_id) = req.target_connection_id.as_deref() {
        validate_id(target_id, "target connection id")?;
        let target = repository(server)
            .get(&req.project_id, target_id)
            .await
            .map_err(internal)?
            .ok_or_else(|| Status::not_found("migration SourceConnection not found"))?;
        if target.provisioning_mode != SourceConnectionMode::ManagedShared
            || target.state != SourceConnectionState::Active
        {
            return Err(Status::failed_precondition(
                "shared to dedicated migration requires one active managed_shared connection",
            ));
        }
    }
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
    let mut manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/dedicated-app-manifest.json"))
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
                "target_connection_id": req.target_connection_id,
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
            target_connection_id: req.target_connection_id.clone(),
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
    crate::server::authorize(server, &request, "SourceConnectionDedicatedGet")
        .map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    validate_id(&req.provisioning_id, "provisioning id")?;
    let mut checkpoint = repository(server)
        .dedicated_provisioning(&req.project_id, &req.provisioning_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("dedicated provisioning not found"))?;
    let expired = chrono::DateTime::parse_from_rfc3339(&checkpoint.expires_at)
        .map(|value| value < chrono::Utc::now())
        .unwrap_or(true);
    if expired
        && matches!(
            checkpoint.status.as_str(),
            "awaiting_approval" | "creating" | "handoff_pending"
        )
    {
        server
            .dedicated_sessions
            .lock()
            .await
            .remove(&checkpoint.id);
        checkpoint =
            mark_dedicated_attention(server, &checkpoint, "provisioning_session_expired").await?;
    } else if matches!(checkpoint.status.as_str(), "creating" | "handoff_pending")
        && !server
            .dedicated_sessions
            .lock()
            .await
            .contains_key(&checkpoint.id)
    {
        checkpoint =
            mark_dedicated_attention(server, &checkpoint, "provisioning_session_lost").await?;
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
    resolve_dedicated_attention(
        server,
        &checkpoint.project_id,
        &checkpoint.id,
        "provisioning_abandoned",
    )
    .await?;
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
    let expired = chrono::DateTime::parse_from_rfc3339(&current.expires_at)
        .map(|value| value < chrono::Utc::now())
        .unwrap_or(true);
    if expired {
        server.dedicated_sessions.lock().await.remove(&current.id);
        mark_dedicated_attention(server, &current, "provisioning_session_expired").await?;
        return Err(Status::failed_precondition(
            "dedicated provisioning session expired",
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
    let migration_target = match checkpoint.target_connection_id.as_deref() {
        Some(connection_id) => {
            let target = repository(server)
                .get(&req.project_id, connection_id)
                .await
                .map_err(internal)?
                .ok_or_else(|| Status::not_found("migration SourceConnection not found"))?;
            if target.provisioning_mode != SourceConnectionMode::ManagedShared
                || target.state != SourceConnectionState::Active
            {
                server
                    .dedicated_sessions
                    .lock()
                    .await
                    .insert(req.provisioning_id.clone(), session);
                return Err(Status::failed_precondition(
                    "migration SourceConnection changed after review",
                ));
            }
            Some(target)
        }
        None => None,
    };
    let migration_fence =
        migration_target
            .as_ref()
            .map(|target| crate::slack_gateway::GatewayMigrationFence {
                installation_id: &target.installation_id,
                expected_version: target.version,
                source_mode: target.provisioning_mode.as_str(),
            });
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
            migration_fence.as_ref(),
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

pub(super) async fn mark_dedicated_attention(
    server: &OrchestratorServer,
    checkpoint: &CoreDedicatedProvisioning,
    error_code: &str,
) -> Result<CoreDedicatedProvisioning, Status> {
    let updated = repository(server)
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
        .map_err(internal)?;
    let digest = hex::encode(Sha256::digest(updated.id.as_bytes()));
    server
        .state
        .attention_repo
        .upsert_external_candidate(AttentionCandidate {
            id: format!("attention-dedicated-{}", &digest[..24]),
            project_id: updated.project_id.clone(),
            task_id: String::new(),
            task_item_id: None,
            step_id: None,
            session_id: None,
            kind: "source_connection_provisioning_attention".into(),
            severity: AttentionSeverity::Intervention,
            title: "Dedicated Slack App provisioning needs a decision".into(),
            summary: format!(
                "Provisioning checkpoint {} stopped with {}. Resume only when offered or abandon it; automatic App recreation is disabled.",
                updated.id, error_code
            ),
            requested_decision: Some(serde_json::json!({
                "provisioning_id": updated.id,
                "safe_error_code": error_code,
                "choices": ["resume_if_available", "abandon_and_review_orphan_app"]
            })),
            actions: vec![],
            dedupe_key: format!("source-connection-provisioning:{}", updated.id),
            source_event_id: format!(
                "dedicated-provisioning:{}:{}",
                updated.id, updated.updated_at
            ),
            source_route_id: None,
            source_binding_name: None,
            occurred_at: updated.updated_at.clone(),
            sla_deadline: None,
        })
        .await
        .map_err(internal)?;
    Ok(updated)
}

pub(super) async fn resolve_dedicated_attention(
    server: &OrchestratorServer,
    project_id: &str,
    provisioning_id: &str,
    reason: &str,
) -> Result<(), Status> {
    server
        .state
        .attention_repo
        .resolve_external_candidate(
            project_id,
            &format!("source-connection-provisioning:{provisioning_id}"),
            &format!("dedicated-provisioning:{provisioning_id}:resolved"),
            reason,
        )
        .await
        .map(|_| ())
        .map_err(internal)
}
