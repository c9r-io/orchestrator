//! QA doctor observability queries for `task_execution_metrics`.

use anyhow::Result;

use crate::async_database::AsyncDatabase;

/// Aggregated QA observability statistics.
pub struct QaDoctorStats {
    /// Total rows in `task_execution_metrics`.
    pub task_execution_metrics_total: u64,
    /// Rows created within the last 24 hours.
    pub task_execution_metrics_last_24h: u64,
    /// Fraction of rows with `status = 'completed'` (0.0 when table is empty).
    pub task_completion_rate: f64,
}

/// Query `task_execution_metrics` for the three FR-088 observability indicators.
pub async fn qa_doctor_stats(db: &AsyncDatabase) -> Result<QaDoctorStats> {
    let stats = db
        .reader()
        .call(|conn| {
            let total: i64 = conn.query_row(
                "SELECT COUNT(*) FROM task_execution_metrics",
                [],
                |row| row.get(0),
            )?;

            let last_24h: i64 = conn.query_row(
                "SELECT COUNT(*) FROM task_execution_metrics \
                 WHERE created_at >= datetime('now', '-24 hours')",
                [],
                |row| row.get(0),
            )?;

            let completed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM task_execution_metrics WHERE status = 'completed'",
                [],
                |row| row.get(0),
            )?;

            let rate = if total > 0 {
                completed as f64 / total as f64
            } else {
                0.0
            };

            Ok(QaDoctorStats {
                task_execution_metrics_total: total as u64,
                task_execution_metrics_last_24h: last_24h as u64,
                task_completion_rate: rate,
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(stats)
}
