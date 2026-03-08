pub mod daemon;
pub mod version;

use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::{Commands, OutputFormat};

pub async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: Commands,
) -> Result<()> {
    match command {
        Commands::Apply {
            file,
            dry_run,
            project,
        } => {
            let content = if file == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            } else {
                std::fs::read_to_string(&file).map_err(|e| {
                    anyhow::anyhow!("failed to read manifest file '{}': {}", file, e)
                })?
            };

            let resp = client
                .apply(orchestrator_proto::ApplyRequest {
                    content,
                    dry_run,
                    project,
                })
                .await?
                .into_inner();

            for entry in &resp.results {
                let scope = entry
                    .project_scope
                    .as_ref()
                    .map(|p| format!(" (project: {})", p))
                    .unwrap_or_default();
                if dry_run {
                    println!(
                        "{}/{} would be {} (dry run){}",
                        entry.kind, entry.name, entry.action, scope
                    );
                } else {
                    println!("{}/{} {}{}", entry.kind, entry.name, entry.action, scope);
                }
            }
            if let Some(version) = resp.config_version {
                println!("configuration version: {}", version);
            }
            for err in &resp.errors {
                eprintln!("Error: {}", err);
            }
            if !resp.errors.is_empty() {
                std::process::exit(1);
            }
            Ok(())
        }

        Commands::Get {
            resource,
            output,
            selector,
        } => {
            let resp = client
                .get(orchestrator_proto::GetRequest {
                    resource,
                    selector,
                    output_format: format_to_string(output),
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(())
        }

        Commands::Describe { resource, output } => {
            let resp = client
                .describe(orchestrator_proto::DescribeRequest {
                    resource,
                    output_format: format_to_string(output),
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(())
        }

        Commands::Delete { resource, force } => {
            let resp = client
                .delete(orchestrator_proto::DeleteRequest { resource, force })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        Commands::Task(cmd) => dispatch_task(client, cmd).await,
        Commands::Store(cmd) => dispatch_store(client, cmd).await,

        Commands::Debug { component } => {
            let resp = client
                .config_debug(orchestrator_proto::ConfigDebugRequest { component })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(())
        }

        Commands::Check { workflow, output } => {
            let resp = client
                .check(orchestrator_proto::CheckRequest {
                    workflow,
                    output_format: format_to_string(output),
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            std::process::exit(resp.exit_code);
        }

        Commands::Init { root } => {
            let resp = client
                .init(orchestrator_proto::InitRequest { root })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        Commands::Db(cmd) => {
            use crate::DbCommands;
            match cmd {
                DbCommands::Reset {
                    force,
                    include_history,
                    include_config,
                } => {
                    let resp = client
                        .db_reset(orchestrator_proto::DbResetRequest {
                            force,
                            include_history,
                            include_config,
                        })
                        .await?
                        .into_inner();
                    println!("{}", resp.message);
                    Ok(())
                }
            }
        }

        Commands::Manifest(cmd) => {
            use crate::ManifestCommands;
            match cmd {
                ManifestCommands::Validate { file } => {
                    let content = if file == "-" {
                        use std::io::Read;
                        let mut buf = String::new();
                        std::io::stdin().read_to_string(&mut buf)?;
                        buf
                    } else {
                        std::fs::read_to_string(&file).map_err(|e| {
                            anyhow::anyhow!("failed to read manifest file '{}': {}", file, e)
                        })?
                    };

                    let resp = client
                        .manifest_validate(orchestrator_proto::ManifestValidateRequest { content })
                        .await?
                        .into_inner();
                    println!("{}", resp.message);
                    for err in &resp.errors {
                        eprintln!("  {}", err);
                    }
                    if !resp.valid {
                        std::process::exit(1);
                    }
                    Ok(())
                }
            }
        }

        // These are handled before dispatch
        Commands::Version | Commands::Daemon(_) => unreachable!(),
    }
}

async fn dispatch_task(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: crate::TaskCommands,
) -> Result<()> {
    use crate::TaskCommands;

    match cmd {
        TaskCommands::List {
            status,
            output,
            verbose: _,
        } => {
            let resp = client
                .task_list(orchestrator_proto::TaskListRequest {
                    status_filter: status,
                })
                .await?
                .into_inner();
            crate::output::print_task_list(&resp.tasks, output);
            Ok(())
        }

        TaskCommands::Create {
            name,
            goal,
            project,
            workspace,
            workflow,
            target_file,
            no_start,
            detach,
            attach,
        } => {
            let should_detach = detach && !attach;
            let resp = client
                .task_create(orchestrator_proto::TaskCreateRequest {
                    name,
                    goal,
                    project_id: project,
                    workspace_id: workspace,
                    workflow_id: workflow,
                    target_files: target_file,
                    no_start,
                    detach: should_detach,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Info { task_id, output } => {
            let resp = client
                .task_info(orchestrator_proto::TaskInfoRequest { task_id })
                .await?
                .into_inner();
            crate::output::print_task_detail(&resp, output);
            Ok(())
        }

        TaskCommands::Start {
            task_id,
            latest,
            detach,
        } => {
            let resp = client
                .task_start(orchestrator_proto::TaskStartRequest {
                    task_id,
                    latest,
                    detach,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Pause { task_id } => {
            let resp = client
                .task_pause(orchestrator_proto::TaskPauseRequest { task_id })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Resume { task_id, detach } => {
            let resp = client
                .task_resume(orchestrator_proto::TaskResumeRequest { task_id, detach })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Logs {
            task_id,
            follow,
            tail,
            timestamps,
        } => {
            let mut stream = client
                .task_logs(orchestrator_proto::TaskLogsRequest {
                    task_id: task_id.clone(),
                    tail: tail as u64,
                    timestamps,
                })
                .await?
                .into_inner();

            use tokio_stream::StreamExt;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(c) => println!("{}", c.content),
                    Err(e) => {
                        eprintln!("stream error: {}", e);
                        break;
                    }
                }
            }

            if follow {
                let mut follow_stream = client
                    .task_follow(orchestrator_proto::TaskFollowRequest { task_id })
                    .await?
                    .into_inner();

                while let Some(line) = follow_stream.next().await {
                    match line {
                        Ok(l) => println!("{}", l.line),
                        Err(e) => {
                            eprintln!("follow error: {}", e);
                            break;
                        }
                    }
                }
            }
            Ok(())
        }

        TaskCommands::Delete { task_id, force } => {
            let resp = client
                .task_delete(orchestrator_proto::TaskDeleteRequest { task_id, force })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Retry {
            task_item_id,
            detach,
            force,
        } => {
            let resp = client
                .task_retry(orchestrator_proto::TaskRetryRequest {
                    task_item_id,
                    detach,
                    force,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Watch { task_id, interval } => {
            let mut stream = client
                .task_watch(orchestrator_proto::TaskWatchRequest {
                    task_id,
                    interval_secs: interval,
                })
                .await?
                .into_inner();

            use tokio_stream::StreamExt;
            while let Some(snapshot) = stream.next().await {
                match snapshot {
                    Ok(s) => {
                        if let Some(task) = &s.task {
                            println!(
                                "[{}] status={} items={}/{} failed={}",
                                task.id,
                                task.status,
                                task.finished_items,
                                task.total_items,
                                task.failed_items
                            );
                            for item in &s.items {
                                println!(
                                    "  item {} status={}",
                                    &item.id[..8.min(item.id.len())],
                                    item.status
                                );
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("watch error: {}", e);
                        break;
                    }
                }
            }
            Ok(())
        }

        TaskCommands::Trace { task_id, verbose } => {
            let resp = client
                .task_trace(orchestrator_proto::TaskTraceRequest { task_id, verbose })
                .await?
                .into_inner();

            println!("TRACE TIMELINE ({} events)", resp.entries.len());
            println!("{:-<70}", "");
            for entry in &resp.entries {
                let item = entry
                    .item_id
                    .as_deref()
                    .map(|id| format!(" item={}", &id[..8.min(id.len())]))
                    .unwrap_or_default();
                let step = if entry.step.is_empty() {
                    String::new()
                } else {
                    format!(" step={}", entry.step)
                };
                println!(
                    "{} {}{}{}",
                    entry.timestamp, entry.event_type, step, item
                );
            }

            if !resp.anomalies.is_empty() {
                println!("\nANOMALIES ({} detected)", resp.anomalies.len());
                println!("{:-<70}", "");
                for anomaly in &resp.anomalies {
                    let at = anomaly
                        .at
                        .as_deref()
                        .map(|t| format!(" at {}", t))
                        .unwrap_or_default();
                    println!(
                        "[{}] {}: {}{}",
                        anomaly.severity.to_uppercase(),
                        anomaly.rule,
                        anomaly.message,
                        at
                    );
                }
            }
            Ok(())
        }
    }
}

async fn dispatch_store(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: crate::StoreCommands,
) -> Result<()> {
    use crate::StoreCommands;

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

fn format_to_string(f: OutputFormat) -> String {
    match f {
        OutputFormat::Table => "table".to_string(),
        OutputFormat::Json => "json".to_string(),
        OutputFormat::Yaml => "yaml".to_string(),
    }
}
