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
            let total: i64 =
                conn.query_row("SELECT COUNT(*) FROM task_execution_metrics", [], |row| {
                    row.get(0)
                })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    /// Helper: insert a row into `task_execution_metrics`.
    async fn insert_metric(db: &AsyncDatabase, task_id: &str, status: &str, created_at: &str) {
        let tid = task_id.to_owned();
        let st = status.to_owned();
        let ca = created_at.to_owned();
        db.writer()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO task_execution_metrics \
                     (task_id, status, current_cycle, unresolved_items, total_items, \
                      failed_items, command_runs, created_at) \
                     VALUES (?1, ?2, 1, 0, 1, 0, 1, ?3)",
                    rusqlite::params![tid, st, ca],
                )?;
                Ok(())
            })
            .await
            .expect("insert_metric");
    }

    #[tokio::test]
    async fn empty_metrics_table() {
        let mut ts = TestState::new();
        let state = ts.build();

        let stats = qa_doctor_stats(&state.async_database).await.unwrap();
        assert_eq!(stats.task_execution_metrics_total, 0);
        assert_eq!(stats.task_execution_metrics_last_24h, 0);
        assert!((stats.task_completion_rate - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn metrics_with_completed_tasks() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_metric(&state.async_database, "t1", "completed", "2024-01-01T00:00:00").await;
        insert_metric(&state.async_database, "t2", "completed", "2024-01-01T00:00:00").await;
        insert_metric(&state.async_database, "t3", "failed", "2024-01-01T00:00:00").await;

        let stats = qa_doctor_stats(&state.async_database).await.unwrap();
        assert_eq!(stats.task_execution_metrics_total, 3);
        // 2 completed out of 3
        assert!((stats.task_completion_rate - 2.0 / 3.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn metrics_last_24h_filters_correctly() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Old row — outside 24h window
        insert_metric(&state.async_database, "old", "completed", "2020-01-01T00:00:00").await;
        // Recent row — use datetime('now') via a direct insert
        state
            .async_database
            .writer()
            .call(|conn| {
                conn.execute(
                    "INSERT INTO task_execution_metrics \
                     (task_id, status, current_cycle, unresolved_items, total_items, \
                      failed_items, command_runs, created_at) \
                     VALUES ('recent', 'completed', 1, 0, 1, 0, 1, datetime('now'))",
                    [],
                )?;
                Ok(())
            })
            .await
            .expect("insert recent metric");

        let stats = qa_doctor_stats(&state.async_database).await.unwrap();
        assert_eq!(stats.task_execution_metrics_total, 2);
        assert_eq!(stats.task_execution_metrics_last_24h, 1);
    }
}
