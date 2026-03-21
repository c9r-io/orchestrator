use serde::Serialize;
use tauri::State;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct CleanupResult {
    pub affected_count: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventStatsResult {
    pub total_rows: u64,
    pub earliest: String,
    pub latest: String,
}

/// Clean up old events (admin).
#[tauri::command]
pub async fn event_cleanup(
    state: State<'_, Arc<AppState>>,
    older_than_days: u32,
    dry_run: Option<bool>,
    archive: Option<bool>,
) -> Result<CleanupResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .event_cleanup(orchestrator_proto::EventCleanupRequest {
            older_than_days,
            dry_run: dry_run.unwrap_or(false),
            archive: archive.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(CleanupResult {
        affected_count: inner.affected_count,
        message: inner.message,
    })
}

/// Get event statistics (read_only+).
#[tauri::command]
pub async fn event_stats(state: State<'_, Arc<AppState>>) -> Result<EventStatsResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .event_stats(orchestrator_proto::EventStatsRequest {})
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(EventStatsResult {
        total_rows: inner.total_rows,
        earliest: inner.earliest,
        latest: inner.latest,
    })
}
