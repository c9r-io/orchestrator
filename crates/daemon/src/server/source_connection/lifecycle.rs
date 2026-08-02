//! Reviewed dedicated App lifecycle: manifest upgrade and App deletion.

use super::oauth::create_intent;
use super::projection::*;
use super::*;

pub(crate) async fn dedicated_upgrade_preview(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionDedicatedUpgradePreviewRequest>,
) -> Result<Response<SourceConnectionDedicatedLifecycleResponse>, Status> {
    let config_token = Zeroizing::new(std::mem::take(&mut request.get_mut().config_token));
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    validate_config_token(&config_token)?;
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
            "dedicated App upgrade requires one active managed_dedicated connection",
        ));
    }
    let identity = repository(server)
        .dedicated_app_identity_for_connection(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            Status::failed_precondition("dedicated App lifecycle identity is missing")
        })?;
    let app_id = Zeroizing::new(
        encryption(server)?
            .decrypt_source_connection_credential(
                &req.project_id,
                &identity.provisioning_id,
                &identity.app_id_ciphertext,
            )
            .map_err(internal)?,
    );
    let current_manifest = server
        .slack_manifest_client
        .export_manifest(&config_token, &app_id)
        .await
        .map_err(|error| unavailable(anyhow::Error::new(error)))?;
    let gateway = server
        .slack_gateway
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?;
    let (oauth_callback_url, events_url) =
        dedicated_urls(gateway.origin(), &identity.provisioning_id)?;
    let mut target_manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/dedicated-app-manifest.json"))
            .map_err(internal)?;
    render_manifest_endpoints(&mut target_manifest, &oauth_callback_url, &events_url)
        .map_err(internal)?;
    reviewed_manifest_contract(&target_manifest).map_err(internal)?;
    server
        .slack_manifest_client
        .validate_manifest(&config_token, &target_manifest)
        .await
        .map_err(|error| unavailable(anyhow::Error::new(error)))?;
    let diff = semantic_manifest_diff(&current_manifest, &target_manifest)?;
    let permission_expansion = diff.iter().any(|entry| entry.permission_expansion);
    let manifest_digest = hex::encode(Sha256::digest(
        serde_json::to_vec(&target_manifest).map_err(internal)?,
    ));
    let lifecycle_id = format!("dedicated-lifecycle-{}", Uuid::new_v4());
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionDedicatedUpgradePreview",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection",
            target_id: &req.id,
            action: "source.connection.dedicated.upgrade.preview",
            expected_version: Some(req.expected_version.to_string()),
            fencing_token: Some(current.generation),
            canonical_request: serde_json::json!({
                "connection_id": req.id,
                "manifest_version": DEDICATED_MANIFEST_VERSION,
                "manifest_digest": manifest_digest,
                "permission_expansion": permission_expansion,
            }),
            fallback_reason_code: "dedicated_slack_app_lifecycle",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("upgrade preview already processed")));
    }
    server.dedicated_lifecycle_sessions.lock().await.insert(
        lifecycle_id.clone(),
        DedicatedLifecycleSession {
            project_id: req.project_id,
            connection_id: req.id.clone(),
            expected_version: req.expected_version,
            provisioning_id: identity.provisioning_id,
            app_id,
            app_id_digest: identity.app_id_digest,
            manifest: target_manifest,
            manifest_digest: manifest_digest.clone(),
            diff: diff.clone(),
            permission_expansion,
            config_token,
            expires_at: expires_at.clone(),
        },
    );
    attempt
        .succeeded(server, Some("source_connection"), Some(&req.id))
        .await?;
    Ok(attempt.response(dedicated_lifecycle_response(
        lifecycle_id,
        req.id,
        "awaiting_approval",
        manifest_digest,
        diff,
        permission_expansion,
        expires_at,
        None,
        Some(current),
    )))
}

pub(crate) async fn dedicated_upgrade_apply(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionDedicatedUpgradeApplyRequest>,
) -> Result<Response<SourceConnectionDedicatedLifecycleResponse>, Status> {
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    validate_id(&req.lifecycle_id, "lifecycle id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    let session = server
        .dedicated_lifecycle_sessions
        .lock()
        .await
        .remove(&req.lifecycle_id)
        .ok_or_else(|| Status::failed_precondition("dedicated_lifecycle_session_lost"))?;
    if session.project_id != req.project_id
        || session.connection_id != req.id
        || session.expected_version != req.expected_version
    {
        return Err(Status::permission_denied(
            "dedicated lifecycle session boundary mismatch",
        ));
    }
    if chrono::DateTime::parse_from_rfc3339(&session.expires_at)
        .map(|value| value < chrono::Utc::now())
        .unwrap_or(true)
    {
        return Err(Status::failed_precondition(
            "dedicated_lifecycle_session_expired",
        ));
    }
    let current = repository(server)
        .get(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection not found"))?;
    if current.version != req.expected_version
        || current.provisioning_mode != SourceConnectionMode::ManagedDedicated
        || current.state != SourceConnectionState::Active
    {
        return Err(Status::aborted(
            "SourceConnection changed after manifest review",
        ));
    }
    let daemon_id = repository(server).daemon_id().await.map_err(internal)?;
    let credential = repository(server)
        .credential(&req.project_id, &req.id, &daemon_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::failed_precondition("active connection credential missing"))?;
    let pairing = Zeroizing::new(
        encryption(server)?
            .decrypt_source_connection_credential(
                &req.project_id,
                &req.id,
                &credential.pairing_secret_ciphertext,
            )
            .map_err(internal)?,
    );
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionDedicatedUpgradeApply",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection",
            target_id: &req.id,
            action: "source.connection.dedicated.upgrade.apply",
            expected_version: Some(req.expected_version.to_string()),
            fencing_token: Some(current.generation),
            canonical_request: serde_json::json!({
                "connection_id": req.id,
                "lifecycle_id": req.lifecycle_id,
                "manifest_version": DEDICATED_MANIFEST_VERSION,
                "manifest_digest": session.manifest_digest,
                "permission_expansion": session.permission_expansion,
            }),
            fallback_reason_code: "dedicated_slack_app_lifecycle",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("upgrade already processed")));
    }
    let result = async {
        let provider_permissions_updated = server
            .slack_manifest_client
            .update_manifest(&session.config_token, &session.app_id, &session.manifest)
            .await
            .map_err(|error| unavailable(anyhow::Error::new(error)))?;
        let gateway = server
            .slack_gateway
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?;
        let gateway_connection = gateway
            .update_dedicated_manifest(
                &session.provisioning_id,
                &daemon_id,
                &req.project_id,
                &session.app_id_digest,
                DEDICATED_MANIFEST_VERSION,
                &session.manifest_digest,
            )
            .await
            .map_err(unavailable)?;
        if gateway_connection.id != current.installation_id
            || gateway_connection.version != current.version + 1
            || gateway_connection.app_id_digest.as_deref() != Some(session.app_id_digest.as_str())
        {
            return Err(Status::data_loss(
                "Gateway dedicated lifecycle projection mismatch",
            ));
        }
        let reauthorization_required = session.permission_expansion || provider_permissions_updated;
        let mut updated = repository(server)
            .update_dedicated_connection_lifecycle(UpdateDedicatedConnectionLifecycle {
                project_id: req.project_id.clone(),
                id: req.id.clone(),
                expected_version: req.expected_version,
                state: SourceConnectionState::Active,
                manifest_version: DEDICATED_MANIFEST_VERSION.into(),
                provision_state: "completed".into(),
                error_code: None,
                request_id: attempt.request_id.clone(),
            })
            .await
            .map_err(internal)?;
        let mut oauth = None;
        if reauthorization_required {
            let suspended = gateway
                .suspend(
                    &credential.installation_id,
                    &daemon_id,
                    updated.version,
                    &pairing,
                )
                .await
                .map_err(unavailable)?;
            if suspended.version != updated.version + 1 || suspended.state != "suspended" {
                return Err(Status::data_loss("Gateway suspension projection mismatch"));
            }
            updated = repository(server)
                .update_dedicated_connection_lifecycle(UpdateDedicatedConnectionLifecycle {
                    project_id: req.project_id.clone(),
                    id: req.id.clone(),
                    expected_version: updated.version,
                    state: SourceConnectionState::Suspended,
                    manifest_version: DEDICATED_MANIFEST_VERSION.into(),
                    provision_state: "reauthorization_required".into(),
                    error_code: Some("slack_manifest_reauthorization_required".into()),
                    request_id: attempt.request_id.clone(),
                })
                .await
                .map_err(internal)?;
            upsert_lifecycle_attention(server, &updated).await?;
            let intent = create_intent(
                server,
                &req.project_id,
                &updated.display_label,
                &attempt.request_id,
                Some(&session.provisioning_id),
                None,
            )
            .await?;
            oauth = Some((intent.id, intent.authorize_url.unwrap_or_default()));
        }
        Ok((updated, oauth, reauthorization_required))
    }
    .await;
    match result {
        Ok((updated, oauth, reauthorization_required)) => {
            attempt
                .succeeded(server, Some("source_connection"), Some(&updated.id))
                .await?;
            Ok(attempt.response(dedicated_lifecycle_response(
                req.lifecycle_id,
                req.id,
                if reauthorization_required {
                    "reauthorization_required"
                } else {
                    "completed"
                },
                session.manifest_digest,
                session.diff,
                reauthorization_required,
                session.expires_at,
                oauth,
                Some(updated),
            )))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

pub(crate) async fn dedicated_delete(
    server: &OrchestratorServer,
    mut request: Request<SourceConnectionDedicatedDeleteRequest>,
) -> Result<Response<SourceConnection>, Status> {
    let config_token = Zeroizing::new(std::mem::take(&mut request.get_mut().config_token));
    let typed_app_id = Zeroizing::new(std::mem::take(&mut request.get_mut().typed_app_id));
    let req = request.get_ref().clone();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    validate_mutation(&req.reason, &req.idempotency_key)?;
    validate_config_token(&config_token)?;
    let current = repository(server)
        .get(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection not found"))?;
    if current.version != req.expected_version {
        return Err(Status::aborted("SourceConnection version conflict"));
    }
    if current.provisioning_mode != SourceConnectionMode::ManagedDedicated
        || current.state != SourceConnectionState::Disconnected
    {
        return Err(Status::failed_precondition(
            "dedicated App deletion requires a disconnected managed_dedicated connection",
        ));
    }
    let identity = repository(server)
        .dedicated_app_identity_for_connection(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            Status::failed_precondition("dedicated App lifecycle identity is missing")
        })?;
    let app_id = Zeroizing::new(
        encryption(server)?
            .decrypt_source_connection_credential(
                &req.project_id,
                &identity.provisioning_id,
                &identity.app_id_ciphertext,
            )
            .map_err(internal)?,
    );
    if typed_app_id.as_str() != app_id.as_str() {
        return Err(Status::invalid_argument(
            "typed Slack App ID does not match the governed connection",
        ));
    }
    let context = audit_context(&req.reason, &req.idempotency_key);
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceConnectionDedicatedDelete",
        Some(&context),
        ActionDescriptor {
            project_id: &req.project_id,
            target_type: "source_connection",
            target_id: &req.id,
            action: "source.connection.dedicated.delete",
            expected_version: Some(req.expected_version.to_string()),
            fencing_token: Some(current.generation),
            canonical_request: serde_json::json!({
                "connection_id": req.id,
                "app_id_digest": identity.app_id_digest,
            }),
            fallback_reason_code: "dedicated_slack_app_delete",
            fallback_operator_reason: Some(&req.reason),
            fallback_idempotency_key: Some(&req.idempotency_key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists("App deletion already processed")));
    }
    let result = async {
        server
            .slack_manifest_client
            .export_manifest(&config_token, &app_id)
            .await
            .map_err(|error| unavailable(anyhow::Error::new(error)))?;
        server
            .slack_manifest_client
            .delete_manifest(&config_token, &app_id)
            .await
            .map_err(|error| unavailable(anyhow::Error::new(error)))?;
        let daemon_id = repository(server).daemon_id().await.map_err(internal)?;
        server
            .slack_gateway
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Slack Gateway is not configured"))?
            .retire_dedicated_app(
                &identity.provisioning_id,
                &daemon_id,
                &req.project_id,
                &identity.app_id_digest,
            )
            .await
            .map_err(unavailable)?;
        repository(server)
            .update_dedicated_connection_lifecycle(UpdateDedicatedConnectionLifecycle {
                project_id: req.project_id.clone(),
                id: req.id.clone(),
                expected_version: req.expected_version,
                state: SourceConnectionState::Disconnected,
                manifest_version: current
                    .manifest_version
                    .clone()
                    .unwrap_or_else(|| DEDICATED_MANIFEST_VERSION.into()),
                provision_state: "app_deleted".into(),
                error_code: None,
                request_id: attempt.request_id.clone(),
            })
            .await
            .map_err(internal)
    }
    .await;
    match result {
        Ok(updated) => {
            attempt
                .succeeded(server, Some("source_connection"), Some(&updated.id))
                .await?;
            Ok(attempt.response(connection_to_proto(updated)))
        }
        Err(status) => Err(attempt.failed(server, status).await),
    }
}

async fn upsert_lifecycle_attention(
    server: &OrchestratorServer,
    connection: &CoreConnection,
) -> Result<(), Status> {
    let digest = hex::encode(Sha256::digest(connection.id.as_bytes()));
    server
        .state
        .attention_repo
        .upsert_external_candidate(AttentionCandidate {
            id: format!("attention-dedicated-lifecycle-{}", &digest[..24]),
            project_id: connection.project_id.clone(),
            task_id: String::new(),
            task_item_id: None,
            step_id: None,
            session_id: None,
            kind: "source_connection_reauthorization_required".into(),
            severity: AttentionSeverity::Intervention,
            title: "Dedicated Slack App requires OAuth reauthorization".into(),
            summary: format!(
                "Connection {} is suspended because its reviewed manifest expanded permissions. Complete the pending Slack OAuth intent before delivery resumes.",
                connection.id
            ),
            requested_decision: Some(serde_json::json!({
                "connection_id": connection.id,
                "safe_error_code": "slack_manifest_reauthorization_required",
            })),
            actions: vec![],
            dedupe_key: format!("source-connection-lifecycle:{}", connection.id),
            source_event_id: format!(
                "dedicated-lifecycle:{}:{}",
                connection.id, connection.updated_at
            ),
            source_route_id: None,
            source_binding_name: None,
            occurred_at: connection.updated_at.clone(),
            sla_deadline: None,
        })
        .await
        .map_err(internal)?;
    Ok(())
}
