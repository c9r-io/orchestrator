use crate::state::InnerState;
use crate::store::{StoreOp, StoreOpResult};
use anyhow::Result;

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
        StoreOpResult::Value(Some(v)) => Ok(Some(serde_json::to_string_pretty(&v)?)),
        StoreOpResult::Value(None) => Ok(None),
        _ => Ok(None),
    }
}

pub async fn store_put(
    state: &InnerState,
    store: &str,
    key: &str,
    value: &str,
    project: &str,
    task_id: &str,
) -> Result<()> {
    let op = StoreOp::Put {
        store_name: store.to_string(),
        project_id: project.to_string(),
        key: key.to_string(),
        value: value.to_string(),
        task_id: task_id.to_string(),
    };
    execute_store_op(state, op).await?;
    Ok(())
}

pub async fn store_delete(
    state: &InnerState,
    store: &str,
    key: &str,
    project: &str,
) -> Result<()> {
    let op = StoreOp::Delete {
        store_name: store.to_string(),
        project_id: project.to_string(),
        key: key.to_string(),
    };
    execute_store_op(state, op).await?;
    Ok(())
}

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

pub async fn store_prune(state: &InnerState, store: &str, project: &str) -> Result<()> {
    use crate::crd::projection::CrdProjectable as _;

    let store_config = {
        let config = state
            .active_config
            .read()
            .map_err(|_| anyhow::anyhow!("failed to read active config"))?;
        let key = format!("WorkflowStore/{}", store);
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
    let custom_resources = {
        let config = state
            .active_config
            .read()
            .map_err(|_| anyhow::anyhow!("failed to read active config"))?;
        config.config.custom_resources.clone()
    };
    state.store_manager.execute(&custom_resources, op).await
}
