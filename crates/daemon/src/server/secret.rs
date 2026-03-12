use agent_orchestrator::secret_key_audit;
use agent_orchestrator::secret_key_lifecycle;
use agent_orchestrator::secret_store_crypto::SecretEncryption;
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{authorize, map_core_error, OrchestratorServer};

fn map_key_record(r: &secret_key_lifecycle::KeyRecord) -> SecretKeyRecord {
    SecretKeyRecord {
        key_id: r.key_id.clone(),
        state: r.state.as_str().to_string(),
        fingerprint: r.fingerprint.clone(),
        file_path: r.file_path.clone(),
        created_at: r.created_at.clone(),
        activated_at: r.activated_at.clone(),
        rotated_out_at: r.rotated_out_at.clone(),
        retired_at: r.retired_at.clone(),
        revoked_at: r.revoked_at.clone(),
    }
}

pub(crate) async fn secret_key_status(
    server: &OrchestratorServer,
    request: Request<SecretKeyStatusRequest>,
) -> Result<Response<SecretKeyStatusResponse>, Status> {
    authorize(server, &request, "SecretKeyStatus").map_err(Status::from)?;

    let keyring = secret_key_lifecycle::load_keyring(&server.state.app_root, &server.state.db_path)
        .map_err(|e| {
            map_core_error(agent_orchestrator::error::classify_secret_error(
                "secret.status",
                e,
            ))
        })?;

    let active_key = keyring.active_record().map(map_key_record);
    let all_keys = keyring.all_records().iter().map(map_key_record).collect();

    Ok(Response::new(SecretKeyStatusResponse {
        active_key,
        all_keys,
    }))
}

pub(crate) async fn secret_key_list(
    server: &OrchestratorServer,
    request: Request<SecretKeyListRequest>,
) -> Result<Response<SecretKeyListResponse>, Status> {
    authorize(server, &request, "SecretKeyList").map_err(Status::from)?;

    let keyring = secret_key_lifecycle::load_keyring(&server.state.app_root, &server.state.db_path)
        .map_err(|e| {
            map_core_error(agent_orchestrator::error::classify_secret_error(
                "secret.list",
                e,
            ))
        })?;

    let keys = keyring.all_records().iter().map(map_key_record).collect();
    Ok(Response::new(SecretKeyListResponse { keys }))
}

pub(crate) async fn secret_key_rotate(
    server: &OrchestratorServer,
    request: Request<SecretKeyRotateRequest>,
) -> Result<Response<SecretKeyRotateResponse>, Status> {
    authorize(server, &request, "SecretKeyRotate").map_err(Status::from)?;

    let req = request.into_inner();
    let conn = agent_orchestrator::db::open_conn(&server.state.db_path).map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.rotate",
            e,
        ))
    })?;

    if req.resume {
        let report =
            secret_key_lifecycle::resume_rotation(&conn, &server.state.app_root).map_err(|e| {
                map_core_error(agent_orchestrator::error::classify_secret_error(
                    "secret.rotate",
                    e,
                ))
            })?;
        return Ok(Response::new(SecretKeyRotateResponse {
            message: if report.errors.is_empty() {
                "rotation resumed and completed successfully".to_string()
            } else {
                "rotation resumed with errors".to_string()
            },
            resources_updated: report.resources_updated as u64,
            versions_updated: report.versions_updated as u64,
            errors: report.errors,
        }));
    }

    // Begin new rotation
    let (new_rec, old_rec) = secret_key_lifecycle::begin_rotation(&conn, &server.state.app_root)
        .map_err(|e| {
            map_core_error(agent_orchestrator::error::classify_secret_error(
                "secret.rotate",
                e,
            ))
        })?;

    // Re-encrypt with new key
    let old_key_path = server.state.app_root.join(&old_rec.file_path);
    let new_key_path = server.state.app_root.join(&new_rec.file_path);

    let old_handle = agent_orchestrator::secret_store_crypto::load_key_file_as_handle(
        &old_key_path,
        &old_rec.key_id,
    )
    .map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.rotate",
            e,
        ))
    })?;
    let new_handle = agent_orchestrator::secret_store_crypto::load_key_file_as_handle(
        &new_key_path,
        &new_rec.key_id,
    )
    .map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.rotate",
            e,
        ))
    })?;

    let report = secret_key_lifecycle::re_encrypt_all_secrets(
        &conn,
        &SecretEncryption::from_key(old_handle),
        &SecretEncryption::from_key(new_handle),
    )
    .map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.rotate",
            e,
        ))
    })?;

    // Complete rotation if no errors
    if report.errors.is_empty() {
        secret_key_lifecycle::complete_rotation(&conn, &old_rec.key_id).map_err(|e| {
            map_core_error(agent_orchestrator::error::classify_secret_error(
                "secret.rotate",
                e,
            ))
        })?;
    }

    Ok(Response::new(SecretKeyRotateResponse {
        message: if report.errors.is_empty() {
            format!(
                "rotation complete: new key '{}', old key '{}' retired",
                new_rec.key_id, old_rec.key_id
            )
        } else {
            format!(
                "rotation partially complete with {} errors; old key '{}' remains decrypt_only",
                report.errors.len(),
                old_rec.key_id
            )
        },
        resources_updated: report.resources_updated as u64,
        versions_updated: report.versions_updated as u64,
        errors: report.errors,
    }))
}

pub(crate) async fn secret_key_revoke(
    server: &OrchestratorServer,
    request: Request<SecretKeyRevokeRequest>,
) -> Result<Response<SecretKeyRevokeResponse>, Status> {
    authorize(server, &request, "SecretKeyRevoke").map_err(Status::from)?;

    let req = request.into_inner();
    let conn = agent_orchestrator::db::open_conn(&server.state.db_path).map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.revoke",
            e,
        ))
    })?;

    secret_key_lifecycle::revoke_key(&conn, &req.key_id, req.force).map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.revoke",
            e,
        ))
    })?;

    Ok(Response::new(SecretKeyRevokeResponse {
        message: format!("key '{}' revoked", req.key_id),
    }))
}

pub(crate) async fn secret_key_history(
    server: &OrchestratorServer,
    request: Request<SecretKeyHistoryRequest>,
) -> Result<Response<SecretKeyHistoryResponse>, Status> {
    authorize(server, &request, "SecretKeyHistory").map_err(Status::from)?;

    let req = request.into_inner();
    let conn = agent_orchestrator::db::open_conn(&server.state.db_path).map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.history",
            e,
        ))
    })?;

    let events = if let Some(key_id) = &req.key_id {
        secret_key_audit::query_key_audit_events_for_key(&conn, key_id, req.limit as usize)
    } else {
        secret_key_audit::query_key_audit_events(&conn, req.limit as usize)
    }
    .map_err(|e| {
        map_core_error(agent_orchestrator::error::classify_secret_error(
            "secret.history",
            e,
        ))
    })?;

    let proto_events = events
        .iter()
        .map(|e| SecretKeyAuditEvent {
            event_kind: e.event_kind.as_str().to_string(),
            key_id: e.key_id.clone(),
            key_fingerprint: e.key_fingerprint.clone(),
            actor: e.actor.clone(),
            detail_json: e.detail_json.clone(),
            created_at: e.created_at.clone(),
        })
        .collect();

    Ok(Response::new(SecretKeyHistoryResponse {
        events: proto_events,
    }))
}
