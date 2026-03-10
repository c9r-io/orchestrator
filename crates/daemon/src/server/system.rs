use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::OrchestratorServer;

pub(crate) async fn ping(
    server: &OrchestratorServer,
    request: Request<PingRequest>,
) -> Result<Response<PingResponse>, Status> {
    super::authorize(server, &request, "Ping").map_err(Status::from)?;
    Ok(Response::new(PingResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_hash: env!("BUILD_GIT_HASH").to_string(),
        uptime_secs: server.startup_instant.elapsed().as_secs().to_string(),
    }))
}

pub(crate) async fn shutdown(
    server: &OrchestratorServer,
    request: Request<ShutdownRequest>,
) -> Result<Response<ShutdownResponse>, Status> {
    super::authorize(server, &request, "Shutdown").map_err(Status::from)?;
    let req = request.into_inner();
    tracing::info!(graceful = req.graceful, "shutdown requested via RPC");
    server.shutdown_notify.notify_one();
    Ok(Response::new(ShutdownResponse {
        message: "shutdown initiated".to_string(),
    }))
}

pub(crate) async fn config_debug(
    server: &OrchestratorServer,
    request: Request<ConfigDebugRequest>,
) -> Result<Response<ConfigDebugResponse>, Status> {
    super::authorize(server, &request, "ConfigDebug").map_err(Status::from)?;
    let req = request.into_inner();
    let content =
        agent_orchestrator::service::system::debug_info(&server.state, req.component.as_deref())
            .map_err(|e| Status::internal(format!("{e}")))?;

    Ok(Response::new(ConfigDebugResponse {
        content,
        format: "text".to_string(),
    }))
}

pub(crate) async fn worker_status(
    server: &OrchestratorServer,
    request: Request<WorkerStatusRequest>,
) -> Result<Response<WorkerStatusResponse>, Status> {
    super::authorize(server, &request, "WorkerStatus").map_err(Status::from)?;
    let status = agent_orchestrator::service::system::worker_status(&server.state)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

    Ok(Response::new(status))
}

pub(crate) async fn check(
    server: &OrchestratorServer,
    request: Request<CheckRequest>,
) -> Result<Response<CheckResponse>, Status> {
    super::authorize(server, &request, "Check").map_err(Status::from)?;
    let req = request.into_inner();
    let report = agent_orchestrator::service::system::run_check(
        &server.state,
        req.workflow.as_deref(),
        &req.output_format,
        req.project_id.as_deref(),
    )
    .map_err(|e| Status::internal(format!("{e}")))?;

    Ok(Response::new(CheckResponse {
        content: report.content,
        format: req.output_format,
        exit_code: report.exit_code,
        diagnostics: report
            .report
            .checks
            .iter()
            .map(agent_orchestrator::service::system::diagnostic_entry_from_check)
            .collect(),
    }))
}

pub(crate) async fn init(
    server: &OrchestratorServer,
    request: Request<InitRequest>,
) -> Result<Response<InitResponse>, Status> {
    super::authorize(server, &request, "Init").map_err(Status::from)?;
    let req = request.into_inner();
    let message = agent_orchestrator::service::system::run_init(&server.state, req.root.as_deref())
        .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(InitResponse { message }))
}

pub(crate) async fn manifest_validate(
    server: &OrchestratorServer,
    request: Request<ManifestValidateRequest>,
) -> Result<Response<ManifestValidateResponse>, Status> {
    super::authorize(server, &request, "ManifestValidate").map_err(Status::from)?;
    let req = request.into_inner();
    let report = agent_orchestrator::service::system::validate_manifests(
        &server.state,
        &req.content,
        req.project_id.as_deref(),
    )
    .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(ManifestValidateResponse {
        valid: report.valid,
        errors: report.errors,
        message: report.message,
        diagnostics: report.diagnostics,
    }))
}
