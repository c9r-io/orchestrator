use anyhow::Result;
use orchestrator_proto::{
    ActionAuditContext, AttentionClaimRequest, AttentionExecuteActionRequest,
    AttentionFollowRequest, AttentionGetRequest, AttentionListRequest, AttentionResolveRequest,
    AttentionSnoozeRequest, OrchestratorServiceClient,
};
use tokio_stream::StreamExt;
use tonic::transport::Channel;

use crate::{AttentionCommands, output};

fn idempotency_key(provided: Option<String>) -> String {
    provided.unwrap_or_else(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        format!("cli-{}-{nanos}", std::process::id())
    })
}

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: AttentionCommands,
) -> Result<()> {
    match command {
        AttentionCommands::List {
            project,
            state,
            kind,
            severity,
            assignee,
            task,
            limit,
            output: format,
        } => {
            let response = client
                .attention_list(AttentionListRequest {
                    project_id: project,
                    state,
                    kind,
                    severity,
                    assignee,
                    task_id: task,
                    limit,
                    active_only: false,
                })
                .await?
                .into_inner();
            output::print_attention_list(&response, format)?;
        }
        AttentionCommands::Get { id, output: format } => {
            let item = client
                .attention_get(AttentionGetRequest { id })
                .await?
                .into_inner();
            output::print_attention_item(&item, format)?;
        }
        AttentionCommands::Claim {
            id,
            expected_version,
            idempotency_key: key,
        } => {
            let key = idempotency_key(key);
            let item = client
                .attention_claim(AttentionClaimRequest {
                    id,
                    expected_version,
                    idempotency_key: key.clone(),
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_triage".into(),
                        operator_reason: None,
                        idempotency_key: Some(key),
                    }),
                })
                .await?
                .into_inner();
            output::print_attention_item(&item, crate::OutputFormat::Yaml)?;
        }
        AttentionCommands::Snooze {
            id,
            expected_version,
            until,
            idempotency_key: key,
        } => {
            let key = idempotency_key(key);
            let item = client
                .attention_snooze(AttentionSnoozeRequest {
                    id,
                    expected_version,
                    idempotency_key: key.clone(),
                    until,
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_snooze".into(),
                        operator_reason: None,
                        idempotency_key: Some(key),
                    }),
                })
                .await?
                .into_inner();
            output::print_attention_item(&item, crate::OutputFormat::Yaml)?;
        }
        AttentionCommands::Resolve {
            id,
            expected_version,
            reason,
            idempotency_key: key,
        } => {
            let key = idempotency_key(key);
            let item = client
                .attention_resolve(AttentionResolveRequest {
                    id,
                    expected_version,
                    idempotency_key: key.clone(),
                    reason: reason.clone(),
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_resolve".into(),
                        operator_reason: Some(reason),
                        idempotency_key: Some(key),
                    }),
                })
                .await?
                .into_inner();
            output::print_attention_item(&item, crate::OutputFormat::Yaml)?;
        }
        AttentionCommands::Action {
            id,
            action_id,
            expected_version,
            input,
            idempotency_key: key,
        } => {
            let key = idempotency_key(key);
            let item = client
                .attention_execute_action(AttentionExecuteActionRequest {
                    id,
                    expected_version,
                    idempotency_key: key.clone(),
                    action_id,
                    input_json: input,
                    audit: Some(ActionAuditContext {
                        reason_code: "operator_action".into(),
                        operator_reason: None,
                        idempotency_key: Some(key),
                    }),
                })
                .await?
                .into_inner();
            output::print_attention_item(&item, crate::OutputFormat::Yaml)?;
        }
        AttentionCommands::Follow {
            after,
            project,
            output: format,
        } => {
            let mut stream = client
                .attention_follow(AttentionFollowRequest {
                    after_change_id: after,
                    project_id: project,
                    interval_millis: 500,
                    state: None,
                    kind: None,
                    severity: None,
                    assignee: None,
                    task_id: None,
                    active_only: false,
                })
                .await?
                .into_inner();
            while let Some(delta) = stream.next().await {
                output::print_attention_delta(&delta?, format)?;
            }
        }
    }
    Ok(())
}
