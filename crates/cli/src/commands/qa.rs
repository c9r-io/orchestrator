use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use serde_json::json;
use tonic::transport::Channel;

use crate::{OutputFormat, QaCommands};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: QaCommands,
) -> Result<()> {
    match cmd {
        QaCommands::Doctor { output } => {
            let resp = client
                .qa_doctor(orchestrator_proto::QaDoctorRequest {})
                .await?
                .into_inner();
            print_doctor(&resp, output)
        }
    }
}

fn print_doctor(resp: &orchestrator_proto::QaDoctorResponse, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "observability": {
                        "task_execution_metrics_total": resp.task_execution_metrics_total,
                        "task_execution_metrics_last_24h": resp.task_execution_metrics_last_24h,
                        "task_completion_rate": resp.task_completion_rate,
                    }
                }))?
            );
            Ok(())
        }
        OutputFormat::Table => {
            println!("{:<40} VALUE", "METRIC");
            println!(
                "{:<40} {}",
                "task_execution_metrics_total", resp.task_execution_metrics_total
            );
            println!(
                "{:<40} {}",
                "task_execution_metrics_last_24h", resp.task_execution_metrics_last_24h
            );
            println!(
                "{:<40} {:.2}",
                "task_completion_rate", resp.task_completion_rate
            );
            Ok(())
        }
        OutputFormat::Yaml => anyhow::bail!("qa commands support only table or json output"),
    }
}
