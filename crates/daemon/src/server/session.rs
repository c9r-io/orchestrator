use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::pin::Pin;
use std::time::Duration;

use agent_orchestrator::config_ext::OrchestratorConfigExt;
use agent_orchestrator::config_load::read_active_config;
use agent_orchestrator::events::insert_event;
use agent_orchestrator::session_store::{self, SessionRow};
use futures::Stream;
use orchestrator_proto::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use super::{OrchestratorServer, authorize, trusted_actor};

const LEASE_TTL_SECS: u64 = 30;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_CHUNK_BYTES: usize = 64 * 1024;

fn ensure_read_enabled(server: &OrchestratorServer) -> Result<(), Status> {
    let enabled = read_active_config(&server.state)
        .map(|active| active.config.runtime_policy().session_read_enabled)
        .unwrap_or(false);
    enabled
        .then_some(())
        .ok_or_else(|| Status::permission_denied("session read APIs are disabled"))
}

fn ensure_control_enabled(server: &OrchestratorServer) -> Result<(), Status> {
    let enabled = read_active_config(&server.state)
        .map(|active| active.config.runtime_policy().session_control_enabled)
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
    let rows = server
        .state
        .async_database
        .reader()
        .call(move |conn| {
            session_store::list_sessions(
                conn,
                req.task_id.as_deref(),
                req.agent_id.as_deref(),
                req.state.as_deref(),
            )
            .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
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
    request: Request<AgentSessionAttachRequest>,
) -> Result<Response<AgentSessionAttachResponse>, Status> {
    authorize(server, &request, "AgentSessionAttach").map_err(Status::from)?;
    ensure_read_enabled(server)?;
    let actor = trusted_actor(&request);
    let requested_mode = if request.get_ref().mode.is_empty() {
        "reader"
    } else {
        request.get_ref().mode.as_str()
    };
    if requested_mode == "writer" {
        ensure_control_enabled(server)?;
        authorize(server, &request, "AgentSessionSendInput").map_err(Status::from)?;
    }
    let req = request.into_inner();
    if req.client_id.trim().is_empty() {
        return Err(Status::invalid_argument("client_id is required"));
    }
    let mode = if req.mode.is_empty() {
        "reader"
    } else {
        req.mode.as_str()
    };
    let row = load(server, &req.session_id).await?;
    if !matches!(row.state.as_str(), "active" | "detached") {
        return Err(Status::failed_precondition("session is not attachable"));
    }
    if mode == "reader" {
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
            mode: mode.into(),
            writer_granted: false,
            fencing_token: None,
            lease_expires_at: None,
        }));
    }
    if mode != "writer" {
        return Err(Status::invalid_argument("mode must be reader or writer"));
    }
    let id = req.session_id.clone();
    let client = req.client_id.clone();
    let actor2 = actor.clone();
    let lease = server
        .state
        .async_database
        .writer()
        .call(move |conn| {
            session_store::acquire_writer_lease(conn, &id, &actor2, &client, LEASE_TTL_SECS)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
        .map_err(|e| Status::internal(e.to_string()))?;
    let lease = lease
        .ok_or_else(|| Status::resource_exhausted("writer lease is held by another client"))?;
    emit(
        server,
        &row,
        "session_writer_acquired",
        json!({"actor":actor,"client_id":req.client_id,"fencing_token":lease.fencing_token}),
    )
    .await;
    Ok(Response::new(AgentSessionAttachResponse {
        session_id: req.session_id,
        client_id: req.client_id,
        mode: mode.into(),
        writer_granted: true,
        fencing_token: Some(lease.fencing_token),
        lease_expires_at: Some(lease.expires_at),
    }))
}

pub(crate) async fn heartbeat(
    server: &OrchestratorServer,
    request: Request<AgentSessionHeartbeatRequest>,
) -> Result<Response<AgentSessionHeartbeatResponse>, Status> {
    authorize(server, &request, "AgentSessionHeartbeat").map_err(Status::from)?;
    ensure_control_enabled(server)?;
    let req = request.into_inner();
    let id = req.session_id.clone();
    let client = req.client_id.clone();
    let expires = server
        .state
        .async_database
        .writer()
        .call(move |conn| {
            session_store::heartbeat_writer(conn, &id, &client, req.fencing_token, LEASE_TTL_SECS)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::failed_precondition("writer lease is stale or expired"))?;
    Ok(Response::new(AgentSessionHeartbeatResponse {
        lease_expires_at: expires,
    }))
}

pub(crate) async fn detach(
    server: &OrchestratorServer,
    request: Request<AgentSessionDetachRequest>,
) -> Result<Response<AgentSessionDetachResponse>, Status> {
    authorize(server, &request, "AgentSessionDetach").map_err(Status::from)?;
    if request.get_ref().mode == "writer" {
        ensure_control_enabled(server)?;
        authorize(server, &request, "AgentSessionSendInput").map_err(Status::from)?;
    }
    let req = request.into_inner();
    if req.mode == "writer" {
        let token = req.fencing_token.ok_or_else(|| {
            Status::invalid_argument("fencing_token is required for writer detach")
        })?;
        let id = req.session_id.clone();
        let client = req.client_id.clone();
        let reason = req.reason.clone();
        let detached = server
            .state
            .async_database
            .writer()
            .call(move |conn| {
                session_store::release_writer(conn, &id, &client, token, &reason)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(agent_orchestrator::async_database::flatten_err)
            .map_err(|e| Status::internal(e.to_string()))?;
        if !detached {
            return Err(Status::failed_precondition("writer lease is stale"));
        }
        return Ok(Response::new(AgentSessionDetachResponse { detached: true }));
    }
    let id = req.session_id;
    let client = req.client_id;
    let reason = req.reason;
    server.state.async_database.writer().call(move|conn|{
        conn.execute("UPDATE session_attachments SET detached_at=?3,reason=?4 WHERE session_id=?1 AND client_id=?2 AND mode='reader' AND detached_at IS NULL",rusqlite::params![id,client,agent_orchestrator::config_load::now_ts(),reason])?;Ok(())
    }).await.map_err(agent_orchestrator::async_database::flatten_err).map_err(|e|Status::internal(e.to_string()))?;
    Ok(Response::new(AgentSessionDetachResponse { detached: true }))
}

pub(crate) async fn send_input(
    server: &OrchestratorServer,
    request: Request<AgentSessionSendInputRequest>,
) -> Result<Response<AgentSessionSendInputResponse>, Status> {
    authorize(server, &request, "AgentSessionSendInput").map_err(Status::from)?;
    ensure_control_enabled(server)?;
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    if req.input.is_empty() || req.input.len() > MAX_INPUT_BYTES {
        return Err(Status::invalid_argument(
            "input must be between 1 and 65536 bytes",
        ));
    }
    if req.idempotency_key.trim().is_empty() {
        return Err(Status::invalid_argument("idempotency_key is required"));
    }
    let row = load(server, &req.session_id).await?;
    if !process_identity_matches(&row) {
        return Err(Status::failed_precondition(
            "session process identity cannot be verified",
        ));
    }
    let id = req.session_id.clone();
    let client = req.client_id.clone();
    let token = req.fencing_token;
    let valid = server
        .state
        .async_database
        .reader()
        .call(move |conn| {
            session_store::validate_writer(conn, &id, &client, token)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
        .map_err(|e| Status::internal(e.to_string()))?;
    if !valid {
        return Err(Status::failed_precondition(
            "writer fencing token is stale or expired",
        ));
    }
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let actor2 = actor.clone();
    let client2 = req.client_id.clone();
    let len = req.input.len() as u64;
    let request_hash = hex::encode(Sha256::digest(&req.input));
    let insert_hash = request_hash.clone();
    let inserted=server.state.async_database.writer().call(move|conn|{
        let n=conn.execute("INSERT OR IGNORE INTO session_control_actions(session_id,actor,client_id,action,idempotency_key,request_hash,result,fencing_token,created_at) VALUES(?1,?2,?3,'send_input',?4,?5,'reserved',?6,?7)",rusqlite::params![sid,actor2,client2,key,insert_hash,token,agent_orchestrator::config_load::now_ts()])?;Ok(n==1)
    }).await.map_err(agent_orchestrator::async_database::flatten_err).map_err(|e|Status::internal(e.to_string()))?;
    if !inserted {
        let sid = req.session_id.clone();
        let key = req.idempotency_key.clone();
        let (stored_hash,result)=server.state.async_database.reader().call(move|conn|Ok(conn.query_row("SELECT request_hash,result FROM session_control_actions WHERE session_id=?1 AND idempotency_key=?2",rusqlite::params![sid,key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?))
            .await.map_err(agent_orchestrator::async_database::flatten_err).map_err(|e|Status::internal(e.to_string()))?;
        if stored_hash != request_hash {
            return Err(Status::aborted(
                "idempotency key was used for different input",
            ));
        }
        return match result.as_str() {
            "accepted" => Ok(Response::new(AgentSessionSendInputResponse {
                accepted_bytes: len,
            })),
            "reserved" => Err(Status::aborted(
                "matching input request is still in progress",
            )),
            _ => Err(Status::unavailable(
                "previous matching input request failed",
            )),
        };
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).custom_flags(libc::O_NONBLOCK);
    let write_result = options
        .open(&row.input_fifo_path)
        .and_then(|mut fifo| fifo.write_all(&req.input));
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let result = if write_result.is_ok() {
        "accepted"
    } else {
        "failed"
    }
    .to_owned();
    server.state.async_database.writer().call(move|conn|{conn.execute("UPDATE session_control_actions SET result=?3 WHERE session_id=?1 AND idempotency_key=?2",rusqlite::params![sid,key,result])?;Ok(())}).await.map_err(agent_orchestrator::async_database::flatten_err).map_err(|e|Status::internal(e.to_string()))?;
    if let Err(error) = write_result {
        return Err(Status::unavailable(format!(
            "session input transport failed: {error}"
        )));
    }
    emit(
        server,
        &row,
        "session_input_accepted",
        json!({"actor":actor,"client_id":req.client_id,"bytes":len,"fencing_token":token}),
    )
    .await;
    Ok(Response::new(AgentSessionSendInputResponse {
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
    let path = if std::path::Path::new(&row.transcript_path).exists() {
        row.transcript_path.clone()
    } else {
        row.stdout_path.clone()
    };
    let patterns = read_active_config(&server.state)
        .map(|a| a.config.runtime_policy().runner.redaction_patterns)
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
        let mut offset = req.offset;
        loop {
            let result = (|| -> std::io::Result<Option<Vec<u8>>> {
                let mut f = std::fs::File::open(&path)?;
                let len = f.metadata()?.len();
                if offset >= len {
                    return Ok(None);
                }
                f.seek(SeekFrom::Start(offset))?;
                let mut buf = vec![0; chunk.min((len - offset) as usize)];
                let n = f.read(&mut buf)?;
                buf.truncate(n);
                Ok(Some(buf))
            })();
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

pub(crate) async fn close(
    server: &OrchestratorServer,
    request: Request<AgentSessionCloseRequest>,
) -> Result<Response<AgentSessionCloseResponse>, Status> {
    authorize(server, &request, "AgentSessionClose").map_err(Status::from)?;
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    ensure_control_enabled(server)?;
    if req.reason.trim().is_empty() || req.idempotency_key.trim().is_empty() {
        return Err(Status::invalid_argument(
            "reason and idempotency_key are required",
        ));
    }
    let row = load(server, &req.session_id).await?;
    if let Some(v) = req.expected_state_version {
        if v != row.state_version {
            return Err(Status::aborted("session state version changed"));
        }
    }
    if !process_identity_matches(&row) {
        return Err(Status::failed_precondition(
            "session process identity cannot be verified",
        ));
    }
    let request_hash = hex::encode(Sha256::digest(
        format!("{}:{:?}", req.reason, req.expected_state_version).as_bytes(),
    ));
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let actor2 = actor.clone();
    let reason = req.reason.clone();
    let hash = request_hash.clone();
    let inserted = server.state.async_database.writer().call(move |conn| {
        let count = conn.execute(
            "INSERT OR IGNORE INTO session_control_actions(session_id,actor,action,idempotency_key,request_hash,result,reason,created_at) VALUES(?1,?2,'close',?3,?4,'reserved',?5,?6)",
            rusqlite::params![sid, actor2, key, hash, reason, agent_orchestrator::config_load::now_ts()],
        )?;
        Ok(count == 1)
    }).await.map_err(agent_orchestrator::async_database::flatten_err).map_err(|e| Status::internal(e.to_string()))?;
    if !inserted {
        let sid = req.session_id.clone();
        let key = req.idempotency_key.clone();
        let stored:String=server.state.async_database.reader().call(move|conn|Ok(conn.query_row("SELECT request_hash FROM session_control_actions WHERE session_id=?1 AND idempotency_key=?2",rusqlite::params![sid,key],|r|r.get(0))?)).await.map_err(agent_orchestrator::async_database::flatten_err).map_err(|e|Status::internal(e.to_string()))?;
        if stored != request_hash {
            return Err(Status::aborted(
                "idempotency key was used for a different close request",
            ));
        }
        return Ok(Response::new(AgentSessionCloseResponse {
            session: Some(to_proto(load(server, &row.id).await?)),
        }));
    }
    server
        .state
        .session_store
        .update_session_state(&row.id, "draining", None, false)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(row.pid as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .map_err(|e| Status::unavailable(format!("failed to close session process: {e}")))?;
    let sid = req.session_id.clone();
    let key = req.idempotency_key.clone();
    let _=server.state.async_database.writer().call(move|conn|{conn.execute("UPDATE session_control_actions SET result='accepted' WHERE session_id=?1 AND idempotency_key=?2",rusqlite::params![sid,key])?;Ok(())}).await;
    emit(
        server,
        &row,
        "session_close_requested",
        json!({"actor":actor,"reason":req.reason}),
    )
    .await;
    Ok(Response::new(AgentSessionCloseResponse {
        session: Some(to_proto(load(server, &row.id).await?)),
    }))
}

pub(crate) async fn resolve_pid(
    server: &OrchestratorServer,
    request: Request<AgentSessionResolvePidRequest>,
) -> Result<Response<AgentSessionResolvePidResponse>, Status> {
    authorize(server, &request, "AgentSessionResolvePid").map_err(Status::from)?;
    let pid = request.into_inner().pid;
    ensure_read_enabled(server)?;
    let rows = server
        .state
        .async_database
        .reader()
        .call(move |conn| {
            session_store::list_sessions_by_pid(conn, pid)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
        .map_err(|e| Status::internal(e.to_string()))?;
    Ok(Response::new(AgentSessionResolvePidResponse {
        sessions: rows.into_iter().map(to_proto).collect(),
    }))
}

fn process_identity_matches(row: &SessionRow) -> bool {
    if row.pid <= 0 {
        return false;
    }
    let Some(expected) = row.process_fingerprint.as_deref() else {
        return false;
    };
    session_store::capture_process_fingerprint(row.pid as u32).as_deref() == Some(expected)
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
