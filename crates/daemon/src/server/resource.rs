use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{OrchestratorServer, map_core_error};

pub(crate) async fn apply(
    server: &OrchestratorServer,
    request: Request<ApplyRequest>,
) -> Result<Response<ApplyResponse>, Status> {
    super::authorize(server, &request, "Apply").map_err(Status::from)?;

    // Elevate to Admin when the manifest contains CRDs with plugins or hooks.
    // This prevents Operator-role callers (including agent subprocesses via UDS)
    // from injecting arbitrary shell commands into the plugin execution pipeline.
    if manifests_contain_executable_commands(&request.get_ref().content) {
        super::authorize(server, &request, "ApplyPluginCrd").map_err(Status::from)?;
    }

    let req = request.into_inner();
    let result = agent_orchestrator::service::resource::apply_manifests(
        &server.state,
        &req.content,
        req.dry_run,
        req.project.as_deref(),
        req.prune,
    )
    .map_err(map_core_error)?;

    Ok(Response::new(result))
}

/// Check whether raw YAML content contains CRD manifests with non-empty
/// `plugins` or lifecycle `hooks` sections — i.e., executable commands.
fn manifests_contain_executable_commands(content: &str) -> bool {
    // Quick substring pre-filter to avoid full YAML parsing in the common case.
    let has_plugins = content.contains("plugins:");
    let has_hooks = content.contains("on_create:") || content.contains("on_update:") || content.contains("on_delete:");
    if !has_plugins && !has_hooks {
        return false;
    }

    // Parse YAML docs to confirm the presence is inside a CRD (kind: CustomResourceDefinition).
    for doc in content.split("\n---") {
        if doc.contains("kind: CustomResourceDefinition") && (
            (has_plugins && doc.contains("plugins:")) ||
            (has_hooks && (doc.contains("on_create:") || doc.contains("on_update:") || doc.contains("on_delete:")))
        ) {
            return true;
        }
    }
    false
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
    .map_err(map_core_error)?;

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
    .map_err(map_core_error)?;

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
    .map_err(map_core_error)?;
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
            .map_err(map_core_error)?;
    Ok(Response::new(ManifestExportResponse {
        content,
        format: req.output_format,
    }))
}
