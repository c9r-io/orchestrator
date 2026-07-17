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
                    audit: Some(trigger_audit("operator_trigger_suspend", "suspend")),
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
                    audit: Some(trigger_audit("operator_trigger_resume", "resume")),
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
        TriggerCommands::Fire {
            name,
            project,
            payload,
        } => {
            let resp = client
                .trigger_fire(orchestrator_proto::TriggerFireRequest {
                    trigger_name: name,
                    project,
                    payload_json: payload,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
    }
}

fn trigger_audit(reason_code: &str, action: &str) -> orchestrator_proto::ActionAuditContext {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    orchestrator_proto::ActionAuditContext {
        reason_code: reason_code.to_string(),
        operator_reason: None,
        idempotency_key: Some(format!("cli-trigger-{action}-{nonce}")),
    }
}
