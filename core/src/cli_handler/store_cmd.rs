use crate::cli::StoreCommands;
use crate::cli_handler::CliHandler;
use crate::store::{StoreOp, StoreOpResult};
use agent_orchestrator::crd::projection::CrdProjectable as _;
use anyhow::Result;

impl CliHandler {
    pub(super) fn handle_store(&self, cmd: &StoreCommands) -> Result<i32> {
        let rt = super::cli_runtime();

        match cmd {
            StoreCommands::Get {
                store,
                key,
                project,
            } => {
                let op = StoreOp::Get {
                    store_name: store.clone(),
                    project_id: project.clone(),
                    key: key.clone(),
                };
                let result = rt.block_on(self.execute_store_op(op))?;
                match result {
                    StoreOpResult::Value(Some(v)) => {
                        println!("{}", serde_json::to_string_pretty(&v)?);
                        Ok(0)
                    }
                    StoreOpResult::Value(None) => {
                        eprintln!("key '{}' not found in store '{}'", key, store);
                        Ok(1)
                    }
                    _ => Ok(0),
                }
            }
            StoreCommands::Put {
                store,
                key,
                value,
                project,
                task_id,
            } => {
                let op = StoreOp::Put {
                    store_name: store.clone(),
                    project_id: project.clone(),
                    key: key.clone(),
                    value: value.clone(),
                    task_id: task_id.clone(),
                };
                rt.block_on(self.execute_store_op(op))?;
                println!("stored key '{}' in '{}'", key, store);
                Ok(0)
            }
            StoreCommands::Delete {
                store,
                key,
                project,
            } => {
                let op = StoreOp::Delete {
                    store_name: store.clone(),
                    project_id: project.clone(),
                    key: key.clone(),
                };
                rt.block_on(self.execute_store_op(op))?;
                println!("deleted key '{}' from '{}'", key, store);
                Ok(0)
            }
            StoreCommands::List {
                store,
                project,
                limit,
                offset,
                output,
            } => {
                let op = StoreOp::List {
                    store_name: store.clone(),
                    project_id: project.clone(),
                    limit: *limit,
                    offset: *offset,
                };
                let result = rt.block_on(self.execute_store_op(op))?;
                match result {
                    StoreOpResult::Entries(entries) => {
                        if entries.is_empty() {
                            println!("no entries in store '{}'", store);
                        } else {
                            match output {
                                crate::cli::OutputFormat::Json => {
                                    println!("{}", serde_json::to_string_pretty(&entries)?);
                                }
                                crate::cli::OutputFormat::Yaml => {
                                    println!("{}", serde_yml::to_string(&entries)?);
                                }
                                _ => {
                                    println!("{:<30} {:<40} VALUE", "KEY", "UPDATED_AT");
                                    for entry in &entries {
                                        let val_str = serde_json::to_string(&entry.value)?;
                                        let truncated = if val_str.len() > 60 {
                                            format!("{}...", &val_str[..57])
                                        } else {
                                            val_str
                                        };
                                        println!(
                                            "{:<30} {:<40} {}",
                                            entry.key, entry.updated_at, truncated
                                        );
                                    }
                                }
                            }
                        }
                        Ok(0)
                    }
                    _ => Ok(0),
                }
            }
            StoreCommands::Prune { store, project } => {
                // Read retention config from the store's WorkflowStore CRD
                let config = self
                    .state
                    .active_config
                    .read()
                    .map_err(|_| anyhow::anyhow!("failed to read active config"))?;
                let custom_resources = &config.config.custom_resources;

                let store_config = {
                    let key = format!("WorkflowStore/{}", store);
                    custom_resources
                        .get(&key)
                        .and_then(|cr| {
                            crate::config::WorkflowStoreConfig::from_cr_spec(&cr.spec).ok()
                        })
                        .unwrap_or_default()
                };

                let op = StoreOp::Prune {
                    store_name: store.clone(),
                    project_id: project.clone(),
                    max_entries: store_config.retention.max_entries,
                    ttl_days: store_config.retention.ttl_days,
                };
                drop(config);
                rt.block_on(self.execute_store_op(op))?;
                println!("pruned store '{}'", store);
                Ok(0)
            }
        }
    }

    async fn execute_store_op(&self, op: StoreOp) -> Result<StoreOpResult> {
        let custom_resources = {
            let config = self
                .state
                .active_config
                .read()
                .map_err(|_| anyhow::anyhow!("failed to read active config"))?;
            config.config.custom_resources.clone()
        };
        self.state
            .store_manager
            .execute(&custom_resources, op)
            .await
    }
}
