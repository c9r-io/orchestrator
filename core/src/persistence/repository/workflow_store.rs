//! Workflow-store entry persistence.
//!
//! Moved to `orchestrator-persistence` (FR-130 Phase A) and re-exported here.
//! The tests stay in core: they drive the repository from a
//! `crate::test_utils::TestState` fixture, which sits above this layer.

pub use orchestrator_persistence::repository::workflow_store::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;
    use orchestrator_persistence::test_support;
    use std::sync::Arc;

    const STORE: &str = "test-store";
    const PROJECT: &str = "test-project";
    const TASK: &str = "task-1";

    fn make_repo(state: &Arc<crate::state::InnerState>) -> SqliteWorkflowStoreRepository {
        SqliteWorkflowStoreRepository::new(state.async_database.clone())
    }

    #[tokio::test]
    async fn get_missing_key_returns_none() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        let result = repo.get(STORE, PROJECT, "no-such-key").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn put_and_get_round_trip() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "k1", r#"{"v":1}"#, TASK)
            .await
            .unwrap();
        let val = repo.get(STORE, PROJECT, "k1").await.unwrap();
        assert_eq!(val, Some(r#"{"v":1}"#.to_string()));
    }

    #[tokio::test]
    async fn put_overwrites_existing() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "k1", "old", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "k1", "new", TASK).await.unwrap();
        let val = repo.get(STORE, PROJECT, "k1").await.unwrap();
        assert_eq!(val, Some("new".to_string()));
    }

    #[tokio::test]
    async fn delete_removes_entry() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "k1", "val", TASK).await.unwrap();
        repo.delete(STORE, PROJECT, "k1").await.unwrap();
        let val = repo.get(STORE, PROJECT, "k1").await.unwrap();
        assert!(val.is_none());
    }

    /// Helper: manually set updated_at for a key to a specific offset from now.
    async fn set_updated_at(state: &Arc<crate::state::InnerState>, key: &str, offset: &str) {
        let k = key.to_string();
        let o = offset.to_string();
        test_support::writer(&state.async_database)
            .call(move |conn| {
                conn.execute(
                "UPDATE workflow_store_entries SET updated_at = datetime('now', ?1) WHERE key = ?2",
                rusqlite::params![o, k],
            )?;
                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn list_returns_entries_ordered_by_time() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "a", "1", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "b", "2", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "c", "3", TASK).await.unwrap();
        // Force distinct timestamps: a=-3min, b=-2min, c=-1min
        set_updated_at(&state, "a", "-3 minutes").await;
        set_updated_at(&state, "b", "-2 minutes").await;
        set_updated_at(&state, "c", "-1 minutes").await;
        let rows = repo.list(STORE, PROJECT, 10, 0).await.unwrap();
        assert_eq!(rows.len(), 3);
        // ORDER BY updated_at DESC — most recent first
        assert_eq!(rows[0].key, "c");
        assert_eq!(rows[1].key, "b");
        assert_eq!(rows[2].key, "a");
    }

    #[tokio::test]
    async fn list_respects_limit_and_offset() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "a", "1", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "b", "2", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "c", "3", TASK).await.unwrap();
        // Force distinct timestamps: a=-3min, b=-2min, c=-1min
        set_updated_at(&state, "a", "-3 minutes").await;
        set_updated_at(&state, "b", "-2 minutes").await;
        set_updated_at(&state, "c", "-1 minutes").await;
        let rows = repo.list(STORE, PROJECT, 2, 1).await.unwrap();
        assert_eq!(rows.len(), 2);
        // Offset 1 skips the newest ("c"), so we get "b" and "a"
        assert_eq!(rows[0].key, "b");
        assert_eq!(rows[1].key, "a");
    }

    #[tokio::test]
    async fn prune_by_ttl_removes_old_entries() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "old1", "v1", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "old2", "v2", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "fresh", "v3", TASK).await.unwrap();

        // Manually backdate old1 and old2
        for key in &["old1", "old2"] {
            let k = key.to_string();
            test_support::writer(&state.async_database).call(move |conn| {
                conn.execute(
                    "UPDATE workflow_store_entries SET updated_at = datetime('now', '-30 days') WHERE key = ?1",
                    rusqlite::params![k],
                )?;
                Ok(())
            }).await.unwrap();
        }

        repo.prune(STORE, PROJECT, None, Some(7)).await.unwrap();
        let rows = repo.list(STORE, PROJECT, 100, 0).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "fresh");
    }

    #[tokio::test]
    async fn prune_by_max_entries_keeps_newest() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        for i in 1..=5 {
            let key = format!("k{i}");
            let val = format!("v{i}");
            repo.put(STORE, PROJECT, &key, &val, TASK).await.unwrap();
        }
        // Force distinct timestamps so prune ordering is deterministic
        for i in 1..=5 {
            let offset = format!("-{} minutes", 6 - i); // k1=-5min .. k5=-1min
            set_updated_at(&state, &format!("k{i}"), &offset).await;
        }
        repo.prune(STORE, PROJECT, Some(2), None).await.unwrap();
        let rows = repo.list(STORE, PROJECT, 100, 0).await.unwrap();
        assert_eq!(rows.len(), 2);
        // The two newest should remain: k5 and k4
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert!(keys.contains(&"k5"));
        assert!(keys.contains(&"k4"));
    }
}
