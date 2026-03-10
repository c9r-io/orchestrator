use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{map_resource_error, OrchestratorServer};

pub(crate) async fn apply(
    server: &OrchestratorServer,
    request: Request<ApplyRequest>,
) -> Result<Response<ApplyResponse>, Status> {
    super::authorize(server, &request, "Apply").map_err(Status::from)?;
    let req = request.into_inner();
    let result = agent_orchestrator::service::resource::apply_manifests(
        &server.state,
        &req.content,
        req.dry_run,
        req.project.as_deref(),
        req.prune,
    )
    .map_err(map_resource_error)?;

    Ok(Response::new(result))
}

pub(crate) async fn get(
    server: &OrchestratorServer,
    request: Request<GetRequest>,
) -> Result<Response<GetResponse>, Status> {
    super::authorize(server, &request, "Get").map_err(Status::from)?;
    let req = request.into_inner();
    let content = agent_orchestrator::service::resource::get_resource(
        &server.state,
        &req.resource,
        req.selector.as_deref(),
        &req.output_format,
        req.project.as_deref(),
    )
    .map_err(|e| Status::internal(format!("{e}")))?;

    Ok(Response::new(GetResponse {
        content,
        format: req.output_format,
    }))
}

pub(crate) async fn describe(
    server: &OrchestratorServer,
    request: Request<DescribeRequest>,
) -> Result<Response<DescribeResponse>, Status> {
    super::authorize(server, &request, "Describe").map_err(Status::from)?;
    let req = request.into_inner();
    let content = agent_orchestrator::service::resource::describe_resource(
        &server.state,
        &req.resource,
        &req.output_format,
        req.project.as_deref(),
    )
    .map_err(|e| Status::internal(format!("{e}")))?;

    Ok(Response::new(DescribeResponse {
        content,
        format: req.output_format,
    }))
}

pub(crate) async fn delete(
    server: &OrchestratorServer,
    request: Request<DeleteRequest>,
) -> Result<Response<DeleteResponse>, Status> {
    super::authorize(server, &request, "Delete").map_err(Status::from)?;
    let req = request.into_inner();
    agent_orchestrator::service::resource::delete_resource(
        &server.state,
        &req.resource,
        req.force,
        req.project.as_deref(),
        req.dry_run,
    )
    .map_err(map_resource_error)?;
    let scope = req
        .project
        .map(|p| format!(" (project: {})", p))
        .unwrap_or_default();
    let verb = if req.dry_run {
        "would be deleted (dry run)"
    } else {
        "deleted"
    };
    Ok(Response::new(DeleteResponse {
        message: format!("{} {}{}", req.resource, verb, scope),
    }))
}

pub(crate) async fn manifest_export(
    server: &OrchestratorServer,
    request: Request<ManifestExportRequest>,
) -> Result<Response<ManifestExportResponse>, Status> {
    super::authorize(server, &request, "ManifestExport").map_err(Status::from)?;
    let req = request.into_inner();
    let content =
        agent_orchestrator::service::resource::export_manifests(&server.state, &req.output_format)
            .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(ManifestExportResponse {
        content,
        format: req.output_format,
    }))
}
