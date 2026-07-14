use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn process_metrics_get(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    window: String,
    bucket: String,
) -> Result<Value, String> {
    let mut client = state.client().await?;
    let response = client
        .process_metrics_get(orchestrator_proto::ProcessMetricsGetRequest {
            project_id,
            window,
            bucket,
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?
        .into_inner();
    serde_json::from_str(&response.metrics_json).map_err(|error| error.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn process_metric_record(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    metric_name: String,
    dimensions: HashMap<String, String>,
    value: f64,
    source_key: String,
) -> Result<bool, String> {
    let mut client = state.client().await?;
    client
        .process_metric_record(orchestrator_proto::ProcessMetricRecordRequest {
            project_id,
            metric_name,
            dimensions,
            value,
            source_key,
        })
        .await
        .map(|response| response.into_inner().inserted)
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}
