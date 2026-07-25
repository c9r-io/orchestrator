use agent_orchestrator::driver::{DriverEvent, DriverOutcome};
use agent_orchestrator::output_validation::validate_phase_output;
use agent_orchestrator::runner::redact_text;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use uuid::Uuid;

use super::types::ValidatedOutput;
use super::util::{effective_exit_code, read_output_with_limit};

/// Stage 4: Read output files, validate structure, sanitize, classify.
#[allow(clippy::too_many_arguments)]
pub(super) async fn validate_phase_output_stage(
    phase: &str,
    run_uuid: Uuid,
    run_id: &str,
    agent_id: &str,
    exit_code: i32,
    stdout_path: &Path,
    stderr_path: &Path,
    redaction_patterns: &[String],
) -> Result<ValidatedOutput> {
    const MAX_PHASE_OUTPUT_BYTES: u64 = 256 * 1024;
    let stdout_output = read_output_with_limit(stdout_path, MAX_PHASE_OUTPUT_BYTES)
        .await
        .with_context(|| format!("failed to read stdout log: {}", stdout_path.display()))?;
    let stderr_output = read_output_with_limit(stderr_path, MAX_PHASE_OUTPUT_BYTES)
        .await
        .with_context(|| format!("failed to read stderr log: {}", stderr_path.display()))?;
    let stdout_content = stdout_output.text;
    let stderr_content = stderr_output.text;

    let validation = validate_phase_output(
        phase,
        run_uuid,
        agent_id,
        exit_code as i64,
        &stdout_content,
        &stderr_content,
    )?;
    let final_exit_code = effective_exit_code(exit_code as i64, validation.status);
    let mut success = final_exit_code == 0;
    let mut validation_event_payload_json: Option<String> = None;
    if validation.status == "failed" {
        success = false;
        validation_event_payload_json = Some(serde_json::to_string(&json!({
            "phase": phase,
            "run_id": run_id,
            "error": validation.error.as_deref().map(|e| redact_text(e, redaction_patterns)),
            "stdout_truncated_prefix_bytes": stdout_output.truncated_prefix_bytes,
            "stderr_truncated_prefix_bytes": stderr_output.truncated_prefix_bytes
        }))?);
    }

    let mut redacted_output = validation.output.clone();
    redacted_output.stdout = redact_text(&redacted_output.stdout, redaction_patterns);
    redacted_output.stderr = redact_text(&redacted_output.stderr, redaction_patterns);

    Ok(ValidatedOutput {
        final_exit_code,
        success,
        validation_status: validation.status,
        validation_event_payload_json,
        redacted_output,
        sandbox_denied: false,
        sandbox_event_type: None,
        sandbox_reason_code: None,
        sandbox_denial_reason: None,
        sandbox_denial_stderr_excerpt: None,
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    })
}

/// Folds the normalized driver stream directly, avoiding a second parse of a
/// size-limited stdout artifact as the terminal source of truth.
#[allow(clippy::too_many_arguments)]
pub(super) async fn validate_driver_events_stage(
    phase: &str,
    run_uuid: Uuid,
    agent_id: &str,
    process_exit_code: i32,
    events: &[DriverEvent],
    stderr_path: &Path,
    redaction_patterns: &[String],
) -> Result<ValidatedOutput> {
    use agent_orchestrator::collab::{AgentOutput, Artifact, ArtifactKind, ExecutionMetrics};

    const MAX_STDERR_BYTES: u64 = 256 * 1024;
    let stderr = read_output_with_limit(stderr_path, MAX_STDERR_BYTES)
        .await
        .with_context(|| format!("failed to read stderr log: {}", stderr_path.display()))?
        .text;
    let mut text = Vec::new();
    let mut artifacts = Vec::new();
    let mut tokens = 0_u64;
    let mut terminal = None;
    for event in events {
        match event {
            DriverEvent::AssistantText(value) => {
                text.push(redact_text(value, redaction_patterns));
            }
            DriverEvent::ToolUse {
                call_id,
                name,
                args,
            } => artifacts.push(
                Artifact::new(ArtifactKind::ToolCall { tool: name.clone() }).with_content(json!({
                    "call_id": call_id,
                    "args": args,
                })),
            ),
            DriverEvent::ToolResult {
                call_id,
                payload,
                is_error,
            } => artifacts.push(
                Artifact::new(ArtifactKind::Data {
                    schema: "driver_tool_result".to_string(),
                })
                .with_content(json!({
                    "call_id": call_id,
                    "payload": payload,
                    "is_error": is_error,
                })),
            ),
            DriverEvent::Usage { tokens: counts, .. } => {
                tokens = tokens
                    .saturating_add(counts.input.unwrap_or(0))
                    .saturating_add(counts.output.unwrap_or(0));
            }
            DriverEvent::Finished { outcome, exit_code } => {
                terminal = Some((*outcome, *exit_code));
                artifacts.push(
                    Artifact::new(ArtifactKind::Data {
                        schema: "driver_terminal".to_string(),
                    })
                    .with_content(json!({
                        "outcome": format!("{outcome:?}").to_lowercase(),
                        "exit_code": exit_code,
                    })),
                );
            }
            DriverEvent::Started { .. } | DriverEvent::PermissionRequested { .. } => {}
        }
    }
    let (outcome, driver_exit_code) =
        terminal.unwrap_or((DriverOutcome::Failed, process_exit_code));
    let final_exit_code = match outcome {
        DriverOutcome::Success => driver_exit_code as i64,
        DriverOutcome::Failed | DriverOutcome::Cancelled => {
            if driver_exit_code == 0 {
                1
            } else {
                driver_exit_code as i64
            }
        }
    };
    let output = AgentOutput::new(
        run_uuid,
        agent_id.to_string(),
        phase.to_string(),
        final_exit_code,
        text.join("\n"),
        redact_text(&stderr, redaction_patterns),
    )
    .with_artifacts(artifacts)
    .with_metrics(ExecutionMetrics {
        tokens_consumed: (tokens > 0).then_some(tokens),
        ..ExecutionMetrics::default()
    });
    let success = final_exit_code == 0;

    Ok(ValidatedOutput {
        final_exit_code,
        success,
        validation_status: if success { "passed" } else { "failed" },
        validation_event_payload_json: None,
        redacted_output: output,
        sandbox_denied: false,
        sandbox_event_type: None,
        sandbox_reason_code: None,
        sandbox_denial_reason: None,
        sandbox_denial_stderr_excerpt: None,
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    })
}

#[cfg(test)]
mod driver_tests {
    use super::*;

    #[tokio::test]
    async fn driver_terminal_truth_does_not_depend_on_bounded_stdout_reread() {
        let directory = tempfile::tempdir().expect("tempdir");
        let stdout_path = directory.path().join("stdout.log");
        let stderr_path = directory.path().join("stderr.log");
        std::fs::write(&stdout_path, "x".repeat(300 * 1024)).expect("large stdout");
        std::fs::write(&stderr_path, "").expect("stderr");
        let events = vec![
            DriverEvent::AssistantText("normalized answer".to_string()),
            DriverEvent::Finished {
                outcome: DriverOutcome::Success,
                exit_code: 0,
            },
        ];

        let validated = validate_driver_events_stage(
            "implement",
            Uuid::new_v4(),
            "driver-agent",
            0,
            &events,
            &stderr_path,
            &[],
        )
        .await
        .expect("fold normalized stream");

        assert!(validated.success);
        assert_eq!(validated.redacted_output.stdout, "normalized answer");
        assert!(std::fs::metadata(stdout_path).expect("metadata").len() > 256 * 1024);
    }

    #[tokio::test]
    async fn failed_driver_terminal_is_a_hard_validation_failure() {
        let directory = tempfile::tempdir().expect("tempdir");
        let stderr_path = directory.path().join("stderr.log");
        std::fs::write(&stderr_path, "provider failed").expect("stderr");
        let events = vec![DriverEvent::Finished {
            outcome: DriverOutcome::Failed,
            exit_code: 1,
        }];

        let validated = validate_driver_events_stage(
            "qa",
            Uuid::new_v4(),
            "shell-driver",
            1,
            &events,
            &stderr_path,
            &[],
        )
        .await
        .expect("fold failed terminal");

        assert!(!validated.success);
        assert_eq!(validated.final_exit_code, 1);
        assert_eq!(validated.validation_status, "failed");
    }
}
