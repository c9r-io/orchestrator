//! Task query operations - modularized for single responsibility.
//!
//! This module provides task query, log streaming, and watch functionality.
//! It is organized into submodules by responsibility:
//!
//! - [`task_queries`] - Task CRUD operations (resolve, load, list, get, delete)
//! - [`log_stream`] - Log streaming and file tailing
//! - [`watch`] - Real-time task monitoring
//! - [`format`] - Display formatting utilities

use agent_orchestrator::anomaly::AnomalyRule;
use std::time::{Duration, Instant};

mod format;
mod log_stream;
mod task_queries;
mod watch;

pub use log_stream::{follow_task_logs, stream_task_logs_impl};
pub use task_queries::{
    delete_task_impl, get_task_details_impl, list_tasks_impl, load_task_summary, resolve_task_id,
};
pub use watch::watch_task;

const FOLLOW_WARNING_THROTTLE_SECS: u64 = 5;

/// Check if an error is transient and should be retried.
fn is_transient_query_error(err: &anyhow::Error) -> bool {
    let message = err.to_string();
    [
        "database is locked",
        "failed to open sqlite db",
        "failed to read log file",
        "failed to seek log file",
        "read stdout tail",
        "read stderr tail",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// Emit an anomaly warning to stderr, throttled to avoid flooding.
fn emit_anomaly_warning(rule: &AnomalyRule, message: &str, last_warning_at: &mut Option<Instant>) {
    let should_print = last_warning_at
        .map(|at| at.elapsed() >= Duration::from_secs(FOLLOW_WARNING_THROTTLE_SECS))
        .unwrap_or(true);
    if should_print {
        eprintln!(
            "[{}: {}] {}",
            rule.escalation().label(),
            rule.canonical_name(),
            message,
        );
        *last_warning_at = Some(Instant::now());
    }
}

#[cfg(test)]
pub(super) mod test_fixtures {
    use agent_orchestrator::dto::CreateTaskPayload;
    use agent_orchestrator::task_ops::create_task_impl;
    use agent_orchestrator::test_utils::TestState;

    pub fn test_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("query-test-{}-{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create query test dir");
        dir
    }

    /// Create a TestState, seed a QA file, create a task, return (state, task_id).
    pub fn seed_task(
        fixture: &mut TestState,
    ) -> (
        std::sync::Arc<agent_orchestrator::state::InnerState>,
        String,
    ) {
        let state = fixture.build();
        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/query_test.md");
        std::fs::write(&qa_file, "# query test\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("query-test".to_string()),
                goal: Some("query-test-goal".to_string()),
                ..Default::default()
            },
        )
        .expect("task should be created");
        (state, created.id)
    }

    /// Get the first task_item id for a given task.
    pub fn first_item_id(state: &agent_orchestrator::state::InnerState, task_id: &str) -> String {
        let conn = agent_orchestrator::db::open_conn(&state.db_path).expect("open db");
        conn.query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            rusqlite::params![task_id],
            |row| row.get(0),
        )
        .expect("task item should exist")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_transient_query_error_matches_known_messages() {
        let err = anyhow::anyhow!("failed to read log file: missing");
        assert!(is_transient_query_error(&err));

        let err = anyhow::anyhow!("database is locked");
        assert!(is_transient_query_error(&err));

        let err = anyhow::anyhow!("permanent failure");
        assert!(!is_transient_query_error(&err));
    }

    #[test]
    fn emit_anomaly_warning_updates_throttle_timestamp() {
        let mut last_warning_at = None;
        emit_anomaly_warning(
            &AnomalyRule::TransientReadError,
            "temporary read failure",
            &mut last_warning_at,
        );
        assert!(last_warning_at.is_some());

        let first = last_warning_at;
        emit_anomaly_warning(
            &AnomalyRule::TransientReadError,
            "temporary read failure",
            &mut last_warning_at,
        );
        assert_eq!(last_warning_at, first);

        last_warning_at = Some(Instant::now() - Duration::from_secs(FOLLOW_WARNING_THROTTLE_SECS));
        emit_anomaly_warning(
            &AnomalyRule::TransientReadError,
            "temporary read failure",
            &mut last_warning_at,
        );
        assert!(last_warning_at.unwrap().elapsed() < Duration::from_secs(1));
    }
}
