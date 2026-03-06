//! Persistent Store — cross-task workflow memory with pluggable backends.
//!
//! Three-layer architecture (analogous to K8s StorageClass pattern):
//! - `StoreBackendProvider` CRD: defines HOW a backend works
//! - `WorkflowStore` CRD: defines WHAT store to use
//! - Store entries: actual data managed by the provider

mod command;
mod file;
mod local;
mod validate;

pub use command::CommandAdapter;
pub use file::FileStoreBackend;
pub use local::LocalStoreBackend;
pub use validate::validate_schema;

use crate::async_database::AsyncDatabase;
use crate::config::{StoreBackendProviderConfig, WorkflowStoreConfig};
use crate::crd::projection::CrdProjectable;
use crate::crd::store::ResourceStore;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Operations that can be performed on a store.
#[derive(Debug, Clone)]
pub enum StoreOp {
    Get {
        store_name: String,
        project_id: String,
        key: String,
    },
    Put {
        store_name: String,
        project_id: String,
        key: String,
        value: String,
        task_id: String,
    },
    Delete {
        store_name: String,
        project_id: String,
        key: String,
    },
    List {
        store_name: String,
        project_id: String,
        limit: u64,
        offset: u64,
    },
    Prune {
        store_name: String,
        project_id: String,
        max_entries: Option<u64>,
        ttl_days: Option<u64>,
    },
}

/// Result of a store operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StoreOpResult {
    /// Value retrieved (None if key not found).
    Value(Option<serde_json::Value>),
    /// List of entries.
    Entries(Vec<StoreEntry>),
    /// Operation succeeded with no return value.
    Ok,
}

/// A single entry in a workflow store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreEntry {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: String,
}

/// Manages store operations, dispatching to the appropriate backend.
pub struct StoreManager {
    #[allow(dead_code)] // retained for future direct DB queries
    async_db: Arc<AsyncDatabase>,
    local_backend: LocalStoreBackend,
    file_backend: FileStoreBackend,
    command_adapter: CommandAdapter,
}

impl StoreManager {
    pub fn new(async_db: Arc<AsyncDatabase>, app_root: std::path::PathBuf) -> Self {
        Self {
            local_backend: LocalStoreBackend::new(async_db.clone()),
            file_backend: FileStoreBackend::new(app_root),
            command_adapter: CommandAdapter,
            async_db,
        }
    }

    /// Execute a store operation, dispatching to the correct backend.
    pub async fn execute(
        &self,
        resource_store: &ResourceStore,
        op: StoreOp,
    ) -> Result<StoreOpResult> {
        let store_name = match &op {
            StoreOp::Get { store_name, .. }
            | StoreOp::Put { store_name, .. }
            | StoreOp::Delete { store_name, .. }
            | StoreOp::List { store_name, .. }
            | StoreOp::Prune { store_name, .. } => store_name.clone(),
        };

        // Resolve WorkflowStore config (auto-provision with defaults if not declared)
        let store_config = self.resolve_store_config(resource_store, &store_name);

        // Validate schema on put
        if let StoreOp::Put { ref value, .. } = op {
            if let Some(ref schema) = store_config.schema {
                let parsed: serde_json::Value = serde_json::from_str(value)
                    .map_err(|e| anyhow!("invalid JSON value for store put: {}", e))?;
                validate_schema(&parsed, schema)?;
            }
        }

        let provider_name = &store_config.provider;
        self.dispatch(resource_store, provider_name, op).await
    }

    fn resolve_store_config(
        &self,
        resource_store: &ResourceStore,
        store_name: &str,
    ) -> WorkflowStoreConfig {
        resource_store
            .get("WorkflowStore", store_name)
            .and_then(|cr| WorkflowStoreConfig::from_cr_spec(&cr.spec).ok())
            .unwrap_or_default()
    }

    async fn dispatch(
        &self,
        resource_store: &ResourceStore,
        provider_name: &str,
        op: StoreOp,
    ) -> Result<StoreOpResult> {
        let provider = self.resolve_provider(resource_store, provider_name)?;

        if provider.builtin {
            match provider_name {
                "local" => self.local_backend.execute(op).await,
                "file" => self.file_backend.execute(op).await,
                _ => Err(anyhow!("unknown builtin provider: {}", provider_name)),
            }
        } else {
            let commands = provider.commands.as_ref().ok_or_else(|| {
                anyhow!("provider '{}' has no commands defined", provider_name)
            })?;
            self.command_adapter.execute(commands, op).await
        }
    }

    fn resolve_provider(
        &self,
        resource_store: &ResourceStore,
        provider_name: &str,
    ) -> Result<StoreBackendProviderConfig> {
        // Built-in providers don't need a CRD instance
        match provider_name {
            "local" | "file" => {
                return Ok(StoreBackendProviderConfig {
                    builtin: true,
                    commands: None,
                });
            }
            _ => {}
        }

        // Look up user-defined provider from ResourceStore
        resource_store
            .get("StoreBackendProvider", provider_name)
            .and_then(|cr| StoreBackendProviderConfig::from_cr_spec(&cr.spec).ok())
            .ok_or_else(|| anyhow!("store backend provider '{}' not found", provider_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_op_debug() {
        let op = StoreOp::Get {
            store_name: "metrics".to_string(),
            project_id: "proj1".to_string(),
            key: "k1".to_string(),
        };
        let debug = format!("{:?}", op);
        assert!(debug.contains("Get"));
        assert!(debug.contains("metrics"));
    }

    #[test]
    fn store_entry_serde_round_trip() {
        let entry = StoreEntry {
            key: "bench_001".to_string(),
            value: serde_json::json!({"test_count": 42}),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: StoreEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.key, "bench_001");
    }
}
