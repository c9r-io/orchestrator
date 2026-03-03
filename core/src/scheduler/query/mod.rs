//! Task query operations - modularized for single responsibility.
//!
//! This module provides task query, log streaming, and watch functionality.
//! It is organized into submodules by responsibility:
//!
//! - [`task_queries`] - Task CRUD operations (resolve, load, list, get, delete)
//! - [`log_stream`] - Log streaming and file tailing
//! - [`watch`] - Real-time task monitoring
//! - [`format`] - Display formatting utilities

use crate::anomaly::AnomalyRule;
use anyhow::Context;
use std::time::{Duration, Instant};

mod format;
mod log_stream;
mod task_queries;
mod watch;

pub use task_queries::{
    delete_task_impl, get_task_details_impl, list_tasks_impl, load_task_summary, resolve_task_id,
};
pub use log_stream::{follow_task_logs, stream_task_logs_impl};
pub use watch::watch_task;

const QUERY_RETRY_ATTEMPTS: usize = 3;
const QUERY_RETRY_DELAY_MS: u64 = 75;
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

/// Retry a query operation with fixed-delay retries on transient errors.
fn retry_query<T, F>(label: &str, f: F) -> anyhow::Result<T>
where
    F: Fn() -> anyhow::Result<T>,
{
    let mut last_err = None;
    for attempt in 0..QUERY_RETRY_ATTEMPTS {
        match f() {
            Ok(value) => return Ok(value),
            Err(err) if is_transient_query_error(&err) && attempt + 1 < QUERY_RETRY_ATTEMPTS => {
                last_err = Some(err);
                std::thread::sleep(std::time::Duration::from_millis(QUERY_RETRY_DELAY_MS));
            }
            Err(err) => {
                return Err(err).with_context(|| format!("{label} failed"));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{label} failed")))
        .with_context(|| format!("{label} failed"))
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
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    pub fn test_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("query-test-{}-{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create query test dir");
        dir
    }

    /// Create a TestState, seed a QA file, create a task, return (state, task_id).
    pub fn seed_task(
        fixture: &mut TestState,
    ) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .app_root
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
    pub fn first_item_id(state: &crate::state::InnerState, task_id: &str) -> String {
        let conn = crate::db::open_conn(&state.db_path).expect("open db");
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
    fn retry_query_retries_transient_error_then_succeeds() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let value = retry_query("transient test", move || {
            let attempt = attempts_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt < 2 {
                Err(anyhow::anyhow!("database is locked"))
            } else {
                Ok(42)
            }
        })
        .expect("retry query should succeed");

        assert_eq!(value, 42);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn retry_query_does_not_retry_permanent_error() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let result: anyhow::Result<i32> = retry_query("permanent test", move || {
            attempts_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(anyhow::anyhow!("task not found: deadbeef"))
        });

        assert!(result.is_err());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
