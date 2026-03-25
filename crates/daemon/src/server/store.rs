use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{OrchestratorServer, map_core_error};

pub(crate) async fn store_get(
    server: &OrchestratorServer,
    request: Request<StoreGetRequest>,
) -> Result<Response<StoreGetResponse>, Status> {
    super::authorize(server, &request, "StoreGet").map_err(Status::from)?;
    let req = request.into_inner();
    let result = agent_orchestrator::service::store::store_get(
        &server.state,
        &req.store,
        &req.key,
        &req.project,
    )
    .await
    .map_err(map_core_error)?;

    Ok(Response::new(StoreGetResponse {
        value_json: result.clone(),
        found: result.is_some(),
    }))
}

pub(crate) async fn store_put(
    server: &OrchestratorServer,
    request: Request<StorePutRequest>,
) -> Result<Response<StorePutResponse>, Status> {
    super::authorize(server, &request, "StorePut").map_err(Status::from)?;
    let req = request.into_inner();
    agent_orchestrator::service::store::store_put(
        &server.state,
        &req.store,
        &req.key,
        &req.value_json,
        &req.project,
        &req.task_id,
    )
    .await
    .map_err(map_core_error)?;

    Ok(Response::new(StorePutResponse {
        message: format!("stored key '{}' in '{}'", req.key, req.store),
    }))
}

pub(crate) async fn store_delete(
    server: &OrchestratorServer,
    request: Request<StoreDeleteRequest>,
) -> Result<Response<StoreDeleteResponse>, Status> {
    super::authorize(server, &request, "StoreDelete").map_err(Status::from)?;
    let req = request.into_inner();
    agent_orchestrator::service::store::store_delete(
        &server.state,
        &req.store,
        &req.key,
        &req.project,
    )
    .await
    .map_err(map_core_error)?;

    Ok(Response::new(StoreDeleteResponse {
        message: format!("deleted key '{}' from '{}'", req.key, req.store),
    }))
}

pub(crate) async fn store_list(
    server: &OrchestratorServer,
    request: Request<StoreListRequest>,
) -> Result<Response<StoreListResponse>, Status> {
    super::authorize(server, &request, "StoreList").map_err(Status::from)?;
    let req = request.into_inner();
    let entries = agent_orchestrator::service::store::store_list(
        &server.state,
        &req.store,
        &req.project,
        req.limit,
        req.offset,
    )
    .await
    .map_err(map_core_error)?;

    Ok(Response::new(StoreListResponse { entries }))
}

pub(crate) async fn store_prune(
    server: &OrchestratorServer,
    request: Request<StorePruneRequest>,
) -> Result<Response<StorePruneResponse>, Status> {
    super::authorize(server, &request, "StorePrune").map_err(Status::from)?;
    let req = request.into_inner();
    agent_orchestrator::service::store::store_prune(&server.state, &req.store, &req.project)
        .await
        .map_err(map_core_error)?;

    Ok(Response::new(StorePruneResponse {
        message: format!("pruned store '{}'", req.store),
    }))
}
