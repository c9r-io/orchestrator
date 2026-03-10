use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use super::common::{format_to_string, read_input_or_file};
use crate::ManifestCommands;

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: ManifestCommands,
) -> Result<()> {
    match cmd {
        ManifestCommands::Validate { file, project } => {
            let content = read_input_or_file(&file)?;
            let resp = client
                .manifest_validate(orchestrator_proto::ManifestValidateRequest {
                    content,
                    project_id: project,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            for err in &resp.errors {
                eprintln!("  {}", err);
            }
            if !resp.valid {
                std::process::exit(1);
            }
            Ok(())
        }
        ManifestCommands::Export { output } => {
            let resp = client
                .manifest_export(orchestrator_proto::ManifestExportRequest {
                    output_format: format_to_string(output),
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(())
        }
    }
}
