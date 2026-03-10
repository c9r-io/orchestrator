use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use super::common::{format_grpc_error, format_to_string, read_input_or_file, resolve_resource};
use crate::{Commands, OutputFormat};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: Commands,
) -> Result<Option<Commands>> {
    match command {
        Commands::Apply {
            file,
            dry_run,
            prune,
            project,
        } => {
            let content = read_input_or_file(&file)?;
            let resp = client
                .apply(orchestrator_proto::ApplyRequest {
                    content,
                    dry_run,
                    project,
                    prune,
                })
                .await
                .map_err(format_grpc_error)?
                .into_inner();

            for entry in &resp.results {
                let scope = entry
                    .project_scope
                    .as_ref()
                    .map(|p| format!(" (project: {})", p))
                    .unwrap_or_default();
                if dry_run {
                    println!(
                        "{}/{} would be {} (dry run){}",
                        entry.kind, entry.name, entry.action, scope
                    );
                } else {
                    println!("{}/{} {}{}", entry.kind, entry.name, entry.action, scope);
                }
            }
            if let Some(version) = resp.config_version {
                println!("configuration version: {}", version);
            }
            for err in &resp.errors {
                eprintln!("Error: {}", err);
            }
            if !resp.errors.is_empty() {
                std::process::exit(1);
            }
            Ok(None)
        }
        Commands::Get {
            resource,
            name,
            output,
            selector,
            project,
        } => {
            let resource = resolve_resource(&resource, name.as_deref());
            let resp = client
                .get(orchestrator_proto::GetRequest {
                    resource,
                    selector,
                    output_format: format_to_string(output),
                    project,
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(None)
        }
        Commands::Describe {
            resource,
            name,
            output,
            project,
        } => {
            let resource = resolve_resource(&resource, name.as_deref());
            let resp = client
                .describe(orchestrator_proto::DescribeRequest {
                    resource,
                    output_format: format_to_string(output),
                    project,
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(None)
        }
        Commands::Delete {
            resource,
            name,
            force,
            dry_run,
            project,
        } => {
            let resource = resolve_resource(&resource, name.as_deref());
            let resp = client
                .delete(orchestrator_proto::DeleteRequest {
                    resource,
                    force,
                    dry_run,
                    project,
                })
                .await
                .map_err(format_grpc_error)?
                .into_inner();
            println!("{}", resp.message);
            Ok(None)
        }
        other => Ok(Some(other)),
    }
}

#[allow(dead_code)]
fn _assert_output_format_send(_format: OutputFormat) {}
