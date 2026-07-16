use anyhow::Result;
use orchestrator_proto::{
    ActionAuditContext, OrchestratorServiceClient, SourceBindRequest, SourceBinding,
    SourceBindingListRequest, SourceEvent, SourceEventGetRequest, SourceEventIngestRequest,
    SourceEventListRequest, SourceReplayRequest,
};
use sha2::{Digest, Sha256};
use tonic::transport::Channel;

use crate::{OutputFormat, SourceCommands};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: SourceCommands,
) -> Result<()> {
    match command {
        SourceCommands::List {
            project,
            task,
            state,
            limit,
            output,
        } => {
            let response = client
                .source_event_list(SourceEventListRequest {
                    project_id: project,
                    task_id: task,
                    routing_state: state,
                    limit,
                })
                .await?
                .into_inner();
            print_events(&response.events, output)?;
        }
        SourceCommands::Get { id, output } => {
            let event = client
                .source_event_get(SourceEventGetRequest { id })
                .await?
                .into_inner();
            print_value(event_value(&event), output)?;
        }
        SourceCommands::Ingest {
            project,
            file,
            payload_hash,
        } => {
            let normalized_json = super::common::read_input_or_file(&file)?;
            let payload_hash = payload_hash.unwrap_or_else(|| {
                Sha256::digest(normalized_json.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            });
            let response = client
                .source_event_ingest(SourceEventIngestRequest {
                    project_id: project,
                    normalized_json,
                    payload_hash,
                    audit: Some(audit_context("operator_source_ingest", "ingest")),
                })
                .await?
                .into_inner();
            let event = response
                .event
                .as_ref()
                .map(event_value)
                .unwrap_or(serde_json::Value::Null);
            print_value(
                serde_json::json!({"inserted": response.inserted, "event": event}),
                OutputFormat::Yaml,
            )?;
        }
        SourceCommands::Bindings { task_id, output } => {
            let response = client
                .source_binding_list(SourceBindingListRequest { task_id })
                .await?
                .into_inner();
            print_bindings(&response.bindings, output)?;
        }
        SourceCommands::Bind {
            project,
            task,
            provider,
            installation,
            conversation,
            thread,
            binding_type,
            source_event,
        } => {
            let binding = client
                .source_bind(SourceBindRequest {
                    project_id: project,
                    task_id: task,
                    provider,
                    installation_id: installation,
                    conversation_id: conversation,
                    thread_id: thread,
                    binding_type,
                    created_by_event_id: source_event,
                    audit: Some(audit_context("operator_source_bind", "bind")),
                })
                .await?
                .into_inner();
            print_value(binding_value(&binding), OutputFormat::Yaml)?;
        }
        SourceCommands::Replay { id } => {
            let response = client
                .source_replay(SourceReplayRequest {
                    id,
                    audit: Some(audit_context("operator_source_replay", "replay")),
                })
                .await?
                .into_inner();
            println!("{}\t{}", response.id, response.status);
        }
    }
    Ok(())
}

fn audit_context(reason_code: &str, prefix: &str) -> ActionAuditContext {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    ActionAuditContext {
        reason_code: reason_code.to_string(),
        operator_reason: None,
        idempotency_key: Some(format!("cli-source-{prefix}-{nonce}")),
    }
}

fn print_events(events: &[SourceEvent], output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        println!("ID\tPROVIDER\tTYPE\tINSTALLATION\tSTATE\tTASK\tRECEIVED");
        for event in events {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                event.id,
                event.provider,
                event.event_type,
                event.installation_id,
                event.routing_state,
                event.routed_task_id.as_deref().unwrap_or("-"),
                event.received_at
            );
        }
        return Ok(());
    }
    print_value(
        serde_json::Value::Array(events.iter().map(event_value).collect()),
        output,
    )
}

fn print_bindings(bindings: &[SourceBinding], output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        println!("ID\tPROVIDER\tINSTALLATION\tCONVERSATION\tTHREAD\tTYPE\tTASK");
        for binding in bindings {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                binding.id,
                binding.provider,
                binding.installation_id,
                binding.conversation_id.as_deref().unwrap_or("-"),
                binding.thread_id.as_deref().unwrap_or("-"),
                binding.binding_type,
                binding.task_id
            );
        }
        return Ok(());
    }
    print_value(
        serde_json::Value::Array(bindings.iter().map(binding_value).collect()),
        output,
    )
}

fn print_value(value: serde_json::Value, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
        OutputFormat::Yaml | OutputFormat::Table => print!("{}", serde_yaml::to_string(&value)?),
    }
    Ok(())
}

fn event_value(event: &SourceEvent) -> serde_json::Value {
    serde_json::json!({
        "id": event.id,
        "project_id": event.project_id,
        "provider": event.provider,
        "installation_id": event.installation_id,
        "external_event_id": event.external_event_id,
        "event_type": event.event_type,
        "external_actor_id": event.external_actor_id,
        "conversation_id": event.conversation_id,
        "thread_id": event.thread_id,
        "occurred_at": event.occurred_at,
        "received_at": event.received_at,
        "normalized": serde_json::from_str::<serde_json::Value>(&event.normalized_json)
            .unwrap_or(serde_json::Value::Null),
        "payload_hash": event.payload_hash,
        "routing_state": event.routing_state,
        "routing_attempts": event.routing_attempts,
        "routed_task_id": event.routed_task_id,
        "last_error_code": event.last_error_code,
    })
}

fn binding_value(binding: &SourceBinding) -> serde_json::Value {
    serde_json::json!({
        "id": binding.id,
        "project_id": binding.project_id,
        "task_id": binding.task_id,
        "provider": binding.provider,
        "installation_id": binding.installation_id,
        "conversation_id": binding.conversation_id,
        "thread_id": binding.thread_id,
        "binding_type": binding.binding_type,
        "created_by_event_id": binding.created_by_event_id,
        "created_at": binding.created_at,
    })
}
