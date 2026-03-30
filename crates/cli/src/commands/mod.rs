mod agent;
mod common;
/// Local daemon lifecycle commands (stop / status).
pub mod daemon;
mod db;
mod event;
mod manifest;
mod qa;
mod resource;
mod secret;
mod store;
mod task;
mod trigger;

/// Local debug command implementations.
pub mod debug;
/// Version-reporting commands that do not require daemon access.
pub mod version;

use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::Commands;
use common::format_to_string;

/// Dispatch a parsed top-level command to the appropriate handler.
pub async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: Commands,
) -> Result<()> {
    let command = match resource::dispatch(client, command).await? {
        Some(command) => command,
        None => return Ok(()),
    };

    match command {
        Commands::Agent(cmd) => agent::dispatch(client, cmd).await,
        Commands::Task(cmd) => task::dispatch(client, cmd).await,
        Commands::Store(cmd) => store::dispatch(client, cmd).await,
        Commands::Secret(cmd) => secret::dispatch(client, cmd).await,
        Commands::Db(cmd) => db::dispatch(client, cmd).await,
        Commands::Event(cmd) => event::dispatch(client, cmd).await,
        Commands::Trigger(cmd) => trigger::dispatch(client, cmd).await,
        Commands::Debug {
            component,
            command: None,
        } => {
            if component.as_deref() == Some("daemon") {
                let ping = client
                    .ping(orchestrator_proto::PingRequest {})
                    .await?
                    .into_inner();
                let status = client
                    .worker_status(orchestrator_proto::WorkerStatusRequest {})
                    .await?
                    .into_inner();
                debug::print_daemon_status(ping, status);
                return Ok(());
            }
            let resp = client
                .config_debug(orchestrator_proto::ConfigDebugRequest { component })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(())
        }

        Commands::Debug {
            component: _,
            command: Some(_),
        } => unreachable!("local debug subcommands are handled before daemon dispatch"),

        Commands::Check {
            workflow,
            output,
            project,
        } => {
            let resp = client
                .check(orchestrator_proto::CheckRequest {
                    workflow,
                    output_format: format_to_string(output),
                    project_id: project,
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            std::process::exit(resp.exit_code);
        }

        Commands::Init { root } => {
            let resp = client
                .init(orchestrator_proto::InitRequest { root })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        Commands::Qa(cmd) => qa::dispatch(client, cmd).await,
        Commands::Manifest(cmd) => manifest::dispatch(client, cmd).await,

        // Handled before dispatch
        Commands::Version { .. } | Commands::Daemon(_) => unreachable!(),
        Commands::Apply { .. }
        | Commands::Get { .. }
        | Commands::Describe { .. }
        | Commands::Delete { .. } => unreachable!("resource commands are handled before match"),
    }
}
