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
            print_agent_list(&resp.agents, output);
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
            print_sessions(&rows, output);
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
            print_sessions(&row, output);
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
                if !chunk.data.is_empty() {
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
                    idempotency_key: key,
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
                    reason,
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
                    reason,
                    idempotency_key: key,
                    expected_state_version: expected_version,
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
            print_sessions(&rows, output);
            Ok(())
        }
    }
}

fn session_json(s: &orchestrator_proto::AgentSession) -> serde_json::Value {
    serde_json::json!({"session_id":s.session_id,"task_id":s.task_id,"task_item_id":s.task_item_id,"step_id":s.step_id,"phase":s.phase,"agent_id":s.agent_id,"state":s.state,"pid":s.pid,"writer_client_id":s.writer_client_id,"writer_actor":s.writer_actor,"writer_lease_expires_at":s.writer_lease_expires_at,"writer_fencing_token":s.writer_fencing_token,"state_version":s.state_version,"created_at":s.created_at,"updated_at":s.updated_at,"ended_at":s.ended_at,"exit_code":s.exit_code})
}
fn now_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}
fn print_sessions(rows: &[orchestrator_proto::AgentSession], format: OutputFormat) {
    match format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&rows.iter().map(session_json).collect::<Vec<_>>())
                .unwrap_or_default()
        ),
        OutputFormat::Yaml => {
            for s in rows {
                println!(
                    "- session_id: {}\n  task_id: {}\n  agent_id: {}\n  state: {}\n  pid: {}",
                    s.session_id, s.task_id, s.agent_id, s.state, s.pid
                )
            }
        }
        OutputFormat::Table => {
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
        }
    }
}

fn print_agent_list(agents: &[orchestrator_proto::AgentStatus], format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            let json_agents: Vec<serde_json::Value> = agents
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "name": a.name,
                        "enabled": a.enabled,
                        "lifecycle_state": a.lifecycle_state,
                        "in_flight_items": a.in_flight_items,
                        "capabilities": a.capabilities,
                        "drain_requested_at": a.drain_requested_at,
                        "is_healthy": a.is_healthy,
                        "diseased_until": a.diseased_until,
                        "consecutive_errors": a.consecutive_errors,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json_agents).unwrap_or_default()
            );
        }
        OutputFormat::Yaml => {
            for a in agents {
                println!("- name: {}", a.name);
                println!("  enabled: {}", a.enabled);
                println!("  lifecycle_state: {}", a.lifecycle_state);
                println!("  in_flight_items: {}", a.in_flight_items);
                println!("  capabilities: {:?}", a.capabilities);
                if let Some(ref dt) = a.drain_requested_at {
                    println!("  drain_requested_at: {}", dt);
                }
                println!("  is_healthy: {}", a.is_healthy);
                if let Some(ref dt) = a.diseased_until {
                    println!("  diseased_until: {}", dt);
                }
                if a.consecutive_errors > 0 {
                    println!("  consecutive_errors: {}", a.consecutive_errors);
                }
            }
        }
        OutputFormat::Table => {
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
        }
    }
}
