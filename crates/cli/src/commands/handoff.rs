use anyhow::Result;
use orchestrator_proto::{
    ActionAuditContext, HandoffGenerateRequest, HandoffGetRequest, HandoffSnapshotResponse,
    OrchestratorServiceClient, ResumeBoundary, ResumeBoundaryListRequest, ResumeExecuteRequest,
    ResumePlanRequest,
};
use tonic::transport::Channel;

use crate::{HandoffCommands, OutputFormat, ResumeCommands};

pub async fn dispatch_handoff(
    client: &mut OrchestratorServiceClient<Channel>,
    command: HandoffCommands,
) -> Result<()> {
    let (snapshot, format) = match command {
        HandoffCommands::Generate {
            task_id,
            cursor,
            output,
        } => (
            client
                .handoff_generate(HandoffGenerateRequest {
                    task_id,
                    source_event_cursor: cursor,
                    audit: Some(audit_context(
                        "operator_handoff",
                        None,
                        generated_key("handoff"),
                    )),
                })
                .await?
                .into_inner(),
            output,
        ),
        HandoffCommands::Get { id, output } => (
            client
                .handoff_get(HandoffGetRequest { id })
                .await?
                .into_inner(),
            output,
        ),
    };
    print_value(snapshot_value(&snapshot), format);
    Ok(())
}

pub async fn dispatch_resume(
    client: &mut OrchestratorServiceClient<Channel>,
    command: ResumeCommands,
) -> Result<()> {
    match command {
        ResumeCommands::Boundaries { task_id, output } => {
            let response = client
                .resume_boundary_list(ResumeBoundaryListRequest { task_id })
                .await?
                .into_inner();
            if output == OutputFormat::Table {
                println!(
                    "{:<18} {:<28} {:<26} SAFE",
                    "BOUNDARY", "STEP", "SIDE EFFECT"
                );
                for boundary in response.boundaries {
                    println!(
                        "{:<18} {:<28} {:<26} {}",
                        short(&boundary.id),
                        boundary.step_id.as_deref().unwrap_or("current"),
                        boundary.side_effect_class,
                        boundary.replay_safe
                    );
                }
            } else {
                print_value(
                    serde_json::Value::Array(
                        response.boundaries.iter().map(boundary_value).collect(),
                    ),
                    output,
                );
            }
        }
        ResumeCommands::Plan {
            task_id,
            boundary,
            mode,
            attention_item,
            output,
        } => {
            let plan = client
                .resume_plan(ResumePlanRequest {
                    task_id,
                    boundary_id: boundary,
                    mode,
                    attention_item_id: attention_item,
                    audit: Some(audit_context(
                        "operator_resume_plan",
                        None,
                        generated_key("resume-plan"),
                    )),
                })
                .await?
                .into_inner();
            print_value(
                serde_json::json!({
                    "id": plan.id,
                    "task_id": plan.task_id,
                    "boundary": plan.boundary.as_ref().map(boundary_value),
                    "mode": plan.mode,
                    "expected_state_version": plan.expected_state_version,
                    "consequence": serde_json::from_str::<serde_json::Value>(&plan.consequence_json).unwrap_or_default(),
                    "elevated_confirmation_required": plan.elevated_confirmation_required,
                    "expires_at": plan.expires_at,
                    "status": plan.status,
                }),
                output,
            );
        }
        ResumeCommands::Execute {
            plan_id,
            expected_state_version,
            reason,
            idempotency_key,
            elevated_confirmation,
            output,
        } => {
            let execution = client
                .resume_execute(ResumeExecuteRequest {
                    plan_id,
                    expected_state_version,
                    operator_reason: reason.clone(),
                    idempotency_key: idempotency_key.clone(),
                    elevated_confirmation,
                    audit: Some(audit_context(
                        "operator_resume_execute",
                        Some(reason),
                        idempotency_key,
                    )),
                })
                .await?
                .into_inner();
            print_value(
                serde_json::json!({
                    "execution_id": execution.execution_id,
                    "plan_id": execution.plan_id,
                    "accepted": execution.accepted,
                    "status": execution.status,
                    "child_task_id": execution.child_task_id,
                }),
                output,
            );
        }
    }
    Ok(())
}

fn snapshot_value(snapshot: &HandoffSnapshotResponse) -> serde_json::Value {
    serde_json::json!({
        "id": snapshot.id,
        "task_id": snapshot.task_id,
        "source_event_cursor": snapshot.source_event_cursor,
        "projection_version": snapshot.projection_version,
        "briefing": serde_json::from_str::<serde_json::Value>(&snapshot.briefing_json).unwrap_or_default(),
        "content_hash": snapshot.content_hash,
        "state_version": snapshot.state_version,
        "generated_by": snapshot.generated_by,
        "created_at": snapshot.created_at,
    })
}

fn boundary_value(boundary: &ResumeBoundary) -> serde_json::Value {
    serde_json::json!({
        "id": boundary.id,
        "task_id": boundary.task_id,
        "cycle": boundary.cycle,
        "step_id": boundary.step_id,
        "task_item_id": boundary.task_item_id,
        "command_run_id": boundary.command_run_id,
        "provider_session_available": boundary.provider_session_available,
        "checkpoint_id": boundary.checkpoint_id,
        "side_effect_class": boundary.side_effect_class,
        "replay_safe": boundary.replay_safe,
        "reason": boundary.reason,
        "state_version": boundary.state_version,
    })
}

fn print_value(value: serde_json::Value, format: OutputFormat) {
    match format {
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        ),
        OutputFormat::Yaml | OutputFormat::Table => {
            print!("{}", serde_yaml::to_string(&value).unwrap_or_default())
        }
    }
}

fn short(value: &str) -> &str {
    value.get(..18).unwrap_or(value)
}

fn generated_key(prefix: &str) -> String {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("cli-{prefix}-{nonce}")
}

fn audit_context(
    reason_code: &str,
    operator_reason: Option<String>,
    idempotency_key: String,
) -> ActionAuditContext {
    ActionAuditContext {
        reason_code: reason_code.to_string(),
        operator_reason,
        idempotency_key: Some(idempotency_key),
    }
}
