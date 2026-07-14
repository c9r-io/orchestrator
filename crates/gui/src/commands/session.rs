use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

fn audit_context(
    reason_code: &str,
    operator_reason: Option<String>,
    idempotency_key: Option<String>,
) -> Option<orchestrator_proto::ActionAuditContext> {
    Some(orchestrator_proto::ActionAuditContext {
        reason_code: reason_code.to_string(),
        operator_reason,
        idempotency_key,
    })
}

fn generated_key(prefix: &str) -> String {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("gui-{prefix}-{nonce}")
}

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct AgentSession {
    pub session_id: String,
    pub task_id: String,
    pub task_item_id: Option<String>,
    pub step_id: String,
    pub agent_id: String,
    pub state: String,
    pub pid: i64,
    pub writer_client_id: Option<String>,
    pub writer_actor: Option<String>,
    pub writer_lease_expires_at: Option<String>,
    pub state_version: i64,
}

fn from_proto(value: orchestrator_proto::AgentSession) -> AgentSession {
    AgentSession {
        session_id: value.session_id,
        task_id: value.task_id,
        task_item_id: value.task_item_id,
        step_id: value.step_id,
        agent_id: value.agent_id,
        state: value.state,
        pid: value.pid,
        writer_client_id: value.writer_client_id,
        writer_actor: value.writer_actor,
        writer_lease_expires_at: value.writer_lease_expires_at,
        state_version: value.state_version,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionLease {
    pub fencing_token: i64,
    pub lease_expires_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionOutputChunk {
    pub offset: u64,
    pub next_offset: u64,
    pub text: String,
    pub eof: bool,
    pub redacted: bool,
}

#[tauri::command]
pub async fn agent_session_list(
    state: State<'_, Arc<AppState>>,
    task_id: Option<String>,
) -> Result<Vec<AgentSession>, String> {
    let mut client = state.client().await?;
    let response = client
        .agent_session_list(orchestrator_proto::AgentSessionListRequest {
            task_id,
            agent_id: None,
            state: None,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?
        .into_inner();
    Ok(response.sessions.into_iter().map(from_proto).collect())
}

#[tauri::command]
pub async fn agent_session_attach(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    client_id: String,
    mode: String,
) -> Result<SessionLease, String> {
    let mut client = state.client().await?;
    let r = client
        .agent_session_attach(orchestrator_proto::AgentSessionAttachRequest {
            audit: audit_context(
                "operator_session_attach",
                None,
                Some(generated_key("session-attach")),
            ),
            session_id,
            client_id,
            mode,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?
        .into_inner();
    Ok(SessionLease {
        fencing_token: r.fencing_token.unwrap_or_default(),
        lease_expires_at: r.lease_expires_at.unwrap_or_default(),
    })
}

#[tauri::command]
pub async fn agent_session_heartbeat(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    client_id: String,
    fencing_token: i64,
) -> Result<String, String> {
    let mut client = state.client().await?;
    Ok(client
        .agent_session_heartbeat(orchestrator_proto::AgentSessionHeartbeatRequest {
            audit: audit_context("lease_heartbeat", None, None),
            session_id,
            client_id,
            fencing_token,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?
        .into_inner()
        .lease_expires_at)
}

#[tauri::command]
pub async fn agent_session_send_input(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    client_id: String,
    fencing_token: i64,
    text: String,
    idempotency_key: String,
) -> Result<u64, String> {
    let mut client = state.client().await?;
    Ok(client
        .agent_session_send_input(orchestrator_proto::AgentSessionSendInputRequest {
            audit: audit_context(
                "operator_session_input",
                None,
                Some(idempotency_key.clone()),
            ),
            session_id,
            client_id,
            fencing_token,
            input: text.into_bytes(),
            idempotency_key,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?
        .into_inner()
        .accepted_bytes)
}

#[tauri::command]
pub async fn agent_session_detach(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    client_id: String,
    mode: String,
    fencing_token: Option<i64>,
) -> Result<bool, String> {
    let mut client = state.client().await?;
    Ok(client
        .agent_session_detach(orchestrator_proto::AgentSessionDetachRequest {
            audit: audit_context(
                "operator_session_detach",
                Some("GUI detach".into()),
                Some(generated_key("session-detach")),
            ),
            session_id,
            client_id,
            mode,
            fencing_token,
            reason: "GUI detach".into(),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?
        .into_inner()
        .detached)
}

#[tauri::command]
pub async fn agent_session_close(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    state_version: i64,
    reason: String,
    idempotency_key: String,
) -> Result<AgentSession, String> {
    let mut client = state.client().await?;
    let r = client
        .agent_session_close(orchestrator_proto::AgentSessionCloseRequest {
            audit: audit_context(
                "operator_session_close",
                Some(reason.clone()),
                Some(idempotency_key.clone()),
            ),
            session_id,
            reason,
            idempotency_key,
            expected_state_version: Some(state_version),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?
        .into_inner();
    r.session
        .map(from_proto)
        .ok_or_else(|| "daemon returned no session".into())
}

#[tauri::command]
pub async fn start_agent_session_read(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    offset: u64,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let response = client
        .agent_session_read(orchestrator_proto::AgentSessionReadRequest {
            session_id: session_id.clone(),
            offset,
            follow: true,
            max_chunk_bytes: 65536,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let mut stream = response.into_inner();
    let key = format!("agent-session-{session_id}");
    let cancel = state.register_stream(&key).await;
    let event = format!("agent-session-output-{session_id}");
    let error_event = format!("stream-error-agent-session-{session_id}");
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {message=stream.message()=>match message{Ok(Some(c))=>{let payload=SessionOutputChunk{offset:c.offset,next_offset:c.next_offset,text:c.text,eof:c.eof,redacted:c.redacted};let _=app.emit(&event,&payload);if c.eof{break}},Ok(None)=>break,Err(e)=>{let _=app.emit(&error_event,crate::errors::humanize_grpc_error(&e));break}},_=cancel.cancelled()=>break}
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_agent_session_read(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    state
        .cancel_stream(&format!("agent-session-{session_id}"))
        .await;
    Ok(())
}
