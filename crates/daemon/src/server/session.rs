use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use agent_orchestrator::config_ext::OrchestratorConfigExt;
use agent_orchestrator::config_load::read_active_config;
use agent_orchestrator::events::insert_event;
use agent_orchestrator::session_control_audit;
use agent_orchestrator::session_store::{self, SessionRow};
use futures::Stream;
use orchestrator_proto::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use super::action_audit::{self, ActionDescriptor};
use super::{OrchestratorServer, authorize, trusted_actor};

const LEASE_TTL_SECS: u64 = 30;
const MAX_INPUT_BYTES: usize = 4 * 1024;
const MAX_CHUNK_BYTES: usize = 64 * 1024;
const MAX_SESSION_READERS: usize = 8;

/// Per-session reader occupancy whose permits are released when streams end or disconnect.
#[derive(Default)]
pub(crate) struct SessionReadLimits {
    sessions: tokio::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl SessionReadLimits {
    async fn acquire(&self, session_id: &str) -> Result<tokio::sync::OwnedSemaphorePermit, Status> {
        let semaphore = self
            .sessions
            .lock()
            .await
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(MAX_SESSION_READERS)))
            .clone();
        semaphore
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("session reader limit reached"))
    }
}

fn ensure_read_enabled(server: &OrchestratorServer) -> Result<(), Status> {
    let enabled = read_active_config(&server.state)
        .map(|active| active.config.global_runtime_policy().session_read_enabled)
        .unwrap_or(false);
    enabled
        .then_some(())
        .ok_or_else(|| Status::permission_denied("session read APIs are disabled"))
}

fn ensure_control_enabled(server: &OrchestratorServer) -> Result<(), Status> {
    let enabled = read_active_config(&server.state)
        .map(|active| {
            active
                .config
                .global_runtime_policy()
                .session_control_enabled
        })
        .unwrap_or(false);
    enabled
        .then_some(())
        .ok_or_else(|| Status::permission_denied("session mutation APIs are disabled"))
}

pub(crate) type AgentSessionReadStream =
    Pin<Box<dyn Stream<Item = Result<AgentSessionOutputChunk, Status>> + Send>>;

fn to_proto(row: SessionRow) -> AgentSession {
    AgentSession {
        session_id: row.id,
        task_id: row.task_id,
        task_item_id: row.task_item_id,
        step_id: row.step_id,
        phase: row.phase,
        agent_id: row.agent_id,
        state: if row.state == "exited" {
            "closed".into()
        } else {
            row.state
        },
        pid: row.pid,
        writer_client_id: row.writer_client_id,
        writer_actor: row.writer_actor,
        writer_lease_expires_at: row.writer_lease_expires_at,
        writer_fencing_token: row.writer_fencing_token,
        state_version: row.state_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        ended_at: row.ended_at,
        exit_code: row.exit_code,
    }
}

async fn load(server: &OrchestratorServer, id: &str) -> Result<SessionRow, Status> {
    server
        .state
        .session_store
        .load_session(id)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::not_found("session not found"))
}

pub(crate) async fn list(
    server: &OrchestratorServer,
    request: Request<AgentSessionListRequest>,
) -> Result<Response<AgentSessionListResponse>, Status> {
    authorize(server, &request, "AgentSessionList").map_err(Status::from)?;
    ensure_read_enabled(server)?;
    let req = request.into_inner();
    let rows = session_store::list_sessions_async(
        &server.state.async_database,
        req.task_id,
        req.agent_id,
        req.state,
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(Response::new(AgentSessionListResponse {
        sessions: rows.into_iter().map(to_proto).collect(),
    }))
}

pub(crate) async fn get(
    server: &OrchestratorServer,
    request: Request<AgentSessionGetRequest>,
) -> Result<Response<AgentSessionGetResponse>, Status> {
    authorize(server, &request, "AgentSessionGet").map_err(Status::from)?;
    ensure_read_enabled(server)?;
    let id = request.into_inner().session_id;
    Ok(Response::new(AgentSessionGetResponse {
        session: Some(to_proto(load(server, &id).await?)),
    }))
}

pub(crate) async fn attach(
    server: &OrchestratorServer,
    mut request: Request<AgentSessionAttachRequest>,
) -> Result<Response<AgentSessionAttachResponse>, Status> {
    ensure_read_enabled(server)?;
    let requested_mode = if request.get_ref().mode.is_empty() {
        "reader"
    } else {
        request.get_ref().mode.as_str()
    };
    if requested_mode == "reader" {
        authorize(server, &request, "AgentSessionAttach").map_err(Status::from)?;
    } else if requested_mode != "writer" {
        return Err(Status::invalid_argument("mode must be reader or writer"));
    }
    if request.get_ref().client_id.trim().is_empty() {
        return Err(Status::invalid_argument("client_id is required"));
    }
    let row = load(server, &request.get_ref().session_id).await?;
    if !matches!(row.state.as_str(), "active" | "detached") {
        return Err(Status::failed_precondition("session is not attachable"));
    }
    let actor = trusted_actor(&request);
    if requested_mode == "reader" {
        let req = request.into_inner();
        server
            .state
            .session_store
            .attach_reader(&req.session_id, &req.client_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        emit(
            server,
            &row,
            "session_reader_attached",
            json!({"actor":actor,"client_id":req.client_id}),
        )
        .await;
        return Ok(Response::new(AgentSessionAttachResponse {
            session_id: req.session_id,
            client_id: req.client_id,
            mode: "reader".into(),
            writer_granted: false,
            fencing_token: None,
            lease_expires_at: None,
        }));
    }
    ensure_control_enabled(server)?;
    let context = request.get_ref().audit.clone();
    let session_id = request.get_ref().session_id.clone();
    let client_id = request.get_ref().client_id.clone();
    let project = session_project(server, &row).await?;
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AgentSessionWriterAttach",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "session",
            target_id: &session_id,
            action: "session.writer_attach",
            expected_version: Some(row.state_version.to_string()),
            fencing_token: None,
            canonical_request: json!({"client_id":client_id,"state_version":row.state_version}),
            fallback_reason_code: super::action_audit::FALLBACK_REASON_LEGACY_CLIENT,
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching writer attach already audited",
        )));
    }
    let req = request.into_inner();
    let id = req.session_id.clone();
    let client = req.client_id.clone();
    let actor2 = actor.clone();
    let lease = match session_store::acquire_writer_lease_async(
        &server.state.async_database,
        id,
        actor2,
        client,
        LEASE_TTL_SECS,
    )
    .await
    {
        Ok(lease) => lease,
        Err(error) => {
            return Err(attempt
                .failed(server, Status::internal(error.to_string()))
                .await);
        }
    };
    let Some(lease) = lease else {
        return Err(attempt
            .failed(
                server,
                Status::resource_exhausted("writer lease is held by another client"),
            )
            .await);
    };
    record_session_action(
        server,
        &row.id,
        &actor,
        Some(&req.client_id),
        "writer_attach",
        attempt
            .idempotency_key
            .as_deref()
            .unwrap_or(&attempt.request_id),
        &attempt.request_id,
        Some(lease.fencing_token),
        None,
        "accepted",
    )
    .await?;
    emit(
        server,
        &row,
        "session_writer_acquired",
        json!({"actor":actor,"client_id":req.client_id,"fencing_token":lease.fencing_token,"request_id":attempt.request_id}),
    )
    .await;
    attempt
        .succeeded(server, Some("session_control_action"), Some(&row.id))
        .await?;
    Ok(attempt.response(AgentSessionAttachResponse {
        session_id: req.session_id,
        client_id: req.client_id,
        mode: "writer".into(),
        writer_granted: true,
        fencing_token: Some(lease.fencing_token),
        lease_expires_at: Some(lease.expires_at),
    }))
}

pub(crate) async fn heartbeat(
    server: &OrchestratorServer,
    mut request: Request<AgentSessionHeartbeatRequest>,
) -> Result<Response<AgentSessionHeartbeatResponse>, Status> {
    ensure_control_enabled(server)?;
    let row = load(server, &request.get_ref().session_id).await?;
    let project = session_project(server, &row).await?;
    let context = request.get_ref().audit.clone();
    let session_id = request.get_ref().session_id.clone();
    let client_id = request.get_ref().client_id.clone();
    let fencing_token = request.get_ref().fencing_token;
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AgentSessionHeartbeat",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "session",
            target_id: &session_id,
            action: "session.heartbeat",
            expected_version: Some(row.state_version.to_string()),
            fencing_token: Some(fencing_token),
            canonical_request: json!({"client_id":client_id,"state_version":row.state_version}),
            fallback_reason_code: "lease_heartbeat",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: true,
        },
    )
    .await?;
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    let id = req.session_id.clone();
    let client = req.client_id.clone();
    let expires = match session_store::heartbeat_writer_async(
        &server.state.async_database,
        id,
        client,
        req.fencing_token,
        LEASE_TTL_SECS,
    )
    .await
    {
        Ok(Some(expires)) => expires,
        Ok(None) => {
            return Err(attempt
                .failed(
                    server,
                    Status::failed_precondition("writer lease is stale or expired"),
                )
                .await);
        }
        Err(error) => {
            return Err(attempt
                .failed(server, Status::internal(error.to_string()))
                .await);
        }
    };
    record_session_action(
        server,
        &row.id,
        &actor,
        Some(&req.client_id),
        "heartbeat",
        &attempt.request_id,
        &attempt.request_id,
        Some(req.fencing_token),
        None,
        "accepted",
    )
    .await?;
    attempt
        .succeeded(server, Some("session_control_action"), Some(&row.id))
        .await?;
    Ok(attempt.response(AgentSessionHeartbeatResponse {
        lease_expires_at: expires,
    }))
}

pub(crate) async fn detach(
    server: &OrchestratorServer,
    mut request: Request<AgentSessionDetachRequest>,
) -> Result<Response<AgentSessionDetachResponse>, Status> {
    if request.get_ref().mode != "writer" {
        authorize(server, &request, "AgentSessionDetach").map_err(Status::from)?;
        let req = request.into_inner();
        let id = req.session_id;
        let client = req.client_id;
        let reason = req.reason;
        session_store::detach_reader(
            &server.state.async_database,
            id,
            client,
            agent_orchestrator::config_load::now_ts(),
            reason,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        return Ok(Response::new(AgentSessionDetachResponse { detached: true }));
    }
    ensure_control_enabled(server)?;
    let token = request
        .get_ref()
        .fencing_token
        .ok_or_else(|| Status::invalid_argument("fencing_token is required for writer detach"))?;
    let row = load(server, &request.get_ref().session_id).await?;
    let project = session_project(server, &row).await?;
    let context = request.get_ref().audit.clone();
    let session_id = request.get_ref().session_id.clone();
    let client_id = request.get_ref().client_id.clone();
    let reason = request.get_ref().reason.clone();
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AgentSessionWriterDetach",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "session",
            target_id: &session_id,
            action: "session.writer_detach",
            expected_version: Some(row.state_version.to_string()),
            fencing_token: Some(token),
            canonical_request: json!({"client_id":client_id,"reason":reason,"state_version":row.state_version}),
            fallback_reason_code: super::action_audit::FALLBACK_REASON_LEGACY_CLIENT,
            fallback_operator_reason: Some(&reason),
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching writer detach already audited",
        )));
    }
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    let id = req.session_id.clone();
    let client = req.client_id.clone();
    let reason = req.reason.clone();
    let detached = match session_store::release_writer_async(
        &server.state.async_database,
        id,
        client,
        token,
        reason,
    )
    .await
    {
        Ok(detached) => detached,
        Err(error) => {
            return Err(attempt
                .failed(server, Status::internal(error.to_string()))
                .await);
        }
    };
    if !detached {
        return Err(attempt
            .failed(server, Status::failed_precondition("writer lease is stale"))
            .await);
    }
    record_session_action(
        server,
        &row.id,
        &actor,
        Some(&req.client_id),
        "writer_detach",
        attempt
            .idempotency_key
            .as_deref()
            .unwrap_or(&attempt.request_id),
        &attempt.request_id,
        Some(token),
        Some(&req.reason),
        "accepted",
    )
    .await?;
    emit(
        server,
        &row,
        "session_writer_detached",
        json!({"actor":actor,"client_id":req.client_id,"fencing_token":token,"request_id":attempt.request_id}),
    )
    .await;
    attempt
        .succeeded(server, Some("session_control_action"), Some(&row.id))
        .await?;
    Ok(attempt.response(AgentSessionDetachResponse { detached: true }))
}

pub(crate) async fn send_input(
    server: &OrchestratorServer,
    mut request: Request<AgentSessionSendInputRequest>,
) -> Result<Response<AgentSessionSendInputResponse>, Status> {
    ensure_control_enabled(server)?;
    if request.get_ref().input.is_empty() || request.get_ref().input.len() > MAX_INPUT_BYTES {
        return Err(Status::invalid_argument(
            "input must be between 1 and 4096 bytes",
        ));
    }
    if request.get_ref().idempotency_key.trim().is_empty() {
        return Err(Status::invalid_argument("idempotency_key is required"));
    }
    let row = load(server, &request.get_ref().session_id).await?;
    let project = session_project(server, &row).await?;
    let context = request.get_ref().audit.clone();
    let session_id = request.get_ref().session_id.clone();
    let client_id = request.get_ref().client_id.clone();
    let token = request.get_ref().fencing_token;
    let key = request.get_ref().idempotency_key.clone();
    let input_len = request.get_ref().input.len();
    let input_fingerprint = hex::encode(Sha256::digest(&request.get_ref().input));
    let replay_fingerprint = input_fingerprint.clone();
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AgentSessionSendInput",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "session",
            target_id: &session_id,
            action: "session.send_input",
            expected_version: Some(row.state_version.to_string()),
            fencing_token: Some(token),
            canonical_request: json!({"client_id":client_id,"input_bytes":input_len,"input_fingerprint":input_fingerprint}),
            fallback_reason_code: super::action_audit::FALLBACK_REASON_LEGACY_CLIENT,
            fallback_operator_reason: None,
            fallback_idempotency_key: Some(&key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        let replay_session_id = session_id.clone();
        let replay_key = key.clone();
        let replay = session_control_audit::read_prior_outcome(
            &server.state.async_database,
            replay_session_id,
            replay_key,
        )
        .await
        .map_err(|error| attempt.status(Status::internal(error.to_string())))?;
        return match replay {
            Some((stored_hash, result))
                if stored_hash == replay_fingerprint && result == "accepted" =>
            {
                Ok(attempt.response(AgentSessionSendInputResponse {
                    accepted_bytes: input_len as u64,
                }))
            }
            Some((stored_hash, _)) if stored_hash != replay_fingerprint => Err(attempt.status(
                Status::aborted("idempotency key was used for different input"),
            )),
            Some((_, result)) if result == "reserved" => Err(attempt.status(Status::aborted(
                "matching input request is still in progress",
            ))),
            Some((_, result)) if result == "failed" => Err(attempt.status(Status::unavailable(
                "previous matching input request failed before retry could be audited",
            ))),
            _ => Err(attempt.status(Status::already_exists(
                "matching session input already audited",
            ))),
        };
    }
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    if !process_identity_matches(&row) {
        return Err(attempt
            .failed(
                server,
                Status::failed_precondition("session process identity cannot be verified"),
            )
            .await);
    }
    let id = req.session_id.clone();
    let client = req.client_id.clone();
    let token = req.fencing_token;
    let valid =
        session_store::validate_writer_async(&server.state.async_database, id, client, token)
            .await
            .map_err(|e| attempt.status(Status::internal(e.to_string())))?;
    if !valid {
        return Err(attempt
            .failed(
                server,
                Status::failed_precondition("writer fencing token is stale or expired"),
            )
            .await);
    }
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let actor2 = actor.clone();
    let client2 = req.client_id.clone();
    let len = req.input.len() as u64;
    let request_hash = hex::encode(Sha256::digest(&req.input));
    let insert_hash = request_hash.clone();
    let audit_request_id = attempt.request_id.clone();
    let reservation = session_control_audit::reserve_send_input(
        &server.state.async_database,
        session_control_audit::SendInputReservation {
            session_id: sid,
            actor: actor2,
            client_id: client2,
            idempotency_key: key,
            request_hash: insert_hash,
            fencing_token: token,
            created_at: agent_orchestrator::config_load::now_ts(),
            request_id: audit_request_id,
        },
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    let mut owns_reservation = matches!(reservation, session_control_audit::Reservation::Reserved);
    if let session_control_audit::Reservation::Replayed {
        request_hash: stored_hash,
        result,
    } = reservation
    {
        if stored_hash != request_hash {
            return Err(Status::aborted(
                "idempotency key was used for different input",
            ));
        }
        match result.as_str() {
            "accepted" => {
                return Ok(Response::new(AgentSessionSendInputResponse {
                    accepted_bytes: len,
                }));
            }
            "reserved" => {
                return Err(Status::aborted(
                    "matching input request is still in progress",
                ));
            }
            "failed" => {
                let sid = req.session_id.clone();
                let key = req.idempotency_key.clone();
                let request_id = attempt.request_id.clone();
                owns_reservation = session_control_audit::reclaim_failed_reservation(
                    &server.state.async_database,
                    sid,
                    key,
                    request_id,
                    agent_orchestrator::config_load::now_ts(),
                )
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
                if !owns_reservation {
                    return Err(Status::aborted(
                        "matching input request was concurrently retried",
                    ));
                }
            }
            _ => {
                return Err(Status::unavailable(
                    "previous matching input request is unknown",
                ));
            }
        }
    }
    debug_assert!(owns_reservation);
    let write_result = write_fifo_atomically(&row.input_fifo_path, &req.input);
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let result = if write_result.is_ok() {
        "accepted"
    } else {
        "failed"
    }
    .to_owned();
    session_control_audit::record_result(&server.state.async_database, sid, key, result)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    if let Err(error) = write_result {
        return Err(attempt
            .failed(
                server,
                Status::unavailable(format!("session input transport failed: {error}")),
            )
            .await);
    }
    emit(
        server,
        &row,
        "session_input_accepted",
        json!({"actor":actor,"client_id":req.client_id,"bytes":len,"fencing_token":token,"request_id":attempt.request_id}),
    )
    .await;
    attempt
        .succeeded(server, Some("session_control_action"), Some(&row.id))
        .await?;
    Ok(attempt.response(AgentSessionSendInputResponse {
        accepted_bytes: len,
    }))
}

pub(crate) async fn read(
    server: &OrchestratorServer,
    request: Request<AgentSessionReadRequest>,
) -> Result<Response<AgentSessionReadStream>, Status> {
    authorize(server, &request, "AgentSessionRead").map_err(Status::from)?;
    ensure_read_enabled(server)?;
    let req = request.into_inner();
    let row = load(server, &req.session_id).await?;
    let project_id = session_project(server, &row).await?;
    let reader_permit = server.session_read_limits.acquire(&req.session_id).await?;
    let path = if std::path::Path::new(&row.transcript_path).exists() {
        row.transcript_path.clone()
    } else {
        row.stdout_path.clone()
    };
    let patterns = read_active_config(&server.state)
        .map(|active| {
            active
                .config
                .runtime_policy_for_project(&project_id)
                .runner
                .redaction_patterns
        })
        .unwrap_or_default();
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let session_id = req.session_id.clone();
    let follow = req.follow;
    let chunk = if req.max_chunk_bytes == 0 {
        MAX_CHUNK_BYTES
    } else {
        (req.max_chunk_bytes as usize).clamp(1, MAX_CHUNK_BYTES)
    };
    let terminal = matches!(row.state.as_str(), "closed" | "failed" | "exited");
    let session_store = server.state.session_store.clone();
    tokio::spawn(async move {
        let _reader_permit = reader_permit;
        let mut offset = req.offset;
        loop {
            let result = read_transcript_chunk(std::path::Path::new(&path), offset, chunk);
            match result {
                Ok(Some(bytes)) => {
                    let raw = String::from_utf8_lossy(&bytes);
                    let text = agent_orchestrator::runner::redact_text(&raw, &patterns);
                    let next = offset + bytes.len() as u64;
                    let redacted = text.as_bytes() != bytes.as_slice();
                    if tx
                        .send(Ok(AgentSessionOutputChunk {
                            session_id: session_id.clone(),
                            offset,
                            next_offset: next,
                            timestamp: None,
                            stream: "transcript".into(),
                            data: text.as_bytes().to_vec(),
                            text,
                            eof: false,
                            redacted,
                        }))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    offset = next
                }
                Ok(None) => {
                    let now_terminal = if terminal {
                        true
                    } else {
                        session_store
                            .load_session(&session_id)
                            .await
                            .ok()
                            .flatten()
                            .is_some_and(|row| {
                                matches!(row.state.as_str(), "closed" | "failed" | "exited")
                            })
                    };
                    if !follow || now_terminal {
                        let _ = tx
                            .send(Ok(AgentSessionOutputChunk {
                                session_id: session_id.clone(),
                                offset,
                                next_offset: offset,
                                timestamp: None,
                                stream: "transcript".into(),
                                data: vec![],
                                text: String::new(),
                                eof: true,
                                redacted: false,
                            }))
                            .await;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(Status::unavailable(format!(
                            "transcript unavailable: {e}"
                        ))))
                        .await;
                    break;
                }
            }
        }
    });
    Ok(Response::new(Box::pin(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    )))
}

fn read_transcript_chunk(
    path: &std::path::Path,
    offset: u64,
    max_chunk_bytes: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if offset >= len {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0; max_chunk_bytes.min((len - offset) as usize)];
    let count = file.read(&mut buffer)?;
    buffer.truncate(count);
    Ok(Some(buffer))
}

fn write_fifo_atomically(path: &str, input: &[u8]) -> std::io::Result<()> {
    if input.is_empty() || input.len() > MAX_INPUT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "session input exceeds the atomic FIFO write boundary",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).custom_flags(libc::O_NONBLOCK);
    let mut fifo = options.open(path)?;
    let written = fifo.write(input)?;
    if written != input.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "session input transport accepted a partial atomic write",
        ));
    }
    Ok(())
}

pub(crate) async fn close(
    server: &OrchestratorServer,
    mut request: Request<AgentSessionCloseRequest>,
) -> Result<Response<AgentSessionCloseResponse>, Status> {
    ensure_control_enabled(server)?;
    if request.get_ref().reason.trim().is_empty()
        || request.get_ref().idempotency_key.trim().is_empty()
    {
        return Err(Status::invalid_argument(
            "reason and idempotency_key are required",
        ));
    }
    let row = load(server, &request.get_ref().session_id).await?;
    let project = session_project(server, &row).await?;
    let context = request.get_ref().audit.clone();
    let session_id = request.get_ref().session_id.clone();
    let reason = request.get_ref().reason.clone();
    let key = request.get_ref().idempotency_key.clone();
    let expected = request.get_ref().expected_state_version;
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AgentSessionClose",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "session",
            target_id: &session_id,
            action: "session.close",
            expected_version: expected.map(|value| value.to_string()),
            fencing_token: None,
            canonical_request: json!({"reason":reason,"expected_state_version":expected}),
            fallback_reason_code: super::action_audit::FALLBACK_REASON_LEGACY_CLIENT,
            fallback_operator_reason: Some(&reason),
            fallback_idempotency_key: Some(&key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching session close already audited",
        )));
    }
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    if let Some(v) = req.expected_state_version
        && v != row.state_version
    {
        return Err(attempt
            .failed(server, Status::aborted("session state version changed"))
            .await);
    }
    if !process_identity_matches(&row) {
        return Err(attempt
            .failed(
                server,
                Status::failed_precondition("session process identity cannot be verified"),
            )
            .await);
    }
    let request_hash = hex::encode(Sha256::digest(
        format!("{}:{:?}", req.reason, req.expected_state_version).as_bytes(),
    ));
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let actor2 = actor.clone();
    let reason = req.reason.clone();
    let hash = request_hash.clone();
    let audit_request_id = attempt.request_id.clone();
    let reservation = session_control_audit::reserve_close(
        &server.state.async_database,
        session_control_audit::CloseReservation {
            session_id: sid,
            actor: actor2,
            idempotency_key: key,
            request_hash: hash,
            reason,
            created_at: agent_orchestrator::config_load::now_ts(),
            request_id: audit_request_id,
        },
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    if let session_control_audit::Reservation::Replayed {
        request_hash: stored,
        ..
    } = reservation
    {
        if stored != request_hash {
            return Err(attempt
                .failed(
                    server,
                    Status::aborted("idempotency key was used for a different close request"),
                )
                .await);
        }
        attempt
            .succeeded(server, Some("session_control_action"), Some(&row.id))
            .await?;
        return Ok(attempt.response(AgentSessionCloseResponse {
            session: Some(to_proto(load(server, &row.id).await?)),
        }));
    }
    server
        .state
        .session_store
        .update_session_state(&row.id, "draining", None, false)
        .await
        .map_err(|e| attempt.status(Status::internal(e.to_string())))?;
    let (signal_result, signalled_group) = terminate_session_process(row.pid);
    if let Err(error) = signal_result {
        let original_state = row.state.clone();
        let _ = server
            .state
            .session_store
            .update_session_state(&row.id, &original_state, None, false)
            .await;
        let sid = req.session_id.clone();
        let key = req.idempotency_key.clone();
        let _ = session_control_audit::record_failed(&server.state.async_database, sid, key).await;
        return Err(attempt
            .failed(
                server,
                Status::unavailable(format!("failed to close session process: {error}")),
            )
            .await);
    }
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let _ = session_control_audit::record_accepted(&server.state.async_database, sid, key).await;
    emit(
        server,
        &row,
        "session_close_requested",
        json!({
            "actor": actor,
            "reason": req.reason,
            "request_id": attempt.request_id,
            "signalled_process_group": signalled_group,
        }),
    )
    .await;
    attempt
        .succeeded(server, Some("session_control_action"), Some(&row.id))
        .await?;
    Ok(attempt.response(AgentSessionCloseResponse {
        session: Some(to_proto(load(server, &row.id).await?)),
    }))
}

/// Sends `SIGTERM` to a session, preferring its whole process group.
///
/// Returns the signal result and whether the group form was used.
///
/// Sessions are spawned with `process_group(0)` precisely so that a group signal
/// can take a session and everything it spawned in one go, and the close path
/// was the one place not using it: signalling only the leader leaves any child
/// the session started running and reparented, which is how orphans with dead
/// leaders and live descendants are made (FR-159).
///
/// The group form is used only when the PID provably leads its own group. When
/// it does not — a recorded PID that has been reused, or a platform that cannot
/// answer — this falls back to the single-process signal, which is exactly the
/// previous behaviour. Negating a PID that is not a group leader would deliver
/// the signal to an unrelated group, so the fallback is the safe direction: it
/// can under-reach, never over-reach.
fn terminate_session_process(pid: i64) -> (nix::Result<()>, bool) {
    let target = if session_store::is_process_group_leader(pid) {
        (-(pid as i32), true)
    } else {
        (pid as i32, false)
    };
    let result = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(target.0),
        nix::sys::signal::Signal::SIGTERM,
    );
    (result, target.1)
}

pub(crate) async fn resolve_pid(
    server: &OrchestratorServer,
    request: Request<AgentSessionResolvePidRequest>,
) -> Result<Response<AgentSessionResolvePidResponse>, Status> {
    authorize(server, &request, "AgentSessionResolvePid").map_err(Status::from)?;
    let pid = request.into_inner().pid;
    ensure_read_enabled(server)?;
    let rows = session_store::list_sessions_by_pid_async(&server.state.async_database, pid)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    Ok(Response::new(AgentSessionResolvePidResponse {
        sessions: rows.into_iter().map(to_proto).collect(),
    }))
}

fn process_identity_matches(row: &SessionRow) -> bool {
    session_store::process_identity_status(row.pid, row.process_fingerprint.as_deref())
        == session_store::ProcessIdentityStatus::VerifiedLive
}

async fn session_project(server: &OrchestratorServer, row: &SessionRow) -> Result<String, Status> {
    agent_orchestrator::task_repository::queries::project_id_for_task(
        &server.state.async_database,
        row.task_id.clone(),
    )
    .await
    .map_err(|error| Status::internal(error.to_string()))
}

#[allow(clippy::too_many_arguments)]
async fn record_session_action(
    server: &OrchestratorServer,
    session_id: &str,
    actor: &str,
    client_id: Option<&str>,
    action: &str,
    idempotency_key: &str,
    request_id: &str,
    fencing_token: Option<i64>,
    reason: Option<&str>,
    result: &str,
) -> Result<(), Status> {
    let session_id = session_id.to_string();
    let actor = actor.to_string();
    let client_id = client_id.map(str::to_owned);
    let action = action.to_string();
    let idempotency_key = idempotency_key.to_string();
    let request_id = request_id.to_string();
    let reason = reason.map(str::to_owned);
    let result = result.to_string();
    let request_hash = hex::encode(Sha256::digest(
        format!("{action}:{client_id:?}:{fencing_token:?}:{reason:?}").as_bytes(),
    ));
    session_control_audit::insert_terminal(
        &server.state.async_database,
        session_id,
        actor,
        client_id,
        action,
        Some(idempotency_key),
        request_hash,
        result,
        reason,
        fencing_token,
        agent_orchestrator::config_load::now_ts(),
        request_id,
    )
    .await
    .map_err(|error| Status::internal(error.to_string()))
}

async fn emit(
    server: &OrchestratorServer,
    row: &SessionRow,
    kind: &str,
    payload: serde_json::Value,
) {
    let _ = insert_event(
        &server.state,
        &row.task_id,
        row.task_item_id.as_deref(),
        kind,
        payload,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn per_session_reader_limit_releases_on_stream_drop() {
        let limits = SessionReadLimits::default();
        let mut permits = Vec::new();
        for _ in 0..MAX_SESSION_READERS {
            permits.push(limits.acquire("session-a").await.expect("reader permit"));
        }
        let denied = limits.acquire("session-a").await.unwrap_err();
        assert_eq!(denied.code(), tonic::Code::ResourceExhausted);

        drop(permits.pop());
        let _released_reader = limits
            .acquire("session-a")
            .await
            .expect("disconnected reader releases occupancy");
        let _independent_reader = limits
            .acquire("session-b")
            .await
            .expect("different session has an independent bound");
    }

    #[test]
    fn transcript_offsets_are_independent_and_chunk_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("transcript.log");
        std::fs::write(&path, b"alpha-beta-gamma").expect("write transcript");

        let reader_a = read_transcript_chunk(&path, 0, 5)
            .expect("reader a")
            .expect("reader a bytes");
        let reader_b = read_transcript_chunk(&path, 6, 4)
            .expect("reader b")
            .expect("reader b bytes");
        let reader_a_reconnect = read_transcript_chunk(&path, reader_a.len() as u64, 64)
            .expect("reader a reconnect")
            .expect("remaining bytes");

        assert_eq!(reader_a, b"alpha");
        assert_eq!(reader_b, b"beta");
        assert_eq!(reader_a_reconnect, b"-beta-gamma");
        assert!(read_transcript_chunk(&path, 16, 64).unwrap().is_none());
    }

    #[test]
    fn atomic_input_boundary_rejects_oversized_payload_before_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("capture");
        std::fs::write(&path, []).expect("create capture");

        write_fifo_atomically(path.to_str().unwrap(), b"hello").expect("bounded write");
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        let error = write_fifo_atomically(path.to_str().unwrap(), &vec![b'x'; MAX_INPUT_BYTES + 1])
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    }
}
