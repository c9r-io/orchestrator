//! Pure projections, manifest diffing, and the shared validation helpers.

use super::*;

pub(super) fn encryption(
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

pub(super) fn repository(server: &OrchestratorServer) -> AsyncSourceConnectionRepository {
    AsyncSourceConnectionRepository::new(server.state.async_database.clone())
}

pub(super) fn connection_to_proto(value: CoreConnection) -> SourceConnection {
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

pub(super) fn dedicated_urls(
    gateway_origin: &str,
    provisioning_id: &str,
) -> Result<(String, String), Status> {
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

pub(super) fn manifest_diff(
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

pub(super) fn semantic_manifest_diff(
    current: &serde_json::Value,
    target: &serde_json::Value,
) -> Result<Vec<SourceConnectionManifestDiffEntry>, Status> {
    let current_scopes = manifest_string_array(current, "/oauth_config/scopes/bot")?;
    let target_scopes = manifest_string_array(target, "/oauth_config/scopes/bot")?;
    let current_events =
        manifest_string_array(current, "/settings/event_subscriptions/bot_events")?;
    let target_events = manifest_string_array(target, "/settings/event_subscriptions/bot_events")?;
    let current_redirects = manifest_string_array(current, "/oauth_config/redirect_urls")?
        .into_iter()
        .map(|value| safe_url_origin(&value))
        .collect::<Vec<_>>();
    let target_redirects = manifest_string_array(target, "/oauth_config/redirect_urls")?
        .into_iter()
        .map(|value| safe_url_origin(&value))
        .collect::<Vec<_>>();
    let current_event_url = current
        .pointer("/settings/event_subscriptions/request_url")
        .and_then(serde_json::Value::as_str)
        .map(safe_url_origin)
        .into_iter()
        .collect::<Vec<_>>();
    let target_event_url = target
        .pointer("/settings/event_subscriptions/request_url")
        .and_then(serde_json::Value::as_str)
        .map(safe_url_origin)
        .into_iter()
        .collect::<Vec<_>>();
    let current_rotation = current
        .pointer("/settings/token_rotation_enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        .to_string();
    let target_rotation = target
        .pointer("/settings/token_rotation_enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        .to_string();
    Ok(vec![
        semantic_diff_entry("oauth.scopes.bot", current_scopes, target_scopes, true),
        semantic_diff_entry("events.bot_events", current_events, target_events, true),
        semantic_diff_entry(
            "oauth.redirect_url",
            current_redirects,
            target_redirects,
            true,
        ),
        semantic_diff_entry(
            "events.request_url",
            current_event_url,
            target_event_url,
            true,
        ),
        semantic_diff_entry(
            "settings.token_rotation_enabled",
            vec![current_rotation],
            vec![target_rotation],
            false,
        ),
    ])
}

pub(super) fn manifest_string_array(
    manifest: &serde_json::Value,
    pointer: &str,
) -> Result<Vec<String>, Status> {
    let mut values = manifest
        .pointer(pointer)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Status::failed_precondition("exported Slack manifest is incomplete"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| Status::failed_precondition("exported Slack manifest is invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    values.sort();
    values.dedup();
    Ok(values)
}

pub(super) fn semantic_diff_entry(
    field: &str,
    before: Vec<String>,
    after: Vec<String>,
    expansion_sensitive: bool,
) -> SourceConnectionManifestDiffEntry {
    let added = after.iter().any(|value| !before.contains(value));
    let removed = before.iter().any(|value| !after.contains(value));
    let change = match (added, removed) {
        (false, false) => "unchanged",
        (true, false) => "add",
        (false, true) => "remove",
        (true, true) => "change",
    };
    SourceConnectionManifestDiffEntry {
        field: field.into(),
        change: change.into(),
        before,
        after,
        permission_expansion: expansion_sensitive && added,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dedicated_lifecycle_response(
    lifecycle_id: String,
    connection_id: String,
    status: &str,
    manifest_digest: String,
    diff: Vec<SourceConnectionManifestDiffEntry>,
    permission_expansion: bool,
    expires_at: String,
    oauth: Option<(String, String)>,
    connection: Option<CoreConnection>,
) -> SourceConnectionDedicatedLifecycleResponse {
    let (oauth_intent_id, authorize_url) = oauth
        .map(|(id, url)| (Some(id), Some(url)))
        .unwrap_or((None, None));
    SourceConnectionDedicatedLifecycleResponse {
        lifecycle_id,
        connection_id,
        status: status.into(),
        manifest_version: DEDICATED_MANIFEST_VERSION.into(),
        manifest_digest,
        diff,
        permission_expansion,
        expires_at,
        oauth_intent_id,
        authorize_url,
        connection: connection.map(connection_to_proto),
    }
}

pub(super) fn validate_config_token(value: &str) -> Result<(), Status> {
    if value.trim().is_empty() || value.len() > 8192 {
        return Err(Status::invalid_argument(
            "Configuration Token must contain 1-8192 characters",
        ));
    }
    Ok(())
}

pub(super) fn safe_url_origin(value: &str) -> String {
    url::Url::parse(value)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?.to_string();
            Some(format!("{}://{host}", url.scheme()))
        })
        .unwrap_or_else(|| "invalid-origin".into())
}

pub(super) fn dedicated_response(
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
        target_connection_id: value.target_connection_id,
    }
}

pub(super) fn intent_to_proto(value: CoreIntent) -> SourceConnectionIntentResponse {
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

pub(super) fn mode_capability(
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

pub(super) fn audit_context(reason: &str, idempotency_key: &str) -> ActionAuditContext {
    ActionAuditContext {
        reason_code: "managed_connection".to_string(),
        operator_reason: Some(reason.to_string()),
        idempotency_key: Some(idempotency_key.to_string()),
    }
}

pub(super) fn validate_project(value: &str) -> Result<(), Status> {
    validate_id(value, "project_id")
}

pub(super) fn validate_id(value: &str, label: &str) -> Result<(), Status> {
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

pub(super) fn validate_label(value: &str) -> Result<(), Status> {
    if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "display_label must contain 1-128 printable characters",
        ));
    }
    Ok(())
}

pub(super) fn validate_mutation(reason: &str, idempotency_key: &str) -> Result<(), Status> {
    if reason.trim().is_empty() || reason.len() > 500 {
        return Err(Status::invalid_argument(
            "reason must contain 1-500 characters",
        ));
    }
    validate_id(idempotency_key, "idempotency_key")
}

pub(super) fn internal(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

pub(super) fn unavailable(error: impl std::fmt::Display) -> Status {
    Status::unavailable(error.to_string())
}

pub(super) fn local_terminal_intent_status<'a>(
    gateway_status: &'a str,
    error_code: Option<&str>,
) -> &'a str {
    if gateway_status == "cancelled" {
        "cancelled"
    } else if gateway_status == "expired" || error_code == Some("oauth_intent_expired") {
        "expired"
    } else {
        "failed"
    }
}
