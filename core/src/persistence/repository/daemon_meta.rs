//! Daemon metadata persistence (incarnation counter, etc.).
//!
//! Moved to `orchestrator-persistence` (FR-130 Phase A) and re-exported here.
//! The tests stay in core: they drive the repository from a
//! `crate::test_utils::TestState` fixture, which sits above this layer.

pub use orchestrator_persistence::repository::daemon_meta::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    #[tokio::test]
    async fn increment_incarnation_starts_from_one() {
        let mut ts = TestState::new();
        let state = ts.build();
        let result = increment_incarnation(&state.async_database).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn increment_incarnation_returns_increasing_values() {
        let mut ts = TestState::new();
        let state = ts.build();
        let first = increment_incarnation(&state.async_database).await.unwrap();
        let second = increment_incarnation(&state.async_database).await.unwrap();
        assert!(
            second > first,
            "second ({second}) should be greater than first ({first})"
        );
    }
}
