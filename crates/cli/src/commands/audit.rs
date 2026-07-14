use anyhow::Result;
use orchestrator_proto::{
    ActionAuditGetRequest, ActionAuditListRequest, ActionAuditRecord, OrchestratorServiceClient,
};
use tonic::transport::Channel;

use crate::{AuditCommands, OutputFormat};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: AuditCommands,
) -> Result<()> {
    match command {
        AuditCommands::List {
            project,
            actor,
            target_type,
            target_id,
            action,
            status,
            from,
            to,
            limit,
            output,
        } => {
            let response = client
                .action_audit_list(ActionAuditListRequest {
                    project_id: project,
                    actor,
                    target_type,
                    target_id,
                    action,
                    status,
                    from_time: from,
                    to_time: to,
                    limit,
                })
                .await?
                .into_inner();
            print_records(&response.records, output)?;
        }
        AuditCommands::Get {
            request_id,
            project,
            output,
        } => {
            let record = client
                .action_audit_get(ActionAuditGetRequest {
                    project_id: project,
                    request_id,
                })
                .await?
                .into_inner();
            print_records(std::slice::from_ref(&record), output)?;
        }
    }
    Ok(())
}

fn print_records(records: &[ActionAuditRecord], output: OutputFormat) -> Result<()> {
    let values = records.iter().map(record_value).collect::<Vec<_>>();
    match output {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&values)?),
        OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&values)?),
        OutputFormat::Table => {
            if records.is_empty() {
                println!("No action audit records found.");
                return Ok(());
            }
            println!(
                "{:<38} {:<22} {:<18} {:<11} CREATED",
                "REQUEST ID", "ACTION", "TARGET", "STATUS"
            );
            for record in records {
                let target = format!("{}:{}", record.target_type, record.target_id);
                println!(
                    "{:<38} {:<22} {:<18} {:<11} {}",
                    truncate(&record.request_id, 36),
                    truncate(&record.action, 20),
                    truncate(&target, 16),
                    record.status,
                    record.created_at
                );
            }
            println!("\n{} record(s)", records.len());
        }
    }
    Ok(())
}

fn record_value(record: &ActionAuditRecord) -> serde_json::Value {
    serde_json::json!({
        "request_id": record.request_id,
        "schema_version": record.schema_version,
        "project_id": record.project_id,
        "actor": record.actor,
        "resolved_role": record.resolved_role,
        "transport": record.transport,
        "target_type": record.target_type,
        "target_id": record.target_id,
        "action": record.action,
        "reason_code": record.reason_code,
        "operator_reason": record.operator_reason,
        "idempotency_key": record.idempotency_key,
        "expected_version": record.expected_version,
        "fencing_token": record.fencing_token,
        "request_hash": record.request_hash,
        "status": record.status,
        "error_code": record.error_code,
        "result_type": record.result_type,
        "result_id": record.result_id,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "completed_at": record.completed_at,
    })
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>()
        + "…"
}
