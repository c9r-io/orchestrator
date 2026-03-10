use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::{OutputFormat, StoreCommands};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: StoreCommands,
) -> Result<()> {
    match cmd {
        StoreCommands::Get {
            store,
            key,
            project,
        } => {
            let resp = client
                .store_get(orchestrator_proto::StoreGetRequest {
                    store,
                    key: key.clone(),
                    project,
                })
                .await?
                .into_inner();
            if resp.found {
                println!("{}", resp.value_json.unwrap_or_default());
            } else {
                eprintln!("key '{}' not found", key);
                std::process::exit(1);
            }
            Ok(())
        }

        StoreCommands::Put {
            store,
            key,
            value,
            project,
            task_id,
        } => {
            let resp = client
                .store_put(orchestrator_proto::StorePutRequest {
                    store,
                    key,
                    value_json: value,
                    project,
                    task_id,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        StoreCommands::Delete {
            store,
            key,
            project,
        } => {
            let resp = client
                .store_delete(orchestrator_proto::StoreDeleteRequest {
                    store,
                    key,
                    project,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        StoreCommands::List {
            store,
            project,
            limit,
            offset,
            output,
        } => {
            let resp = client
                .store_list(orchestrator_proto::StoreListRequest {
                    store,
                    project,
                    limit,
                    offset,
                })
                .await?
                .into_inner();

            match output {
                OutputFormat::Json => {
                    let entries: Vec<serde_json::Value> = resp
                        .entries
                        .iter()
                        .map(|e| {
                            serde_json::json!({
                                "key": e.key,
                                "value": e.value_json,
                                "updated_at": e.updated_at,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                }
                OutputFormat::Yaml => {
                    let entries: Vec<serde_json::Value> = resp
                        .entries
                        .iter()
                        .map(|e| {
                            serde_json::json!({
                                "key": e.key,
                                "value": e.value_json,
                                "updated_at": e.updated_at,
                            })
                        })
                        .collect();
                    println!("{}", serde_yml::to_string(&entries)?);
                }
                OutputFormat::Table => {
                    if resp.entries.is_empty() {
                        println!("no entries");
                    } else {
                        println!("{:<30} {:<40} VALUE", "KEY", "UPDATED_AT");
                        for e in &resp.entries {
                            let val = if e.value_json.len() > 60 {
                                format!("{}...", &e.value_json[..57])
                            } else {
                                e.value_json.clone()
                            };
                            println!("{:<30} {:<40} {}", e.key, e.updated_at, val);
                        }
                    }
                }
            }
            Ok(())
        }

        StoreCommands::Prune { store, project } => {
            let resp = client
                .store_prune(orchestrator_proto::StorePruneRequest { store, project })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
    }
}
