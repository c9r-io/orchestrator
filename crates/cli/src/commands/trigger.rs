use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::cli::TriggerCommands;

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: TriggerCommands,
) -> Result<()> {
    match cmd {
        TriggerCommands::Suspend { name, project } => {
            let resp = client
                .trigger_suspend(orchestrator_proto::TriggerSuspendRequest {
                    trigger_name: name,
                    project,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
        TriggerCommands::Resume { name, project } => {
            let resp = client
                .trigger_resume(orchestrator_proto::TriggerResumeRequest {
                    trigger_name: name,
                    project,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
        TriggerCommands::Fire { name, project } => {
            let resp = client
                .trigger_fire(orchestrator_proto::TriggerFireRequest {
                    trigger_name: name,
                    project,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
    }
}
