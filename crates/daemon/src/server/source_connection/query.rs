//! Read-only SourceConnection projections: list, get, watch, and the mode catalog.

use super::projection::*;
use super::*;

pub(crate) async fn list(
    server: &OrchestratorServer,
    request: Request<SourceConnectionListRequest>,
) -> Result<Response<SourceConnectionListResponse>, Status> {
    crate::server::authorize(server, &request, "SourceConnectionList").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    let values = repository(server)
        .list(
            &req.project_id,
            req.provider.as_deref(),
            req.include_disconnected,
            if req.limit == 0 {
                100
            } else {
                req.limit as usize
            },
        )
        .await
        .map_err(internal)?;
    Ok(Response::new(SourceConnectionListResponse {
        connections: values.into_iter().map(connection_to_proto).collect(),
    }))
}

pub(crate) async fn get(
    server: &OrchestratorServer,
    request: Request<SourceConnectionGetRequest>,
) -> Result<Response<SourceConnection>, Status> {
    crate::server::authorize(server, &request, "SourceConnectionGet").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    validate_id(&req.id, "connection id")?;
    let connection = repository(server)
        .get(&req.project_id, &req.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| Status::not_found("SourceConnection not found"))?;
    Ok(Response::new(connection_to_proto(connection)))
}

pub(crate) async fn watch(
    server: &OrchestratorServer,
    request: Request<SourceConnectionWatchRequest>,
) -> Result<Response<SourceConnectionWatchStream>, Status> {
    crate::server::authorize(server, &request, "SourceConnectionWatch").map_err(Status::from)?;
    let req = request.into_inner();
    validate_project(&req.project_id)?;
    let repository = repository(server);
    let interval = std::time::Duration::from_millis(if req.interval_millis == 0 {
        500
    } else {
        req.interval_millis.clamp(250, 5_000) as u64
    });
    let (sender, receiver) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        let mut cursor = req.after_cursor.max(0);
        loop {
            let changes = match repository.changes(&req.project_id, cursor, 200).await {
                Ok(value) => value,
                Err(error) => {
                    let _ = sender.send(Err(internal(error))).await;
                    return;
                }
            };
            for change in changes {
                cursor = change.cursor;
                let connection = match repository.get(&req.project_id, &change.connection_id).await
                {
                    Ok(Some(value)) => value,
                    Ok(None) => continue,
                    Err(error) => {
                        let _ = sender.send(Err(internal(error))).await;
                        return;
                    }
                };
                if sender
                    .send(Ok(SourceConnectionDelta {
                        cursor: change.cursor,
                        connection_version: change.connection_version,
                        state: change.state.as_str().to_string(),
                        error_code: change.error_code,
                        request_id: change.request_id,
                        connection: Some(connection_to_proto(connection)),
                        changed_at: change.created_at,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            tokio::time::sleep(interval).await;
        }
    });
    Ok(Response::new(Box::pin(
        tokio_stream::wrappers::ReceiverStream::new(receiver),
    )))
}

pub(crate) async fn catalog(
    server: &OrchestratorServer,
    request: Request<SourceConnectionCatalogRequest>,
) -> Result<Response<SourceConnectionCatalogResponse>, Status> {
    crate::server::authorize(server, &request, "SourceConnectionCatalogGet")
        .map_err(Status::from)?;
    let Some(gateway) = server.slack_gateway.as_ref() else {
        return Ok(Response::new(SourceConnectionCatalogResponse {
            protocol_version: 1,
            modes: vec![
                mode_capability("managed_shared", false, Some("gateway_not_configured")),
                mode_capability("managed_dedicated", false, Some("gateway_not_configured")),
                mode_capability("manual", true, None),
            ],
            gateway_configured: false,
            permalink_proxy: false,
        }));
    };
    let capabilities = gateway.capabilities().await.map_err(unavailable)?;
    if capabilities.protocol_version != 1 || capabilities.max_delivery_batch == 0 {
        return Err(Status::failed_precondition(
            "Slack Gateway protocol capability mismatch",
        ));
    }
    Ok(Response::new(SourceConnectionCatalogResponse {
        protocol_version: capabilities.protocol_version,
        modes: vec![
            mode_capability(
                "managed_shared",
                capabilities
                    .supported_modes
                    .iter()
                    .any(|value| value == "managed_shared"),
                None,
            ),
            mode_capability(
                "managed_dedicated",
                capabilities
                    .supported_modes
                    .iter()
                    .any(|value| value == "managed_dedicated"),
                None,
            ),
            mode_capability("manual", true, None),
        ],
        gateway_configured: true,
        permalink_proxy: capabilities.permalink_proxy,
    }))
}
