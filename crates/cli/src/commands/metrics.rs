use anyhow::{Result, bail};
use orchestrator_proto::{
    OrchestratorServiceClient, ProcessMetricsGetRequest, ProcessMetricsPruneRequest,
    ProcessMetricsRebuildRequest,
};
use tonic::transport::Channel;

use crate::{MetricsCommands, OutputFormat};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: MetricsCommands,
) -> Result<()> {
    match command {
        MetricsCommands::Process {
            project,
            window,
            bucket,
            output,
        } => {
            let response = client
                .process_metrics_get(ProcessMetricsGetRequest {
                    project_id: project,
                    window,
                    bucket,
                })
                .await?
                .into_inner();
            let value: serde_json::Value = serde_json::from_str(&response.metrics_json)?;
            match output {
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
                OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&value)?),
                OutputFormat::Table => print_table(&value)?,
            }
            Ok(())
        }
        MetricsCommands::Rebuild { project } => {
            let response = client
                .process_metrics_rebuild(ProcessMetricsRebuildRequest {
                    project_id: project,
                })
                .await?
                .into_inner();
            println!("{} ({} rows)", response.message, response.affected_rows);
            Ok(())
        }
        MetricsCommands::Prune { retention_days } => {
            let response = client
                .process_metrics_prune(ProcessMetricsPruneRequest { retention_days })
                .await?
                .into_inner();
            println!("{} ({} rows)", response.message, response.affected_rows);
            Ok(())
        }
    }
}

fn print_table(value: &serde_json::Value) -> Result<()> {
    let project = value["project_id"].as_str().unwrap_or("-");
    let start = value["window_start"].as_str().unwrap_or("-");
    let end = value["window_end"].as_str().unwrap_or("-");
    println!("PROJECT  {project}");
    println!("WINDOW   {start} — {end}");
    println!("{:<44} {:>12} {:>12}", "METRIC", "VALUE", "SAMPLES");
    if let Some(metrics) = value["metrics"].as_array() {
        for metric in metrics {
            let name = metric["name"].as_str().unwrap_or("unknown");
            let labels = metric["labels"]
                .as_object()
                .map(|labels| {
                    labels
                        .iter()
                        .map(|(key, value)| format!("{key}={}", value.as_str().unwrap_or("?")))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            let display = if labels.is_empty() {
                name.to_string()
            } else {
                format!("{name}{{{labels}}}")
            };
            println!(
                "{:<44} {:>12.4} {:>12}",
                display,
                metric["value"].as_f64().unwrap_or(0.0),
                metric["sample_count"].as_u64().unwrap_or(0)
            );
        }
    } else {
        bail!("metrics response does not contain a metrics array");
    }
    Ok(())
}
