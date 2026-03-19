use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::output;
use crate::TaskCommands;

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: TaskCommands,
) -> Result<()> {
    match cmd {
        TaskCommands::List {
            status,
            project,
            output,
            verbose: _,
        } => {
            let resp = client
                .task_list(orchestrator_proto::TaskListRequest {
                    status_filter: status,
                    project_filter: project,
                })
                .await?
                .into_inner();
            output::print_task_list(&resp.tasks, output);
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
        } => {
            let resp = client
                .task_create(orchestrator_proto::TaskCreateRequest {
                    name,
                    goal,
                    project_id: project,
                    workspace_id: workspace,
                    workflow_id: workflow,
                    target_files: target_file,
                    no_start,
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
            output::print_task_detail(&resp, output);
            Ok(())
        }

        TaskCommands::Start { task_id, latest } => {
            let resp = client
                .task_start(orchestrator_proto::TaskStartRequest { task_id, latest })
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

        TaskCommands::Resume {
            task_id,
            reset_blocked,
        } => {
            let resp = client
                .task_resume(orchestrator_proto::TaskResumeRequest {
                    task_id,
                    reset_blocked,
                })
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

        TaskCommands::Delete {
            task_ids,
            all,
            status,
            project,
            force,
        } => {
            if task_ids.len() == 1 && !all {
                // Single-task delete — use original RPC for backward compat
                let task_id = task_ids[0].clone();
                let resp = client
                    .task_delete(orchestrator_proto::TaskDeleteRequest { task_id, force })
                    .await?
                    .into_inner();
                println!("{}", resp.message);
            } else {
                // Bulk delete — explicit IDs or --all with optional filters
                let resp = client
                    .task_delete_bulk(orchestrator_proto::TaskDeleteBulkRequest {
                        task_ids,
                        force,
                        status_filter: status.unwrap_or_default(),
                        project_filter: project.unwrap_or_default(),
                    })
                    .await?
                    .into_inner();
                println!("{}", resp.message);
                for err in &resp.errors {
                    eprintln!("  error: {}", err);
                }
            }
            Ok(())
        }

        TaskCommands::Retry {
            task_item_id,
            force,
        } => {
            let resp = client
                .task_retry(orchestrator_proto::TaskRetryRequest {
                    task_item_id,
                    force,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Recover { task_id } => {
            let resp = client
                .task_recover(orchestrator_proto::TaskRecoverRequest { task_id })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        TaskCommands::Watch {
            task_id,
            interval,
            timeout,
        } => {
            let mut stream = client
                .task_watch(orchestrator_proto::TaskWatchRequest {
                    task_id,
                    interval_secs: interval,
                    timeout_secs: timeout,
                })
                .await?
                .into_inner();

            let deadline = if timeout > 0 {
                Some(tokio::time::Instant::now() + std::time::Duration::from_secs(timeout))
            } else {
                None
            };

            use tokio_stream::StreamExt;
            loop {
                let next = if let Some(dl) = deadline {
                    let remaining = dl.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        eprintln!("watch: timeout after {}s", timeout);
                        break;
                    }
                    match tokio::time::timeout(remaining, stream.next()).await {
                        Ok(item) => item,
                        Err(_) => {
                            eprintln!("watch: timeout after {}s", timeout);
                            break;
                        }
                    }
                } else {
                    stream.next().await
                };

                match next {
                    Some(Ok(s)) => {
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
                    Some(Err(e)) => {
                        eprintln!("watch error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            Ok(())
        }

        TaskCommands::Trace {
            task_id,
            verbose,
            json,
        } => {
            let resp = client
                .task_trace(orchestrator_proto::TaskTraceRequest { task_id, verbose })
                .await?
                .into_inner();

            if json {
                println!("{}", resp.trace_json);
                return Ok(());
            }

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
                println!("{} {}{}{}", entry.timestamp, entry.event_type, step, item);
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
