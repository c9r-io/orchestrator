use std::collections::BTreeMap;

use agent_orchestrator::config_ext::OrchestratorConfigExt;
use agent_orchestrator::process_metrics::{
    AsyncProcessMetricsRepository, MetricObservation, ProcessMetricsQuery, validate_window_bucket,
};
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::OrchestratorServer;

pub(crate) fn record_source_dedup(
    state: &std::sync::Arc<agent_orchestrator::state::InnerState>,
    project_id: &str,
    provider: &str,
) {
    let Ok(loaded) = agent_orchestrator::config_load::read_loaded_config(state) else {
        return;
    };
    if !loaded
        .config
        .runtime_policy_for_project(project_id)
        .observability
        .process_metrics
        .enabled
    {
        return;
    }
    let repository = AsyncProcessMetricsRepository::new(state.async_database.clone());
    let project_id = project_id.to_string();
    let provider = provider.to_string();
    tokio::spawn(async move {
        let mut dimensions = BTreeMap::new();
        dimensions.insert("provider".to_string(), provider);
        if let Err(error) = repository
            .record(MetricObservation {
                project_id,
                metric_name: "source_event_deduplicated_total".to_string(),
                dimensions,
                value: 1.0,
                occurred_at: agent_orchestrator::config_load::now_ts(),
                source_kind: "source_delivery".to_string(),
                source_key: uuid::Uuid::new_v4().to_string(),
            })
            .await
        {
            tracing::warn!(error = %error, "failed to record source deduplication metric");
        }
    });
}

pub(crate) async fn get(
    server: &OrchestratorServer,
    request: Request<ProcessMetricsGetRequest>,
) -> Result<Response<ProcessMetricsGetResponse>, Status> {
    super::authorize(server, &request, "ProcessMetricsGet").map_err(Status::from)?;
    let req = request.into_inner();
    let loaded = agent_orchestrator::config_load::read_loaded_config(&server.state)
        .map_err(|error| Status::internal(error.to_string()))?;
    let policy = loaded.config.runtime_policy_for_project(&req.project_id);
    let config = policy.observability.process_metrics;
    let (window_seconds, bucket_seconds) =
        validate_window_bucket(&req.window, &req.bucket, config.max_window_days)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
    let metrics = AsyncProcessMetricsRepository::new(server.state.async_database.clone())
        .query(ProcessMetricsQuery {
            project_id: req.project_id,
            window_seconds,
            bucket_seconds,
            collection_enabled: config.enabled,
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let metrics_json =
        serde_json::to_string(&metrics).map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(ProcessMetricsGetResponse {
        schema_version: metrics.schema_version,
        metrics_json,
    }))
}

pub(crate) async fn record(
    server: &OrchestratorServer,
    request: Request<ProcessMetricRecordRequest>,
) -> Result<Response<ProcessMetricRecordResponse>, Status> {
    super::authorize(server, &request, "ProcessMetricRecord").map_err(Status::from)?;
    let req = request.into_inner();
    let loaded = agent_orchestrator::config_load::read_loaded_config(&server.state)
        .map_err(|error| Status::internal(error.to_string()))?;
    let config = loaded
        .config
        .runtime_policy_for_project(&req.project_id)
        .observability
        .process_metrics;
    if !config.enabled || !config.ui_telemetry_enabled {
        return Err(Status::failed_precondition(
            "Process Console UI telemetry is disabled",
        ));
    }
    let recorded_at = agent_orchestrator::config_load::now_ts();
    let inserted = AsyncProcessMetricsRepository::new(server.state.async_database.clone())
        .record(MetricObservation {
            project_id: req.project_id,
            metric_name: req.metric_name,
            dimensions: req.dimensions.into_iter().collect::<BTreeMap<_, _>>(),
            value: req.value,
            occurred_at: recorded_at.clone(),
            source_kind: "ui".to_string(),
            source_key: req.source_key,
        })
        .await
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(Response::new(ProcessMetricRecordResponse {
        inserted,
        recorded_at,
    }))
}

pub(crate) async fn rebuild(
    server: &OrchestratorServer,
    request: Request<ProcessMetricsRebuildRequest>,
) -> Result<Response<ProcessMetricsMaintenanceResponse>, Status> {
    super::authorize(server, &request, "ProcessMetricsRebuild").map_err(Status::from)?;
    let project_id = request.into_inner().project_id;
    let affected_rows = AsyncProcessMetricsRepository::new(server.state.async_database.clone())
        .rebuild(&project_id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(ProcessMetricsMaintenanceResponse {
        affected_rows,
        message: format!("rebuilt Process Console metric rollups for {project_id}"),
    }))
}

pub(crate) async fn prune(
    server: &OrchestratorServer,
    request: Request<ProcessMetricsPruneRequest>,
) -> Result<Response<ProcessMetricsMaintenanceResponse>, Status> {
    super::authorize(server, &request, "ProcessMetricsPrune").map_err(Status::from)?;
    let requested = request.into_inner().retention_days;
    let loaded = agent_orchestrator::config_load::read_loaded_config(&server.state)
        .map_err(|error| Status::internal(error.to_string()))?;
    let configured = loaded
        .config
        .global_runtime_policy()
        .observability
        .process_metrics
        .retention_days;
    let retention_days = if requested == 0 {
        configured
    } else {
        requested
    };
    if !(1..=365).contains(&retention_days) {
        return Err(Status::invalid_argument(
            "retention_days must be between 1 and 365",
        ));
    }
    let affected_rows = AsyncProcessMetricsRepository::new(server.state.async_database.clone())
        .prune(retention_days)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(ProcessMetricsMaintenanceResponse {
        affected_rows,
        message: format!("pruned Process Console metrics older than {retention_days} days"),
    }))
}
