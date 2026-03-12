use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::cli::AgentCommands;
use crate::OutputFormat;

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: AgentCommands,
) -> Result<()> {
    match cmd {
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
            }
        }
        OutputFormat::Table => {
            println!(
                "{:<20} {:<8} {:<10} {:<10} {}",
                "NAME", "ENABLED", "STATE", "IN-FLIGHT", "CAPABILITIES"
            );
            for a in agents {
                println!(
                    "{:<20} {:<8} {:<10} {:<10} {}",
                    a.name,
                    a.enabled,
                    a.lifecycle_state,
                    a.in_flight_items,
                    a.capabilities.join(", ")
                );
            }
        }
    }
}
