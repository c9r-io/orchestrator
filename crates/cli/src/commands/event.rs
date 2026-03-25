use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use crate::output;
use crate::EventCommands;

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: EventCommands,
) -> Result<()> {
    match cmd {
        EventCommands::Cleanup {
            older_than_days,
            dry_run,
            archive,
        } => {
            let resp = client
                .event_cleanup(orchestrator_proto::EventCleanupRequest {
                    older_than_days,
                    dry_run,
                    archive,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
        EventCommands::List {
            task,
            event_type,
            limit,
            output,
        } => {
            let resp = client
                .task_events(orchestrator_proto::TaskEventsRequest {
                    task_id: task,
                    event_type_filter: event_type.unwrap_or_default(),
                    limit,
                })
                .await?
                .into_inner();
            output::print_event_list(&resp.events, output);
            Ok(())
        }
        EventCommands::Stats => {
            let resp = client
                .event_stats(orchestrator_proto::EventStatsRequest {})
                .await?
                .into_inner();
            println!("Total events:  {}", resp.total_rows);
            if !resp.earliest.is_empty() {
                println!("Earliest:      {}", resp.earliest);
            }
            if !resp.latest.is_empty() {
                println!("Latest:        {}", resp.latest);
            }
            if !resp.by_task_status.is_empty() {
                println!("\nBy task status:");
                for entry in &resp.by_task_status {
                    println!("  {:<15} {}", entry.status, entry.count);
                }
            }
            Ok(())
        }
    }
}
