use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{OrchestratorServer, map_core_error};

pub(crate) async fn ping(
    server: &OrchestratorServer,
    request: Request<PingRequest>,
) -> Result<Response<PingResponse>, Status> {
    super::authorize(server, &request, "Ping").map_err(Status::from)?;
    let runtime = agent_orchestrator::service::daemon::runtime_snapshot(&server.state);
    Ok(Response::new(PingResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_hash: env!("BUILD_GIT_HASH").to_string(),
        uptime_secs: runtime.uptime_secs.to_string(),
        shutdown_requested: runtime.shutdown_requested,
        lifecycle_state: runtime.lifecycle_state.as_str().to_string(),
        maintenance_mode: runtime.maintenance_mode,
        incarnation: runtime.incarnation,
    }))
}

pub(crate) async fn shutdown(
    server: &OrchestratorServer,
    request: Request<ShutdownRequest>,
) -> Result<Response<ShutdownResponse>, Status> {
    super::authorize(server, &request, "Shutdown").map_err(Status::from)?;
    let req = request.into_inner();
    tracing::info!(graceful = req.graceful, "shutdown requested via RPC");
    server.state.daemon_runtime.request_shutdown();
    server.shutdown_notify.notify_one();
    Ok(Response::new(ShutdownResponse {
        message: "shutdown initiated".to_string(),
    }))
}

pub(crate) async fn maintenance_mode(
    server: &OrchestratorServer,
    request: Request<MaintenanceModeRequest>,
) -> Result<Response<MaintenanceModeResponse>, Status> {
    super::authorize(server, &request, "MaintenanceMode").map_err(Status::from)?;
    let req = request.into_inner();
    server.state.daemon_runtime.set_maintenance_mode(req.enable);
    let state_str = if req.enable { "enabled" } else { "disabled" };
    tracing::info!(enable = req.enable, "maintenance mode {state_str}");
    Ok(Response::new(MaintenanceModeResponse {
        maintenance_mode: req.enable,
        message: format!("maintenance mode {state_str}"),
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
            .map_err(map_core_error)?;

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
        .map_err(map_core_error)?;

    Ok(Response::new(status))
}

pub(crate) async fn check(
    server: &OrchestratorServer,
    request: Request<CheckRequest>,
) -> Result<Response<CheckResponse>, Status> {
    super::authorize(server, &request, "Check").map_err(Status::from)?;
    let req = request.into_inner();
    let report = orchestrator_scheduler::service::system::run_check(
        &server.state,
        req.workflow.as_deref(),
        &req.output_format,
        req.project_id.as_deref(),
    )
    .map_err(map_core_error)?;

    Ok(Response::new(CheckResponse {
        content: report.content,
        format: req.output_format,
        exit_code: report.exit_code,
        diagnostics: report
            .report
            .checks
            .iter()
            .map(orchestrator_scheduler::service::system::diagnostic_entry_from_check)
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
        .map_err(map_core_error)?;
    Ok(Response::new(InitResponse { message }))
}

pub(crate) async fn db_status(
    server: &OrchestratorServer,
    request: Request<DbStatusRequest>,
) -> Result<Response<DbStatusResponse>, Status> {
    super::authorize(server, &request, "DbStatus").map_err(Status::from)?;
    let status =
        agent_orchestrator::service::system::db_status(&server.state).map_err(map_core_error)?;
    Ok(Response::new(status))
}

pub(crate) async fn db_vacuum(
    server: &OrchestratorServer,
    request: Request<DbVacuumRequest>,
) -> Result<Response<DbVacuumResponse>, Status> {
    super::authorize(server, &request, "DbVacuum").map_err(Status::from)?;
    let result = agent_orchestrator::db_maintenance::vacuum_database(&server.state.db_path)
        .map_err(|e| Status::internal(e.to_string()))?;
    let freed = result.size_before.saturating_sub(result.size_after);
    Ok(Response::new(DbVacuumResponse {
        size_before: result.size_before,
        size_after: result.size_after,
        message: format!(
            "VACUUM complete: {} -> {} (freed {})",
            format_bytes(result.size_before),
            format_bytes(result.size_after),
            format_bytes(freed),
        ),
    }))
}

pub(crate) async fn db_log_cleanup(
    server: &OrchestratorServer,
    request: Request<DbLogCleanupRequest>,
) -> Result<Response<DbLogCleanupResponse>, Status> {
    super::authorize(server, &request, "DbLogCleanup").map_err(Status::from)?;
    let req = request.into_inner();
    let days = if req.older_than_days == 0 {
        30
    } else {
        req.older_than_days
    };
    let result = agent_orchestrator::log_cleanup::cleanup_old_logs(
        &server.state.async_database,
        &server.state.logs_dir,
        days,
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(Response::new(DbLogCleanupResponse {
        files_deleted: result.files_deleted,
        bytes_freed: result.bytes_freed,
        message: format!(
            "Deleted {} file(s), freed {}",
            result.files_deleted,
            format_bytes(result.bytes_freed),
        ),
    }))
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

pub(crate) async fn db_migrations_list(
    server: &OrchestratorServer,
    request: Request<DbMigrationsListRequest>,
) -> Result<Response<DbMigrationsListResponse>, Status> {
    super::authorize(server, &request, "DbMigrationsList").map_err(Status::from)?;
    let list = agent_orchestrator::service::system::db_migrations_list(&server.state)
        .map_err(map_core_error)?;
    Ok(Response::new(list))
}

pub(crate) async fn event_cleanup(
    server: &OrchestratorServer,
    request: Request<EventCleanupRequest>,
) -> Result<Response<EventCleanupResponse>, Status> {
    super::authorize(server, &request, "EventCleanup").map_err(Status::from)?;
    let req = request.into_inner();
    let older_than = if req.older_than_days == 0 {
        30
    } else {
        req.older_than_days
    };

    if req.dry_run {
        let count = agent_orchestrator::event_cleanup::count_pending_cleanup(
            &server.state.async_database,
            older_than,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        return Ok(Response::new(EventCleanupResponse {
            affected_count: count,
            message: format!(
                "{count} events would be deleted (dry-run, older than {older_than} days)"
            ),
        }));
    }

    let affected = if req.archive {
        let archive_dir = server.state.data_dir.join("archive/events");
        agent_orchestrator::event_cleanup::archive_events(
            &server.state.async_database,
            &archive_dir,
            older_than,
            1000,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?
    } else {
        agent_orchestrator::event_cleanup::cleanup_old_events(
            &server.state.async_database,
            older_than,
            1000,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?
    };
    Ok(Response::new(EventCleanupResponse {
        affected_count: affected,
        message: format!("{affected} events deleted (older than {older_than} days)"),
    }))
}

pub(crate) async fn event_stats(
    server: &OrchestratorServer,
    request: Request<EventStatsRequest>,
) -> Result<Response<EventStatsResponse>, Status> {
    super::authorize(server, &request, "EventStats").map_err(Status::from)?;
    let stats = agent_orchestrator::event_cleanup::event_stats(&server.state.async_database)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    Ok(Response::new(EventStatsResponse {
        total_rows: stats.total_rows,
        earliest: stats.earliest.unwrap_or_default(),
        latest: stats.latest.unwrap_or_default(),
        by_task_status: stats
            .by_task_status
            .into_iter()
            .map(|(status, count)| EventStatusCount { status, count })
            .collect(),
    }))
}

pub(crate) async fn task_events(
    server: &OrchestratorServer,
    request: Request<TaskEventsRequest>,
) -> Result<Response<TaskEventsResponse>, Status> {
    super::authorize(server, &request, "TaskEvents").map_err(Status::from)?;
    let req = request.into_inner();
    let resolved_id =
        orchestrator_scheduler::service::task::resolve_id(&server.state, &req.task_id)
            .await
            .map_err(map_core_error)?;
    let type_filter = if req.event_type_filter.is_empty() {
        None
    } else {
        Some(req.event_type_filter.as_str())
    };
    let events = agent_orchestrator::event_cleanup::list_task_events(
        &server.state.async_database,
        &resolved_id,
        type_filter,
        req.limit,
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;
    Ok(Response::new(TaskEventsResponse {
        events: events
            .into_iter()
            .map(super::mapping::event_to_proto)
            .collect(),
    }))
}

pub(crate) async fn qa_doctor(
    server: &OrchestratorServer,
    request: Request<QaDoctorRequest>,
) -> Result<Response<QaDoctorResponse>, Status> {
    super::authorize(server, &request, "QaDoctor").map_err(Status::from)?;
    let stats = agent_orchestrator::qa_doctor::qa_doctor_stats(&server.state.async_database)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    Ok(Response::new(QaDoctorResponse {
        task_execution_metrics_total: stats.task_execution_metrics_total,
        task_execution_metrics_last_24h: stats.task_execution_metrics_last_24h,
        task_completion_rate: stats.task_completion_rate,
    }))
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
    .map_err(map_core_error)?;
    Ok(Response::new(ManifestValidateResponse {
        valid: report.valid,
        errors: report.errors,
        message: report.message,
        diagnostics: report.diagnostics,
    }))
}
