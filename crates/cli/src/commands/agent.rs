use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::OutputFormat;
use crate::cli::{AgentCommands, AgentSessionCommands};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: AgentCommands,
) -> Result<()> {
    match cmd {
        AgentCommands::Session(cmd) => dispatch_session(client, cmd).await,
        AgentCommands::List { project, output } => {
            let resp = client
                .agent_list(orchestrator_proto::AgentListRequest {
                    project_id: project,
                })
                .await?
                .into_inner();
            print_agent_list(&resp.agents, output)?;
            Ok(())
        }
        AgentCommands::Cordon {
            agent_name,
            project,
        } => {
            let resp = client
                .agent_cordon(orchestrator_proto::AgentCordonRequest {
                    agent_name,
                    project_id: project,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
        AgentCommands::Uncordon {
            agent_name,
            project,
        } => {
            let resp = client
                .agent_uncordon(orchestrator_proto::AgentUncordonRequest {
                    agent_name,
                    project_id: project,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
        AgentCommands::Drain {
            agent_name,
            project,
            timeout,
        } => {
            let resp = client
                .agent_drain(orchestrator_proto::AgentDrainRequest {
                    agent_name,
                    project_id: project,
                    timeout_secs: timeout,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
    }
}

async fn dispatch_session(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: AgentSessionCommands,
) -> Result<()> {
    use orchestrator_proto::*;
    match cmd {
        AgentSessionCommands::List {
            task,
            agent,
            state,
            output,
        } => {
            let rows = client
                .agent_session_list(AgentSessionListRequest {
                    task_id: task,
                    agent_id: agent,
                    state,
                })
                .await?
                .into_inner()
                .sessions;
            print_sessions(&rows, output)?;
            Ok(())
        }
        AgentSessionCommands::Get { session_id, output } => {
            let row = client
                .agent_session_get(AgentSessionGetRequest { session_id })
                .await?
                .into_inner()
                .session
                .into_iter()
                .collect::<Vec<_>>();
            print_sessions(&row, output)?;
            Ok(())
        }
        AgentSessionCommands::Attach {
            session_id,
            mode,
            client_id,
        } => {
            let r = client
                .agent_session_attach(AgentSessionAttachRequest {
                    session_id,
                    client_id,
                    mode,
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_session_attach".into(),
                        operator_reason: None,
                        idempotency_key: Some(format!("cli-attach-{}", now_nonce())),
                    }),
                })
                .await?
                .into_inner();
            println!(
                "session={} mode={} writer_granted={} fencing_token={} lease_expires_at={}",
                r.session_id,
                r.mode,
                r.writer_granted,
                r.fencing_token
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into()),
                r.lease_expires_at.as_deref().unwrap_or("-")
            );
            Ok(())
        }
        AgentSessionCommands::Read {
            session_id,
            follow,
            offset,
            chunks_json,
        } => {
            use std::io::Write as _;
            let mut stream = client
                .agent_session_read(AgentSessionReadRequest {
                    session_id,
                    offset,
                    follow,
                    max_chunk_bytes: 65536,
                })
                .await?
                .into_inner();
            let mut out = std::io::stdout().lock();
            while let Some(chunk) = stream.message().await? {
                if chunks_json {
                    serde_json::to_writer(
                        &mut out,
                        &serde_json::json!({
                            "session_id": chunk.session_id,
                            "offset": chunk.offset,
                            "next_offset": chunk.next_offset,
                            "stream": chunk.stream,
                            "text": chunk.text,
                            "eof": chunk.eof,
                            "redacted": chunk.redacted,
                        }),
                    )?;
                    writeln!(out)?;
                    out.flush()?;
                } else if !chunk.data.is_empty() {
                    out.write_all(&chunk.data)?;
                    out.flush()?;
                }
                if chunk.eof {
                    break;
                }
            }
            Ok(())
        }
        AgentSessionCommands::Heartbeat {
            session_id,
            client_id,
            fencing_token,
        } => {
            let r = client
                .agent_session_heartbeat(AgentSessionHeartbeatRequest {
                    session_id,
                    client_id,
                    fencing_token,
                    audit: Some(ActionAuditContext {
                        reason_code: "lease_heartbeat".into(),
                        operator_reason: None,
                        idempotency_key: None,
                    }),
                })
                .await?
                .into_inner();
            println!("{}", r.lease_expires_at);
            Ok(())
        }
        AgentSessionCommands::SendInput {
            session_id,
            text,
            client_id,
            fencing_token,
            idempotency_key,
        } => {
            let key = idempotency_key.unwrap_or_else(|| format!("cli-{}", now_nonce()));
            let r = client
                .agent_session_send_input(AgentSessionSendInputRequest {
                    session_id,
                    client_id,
                    fencing_token,
                    input: text.into_bytes(),
                    idempotency_key: key.clone(),
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_session_input".into(),
                        operator_reason: None,
                        idempotency_key: Some(key),
                    }),
                })
                .await?
                .into_inner();
            println!("accepted_bytes={}", r.accepted_bytes);
            Ok(())
        }
        AgentSessionCommands::Detach {
            session_id,
            mode,
            client_id,
            fencing_token,
            reason,
        } => {
            client
                .agent_session_detach(AgentSessionDetachRequest {
                    session_id,
                    client_id,
                    mode,
                    fencing_token,
                    reason: reason.clone(),
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_session_detach".into(),
                        operator_reason: Some(reason),
                        idempotency_key: Some(format!("cli-detach-{}", now_nonce())),
                    }),
                })
                .await?;
            println!("detached");
            Ok(())
        }
        AgentSessionCommands::Close {
            session_id,
            reason,
            expected_version,
            idempotency_key,
        } => {
            let key = idempotency_key.unwrap_or_else(|| format!("cli-close-{}", now_nonce()));
            let r = client
                .agent_session_close(AgentSessionCloseRequest {
                    session_id,
                    reason: reason.clone(),
                    idempotency_key: key.clone(),
                    expected_state_version: expected_version,
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_session_close".into(),
                        operator_reason: Some(reason),
                        idempotency_key: Some(key),
                    }),
                })
                .await?
                .into_inner();
            println!("state={}", r.session.map(|s| s.state).unwrap_or_default());
            Ok(())
        }
        AgentSessionCommands::Resolve { pid, output } => {
            let rows = client
                .agent_session_resolve_pid(AgentSessionResolvePidRequest { pid })
                .await?
                .into_inner()
                .sessions;
            print_sessions(&rows, output)?;
            Ok(())
        }
    }
}

pub(crate) fn session_value(s: &orchestrator_proto::AgentSession) -> serde_json::Value {
    serde_json::json!({"session_id":s.session_id,"task_id":s.task_id,"task_item_id":s.task_item_id,"step_id":s.step_id,"phase":s.phase,"agent_id":s.agent_id,"state":s.state,"pid":s.pid,"writer_client_id":s.writer_client_id,"writer_actor":s.writer_actor,"writer_lease_expires_at":s.writer_lease_expires_at,"writer_fencing_token":s.writer_fencing_token,"state_version":s.state_version,"created_at":s.created_at,"updated_at":s.updated_at,"ended_at":s.ended_at,"exit_code":s.exit_code})
}
fn now_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}
fn print_sessions(rows: &[orchestrator_proto::AgentSession], format: OutputFormat) -> Result<()> {
    let projected = serde_json::Value::Array(rows.iter().map(session_value).collect());
    match format.encoding() {
        Some(encoding) => crate::output::render::emit(&projected, encoding),
        None => {
            println!(
                "{:<38} {:<18} {:<16} {:<10} PID",
                "SESSION", "TASK", "AGENT", "STATE"
            );
            for s in rows {
                println!(
                    "{:<38} {:<18} {:<16} {:<10} {}",
                    s.session_id, s.task_id, s.agent_id, s.state, s.pid
                )
            }
            Ok(())
        }
    }
}

fn print_agent_list(
    agents: &[orchestrator_proto::AgentStatus],
    format: OutputFormat,
) -> Result<()> {
    let projected = serde_json::Value::Array(
        agents
            .iter()
            .map(crate::output::value::agent_status_value)
            .collect(),
    );
    match format.encoding() {
        Some(encoding) => crate::output::render::emit(&projected, encoding),
        None => {
            println!(
                "{:<20} {:<8} {:<10} {:<10} {:<10} CAPABILITIES",
                "NAME", "ENABLED", "STATE", "IN-FLIGHT", "HEALTH"
            );
            for a in agents {
                let health = if a.is_healthy {
                    "healthy".to_string()
                } else {
                    match &a.diseased_until {
                        Some(dt) => format!("diseased({})", &dt[11..16]),
                        None => "diseased".to_string(),
                    }
                };
                println!(
                    "{:<20} {:<8} {:<10} {:<10} {:<10} {}",
                    a.name,
                    a.enabled,
                    a.lifecycle_state,
                    a.in_flight_items,
                    health,
                    a.capabilities.join(", ")
                );
            }
            Ok(())
        }
    }
}
