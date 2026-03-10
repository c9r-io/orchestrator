mod common;
mod manifest;
mod resource;
mod store;
mod task;

pub mod debug;
pub mod version;

use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::Commands;
use common::format_to_string;

pub async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: Commands,
) -> Result<()> {
    let command = match resource::dispatch(client, command).await? {
        Some(command) => command,
        None => return Ok(()),
    };

    match command {
        Commands::Task(cmd) => task::dispatch(client, cmd).await,
        Commands::Store(cmd) => store::dispatch(client, cmd).await,
        Commands::Debug {
            component,
            command: None,
        } => {
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

        Commands::Manifest(cmd) => manifest::dispatch(client, cmd).await,

        // Handled before dispatch
        Commands::Version => unreachable!(),
        Commands::Apply { .. }
        | Commands::Get { .. }
        | Commands::Describe { .. }
        | Commands::Delete { .. } => unreachable!("resource commands are handled before match"),
    }
}
