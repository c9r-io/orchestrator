use crate::error::{OrchestratorError, Result, classify_store_error};
use crate::state::InnerState;
use crate::store::{StoreOp, StoreOpResult};

/// Reads a single workflow-store entry.
///
/// A stored JSON string is returned as its contents, not as a quoted JSON
/// literal, so that `store put k v` followed by `store get k` yields `v`. Every
/// other shape is returned as formatted JSON.
///
/// This is the same unwrapping the retired `store_inputs` binding did before it
/// injected a value into a step (`Value::String(s) => s.clone()`), and steps
/// read stores through this path now that the binding is gone (FR-156). Without
/// it a shell step gets a value with literal quote characters in it, which then
/// silently fails whatever it is interpolated into.
pub async fn store_get(
    state: &InnerState,
    store: &str,
    key: &str,
    project: &str,
) -> Result<Option<String>> {
    let op = StoreOp::Get {
        store_name: store.to_string(),
        project_id: project.to_string(),
        key: key.to_string(),
    };
    let result = execute_store_op(state, op).await?;
    match result {
        StoreOpResult::Value(Some(serde_json::Value::String(text))) => Ok(Some(text)),
        StoreOpResult::Value(Some(v)) => serde_json::to_string_pretty(&v)
            .map(Some)
            .map_err(|err| classify_store_error("store.get", err)),
        StoreOpResult::Value(None) => Ok(None),
        _ => Ok(None),
    }
}

/// Writes a payload into a workflow store entry.
///
/// The backend stores JSON and parses on read, so a value that is not JSON is
/// encoded as a JSON string first. Without this, `store put k 280e3c` succeeds
/// and the matching `store get` fails with "failed to parse stored JSON value":
/// the write is accepted and the entry is unreadable, by this path and by every
/// other reader of that store.
pub async fn store_put(
    state: &InnerState,
    store: &str,
    key: &str,
    value: &str,
    project: &str,
    task_id: &str,
) -> Result<()> {
    let value = match serde_json::from_str::<serde_json::Value>(value) {
        Ok(_) => value.to_string(),
        Err(_) => serde_json::Value::String(value.to_string()).to_string(),
    };
    let op = StoreOp::Put {
        store_name: store.to_string(),
        project_id: project.to_string(),
        key: key.to_string(),
        value,
        task_id: task_id.to_string(),
    };
    execute_store_op(state, op).await?;
    Ok(())
}

/// Deletes a single key from a workflow store.
pub async fn store_delete(state: &InnerState, store: &str, key: &str, project: &str) -> Result<()> {
    let op = StoreOp::Delete {
        store_name: store.to_string(),
        project_id: project.to_string(),
        key: key.to_string(),
    };
    execute_store_op(state, op).await?;
    Ok(())
}

/// Lists workflow-store entries for a project and converts them to proto responses.
pub async fn store_list(
    state: &InnerState,
    store: &str,
    project: &str,
    limit: u64,
    offset: u64,
) -> Result<Vec<orchestrator_proto::StoreEntry>> {
    let op = StoreOp::List {
        store_name: store.to_string(),
        project_id: project.to_string(),
        limit,
        offset,
    };
    let result = execute_store_op(state, op).await?;
    match result {
        StoreOpResult::Entries(entries) => {
            let protos = entries
                .into_iter()
                .map(|e| orchestrator_proto::StoreEntry {
                    key: e.key,
                    value_json: serde_json::to_string(&e.value).unwrap_or_default(),
                    updated_at: e.updated_at,
                })
                .collect();
            Ok(protos)
        }
        _ => Ok(Vec::new()),
    }
}

/// Applies the retention policy configured on a workflow store.
pub async fn store_prune(state: &InnerState, store: &str, project: &str) -> Result<()> {
    use crate::crd::projection::CrdProjectable as _;

    let store_config = {
        let config = crate::config_load::read_loaded_config(state)?;
        let key = format!("WorkflowStore/{store}");
        config
            .config
            .custom_resources
            .get(&key)
            .and_then(|cr| crate::config::WorkflowStoreConfig::from_cr_spec(&cr.spec).ok())
            .unwrap_or_default()
    };

    let op = StoreOp::Prune {
        store_name: store.to_string(),
        project_id: project.to_string(),
        max_entries: store_config.retention.max_entries,
        ttl_days: store_config.retention.ttl_days,
    };
    execute_store_op(state, op).await?;
    Ok(())
}

async fn execute_store_op(state: &InnerState, op: StoreOp) -> Result<StoreOpResult> {
    let custom_resources = crate::config_load::read_loaded_config(state)
        .map_err(|err| OrchestratorError::external_dependency("store.active_config", err))?
        .config
        .custom_resources
        .clone();
    state
        .store_manager
        .execute(&custom_resources, op)
        .await
        .map_err(|err| classify_store_error("store.execute", err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::resource::apply_manifests;
    use crate::test_utils::TestState;

    fn workflow_store_manifest(name: &str, max_entries: u64) -> String {
        format!(
            "apiVersion: orchestrator.dev/v2\nkind: WorkflowStore\nmetadata:\n  name: {name}\nspec:\n  provider: local\n  retention:\n    max_entries: {max_entries}\n"
        )
    }

    #[tokio::test]
    async fn a_non_json_scalar_round_trips_as_itself() {
        // Before FR-156 this write succeeded and the read failed with
        // "failed to parse stored JSON value": the backend stores JSON and
        // parses on read, and nothing encoded the value on the way in. The
        // entry was accepted and unreadable, by this path and by every other
        // reader of that store. docs/workflow/promotion.yaml had been writing
        // a bare SHA that way for its whole life.
        let mut fixture = TestState::new();
        let state = fixture.build();
        let sha = "280e3c8ce2b2d43e1dc5011234567890abcdef12";

        store_put(
            &state,
            "promotion",
            "last_published_sha",
            sha,
            crate::config::DEFAULT_PROJECT_ID,
            "task-1",
        )
        .await
        .expect("store put");

        let loaded = store_get(
            &state,
            "promotion",
            "last_published_sha",
            crate::config::DEFAULT_PROJECT_ID,
        )
        .await
        .expect("store get");

        // Byte-for-byte, with no quote characters: a shell step interpolates
        // this straight into a command, so a quoted literal would be a silent
        // failure rather than an error.
        assert_eq!(loaded.as_deref(), Some(sha));
    }

    #[tokio::test]
    async fn an_explicit_json_string_reads_back_unwrapped_too() {
        // The value the caller wrote is already valid JSON, so it is stored
        // verbatim rather than double-encoded, and the read unwraps it. Both
        // halves matter: encoding it again would give \"v\" on read.
        let mut fixture = TestState::new();
        let state = fixture.build();

        store_put(
            &state,
            "s",
            "quoted",
            r#""already-json""#,
            crate::config::DEFAULT_PROJECT_ID,
            "task-1",
        )
        .await
        .expect("store put");

        let loaded = store_get(&state, "s", "quoted", crate::config::DEFAULT_PROJECT_ID)
            .await
            .expect("store get");

        assert_eq!(loaded.as_deref(), Some("already-json"));
    }

    #[tokio::test]
    async fn a_json_object_is_still_returned_as_json() {
        // The unwrapping is for strings only. An object read back as anything
        // but JSON would break every caller that parses the output.
        let mut fixture = TestState::new();
        let state = fixture.build();

        store_put(
            &state,
            "s",
            "obj",
            r#"{"winner_id":"item-7"}"#,
            crate::config::DEFAULT_PROJECT_ID,
            "task-1",
        )
        .await
        .expect("store put");

        let loaded = store_get(&state, "s", "obj", crate::config::DEFAULT_PROJECT_ID)
            .await
            .expect("store get")
            .expect("stored value");
        let parsed: serde_json::Value = serde_json::from_str(&loaded).expect("still JSON");
        assert_eq!(parsed["winner_id"], "item-7");
    }

    #[tokio::test]
    async fn store_put_get_list_delete_round_trip() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        store_put(
            &state,
            "memories",
            "alpha",
            r#"{"score":1,"name":"first"}"#,
            crate::config::DEFAULT_PROJECT_ID,
            "task-1",
        )
        .await
        .expect("store put");

        let loaded = store_get(
            &state,
            "memories",
            "alpha",
            crate::config::DEFAULT_PROJECT_ID,
        )
        .await
        .expect("store get");
        assert!(loaded.expect("stored value").contains("\"score\": 1"));

        let entries = store_list(&state, "memories", crate::config::DEFAULT_PROJECT_ID, 10, 0)
            .await
            .expect("store list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "alpha");

        store_delete(
            &state,
            "memories",
            "alpha",
            crate::config::DEFAULT_PROJECT_ID,
        )
        .await
        .expect("store delete");
        let missing = store_get(
            &state,
            "memories",
            "alpha",
            crate::config::DEFAULT_PROJECT_ID,
        )
        .await
        .expect("store get after delete");
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn store_prune_uses_workflow_store_retention() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        apply_manifests(
            &state,
            &workflow_store_manifest("ranked", 1),
            false,
            Some(crate::config::DEFAULT_PROJECT_ID),
            false,
        )
        .expect("apply workflow store");

        store_put(
            &state,
            "ranked",
            "first",
            r#"{"rank":1}"#,
            crate::config::DEFAULT_PROJECT_ID,
            "task-1",
        )
        .await
        .expect("put first");
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        store_put(
            &state,
            "ranked",
            "second",
            r#"{"rank":2}"#,
            crate::config::DEFAULT_PROJECT_ID,
            "task-2",
        )
        .await
        .expect("put second");

        store_prune(&state, "ranked", crate::config::DEFAULT_PROJECT_ID)
            .await
            .expect("prune ranked store");

        let entries = store_list(&state, "ranked", crate::config::DEFAULT_PROJECT_ID, 10, 0)
            .await
            .expect("list ranked store");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "second");
    }
}
