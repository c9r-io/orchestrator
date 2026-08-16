use agent_orchestrator::driver::{DriverEvent, DriverOutcome};
use agent_orchestrator::output_validation::validate_phase_output;
use agent_orchestrator::runner::redact_text;
use anyhow::{Context, Result};
use serde_json::{Value, json};
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
    stdout_path: &Path,
    stderr_path: &Path,
    redaction_patterns: &[String],
) -> Result<ValidatedOutput> {
    use agent_orchestrator::collab::{AgentOutput, Artifact, ArtifactKind, ExecutionMetrics};
    use agent_orchestrator::output_validation::structured_scores;

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
            DriverEvent::Finished {
                outcome, exit_code, ..
            } => {
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
    // Terminal truth is the driver's, and stays so. The `confidence` / `quality_score`
    // contract is a separate axis that `AgentOutput::new` defaults to 1.0/1.0, and folding
    // events without reading it silently disabled every confidence-driven policy — the
    // low-confidence Attention handoff DD-128 specifies among them. The shell driver emits
    // no assistant text at all, so its payload only ever reaches us as the stdout artifact.
    let assistant_text = text.join("\n");
    let contract = if assistant_text.trim().is_empty() {
        const MAX_STDOUT_BYTES: u64 = 256 * 1024;
        read_output_with_limit(stdout_path, MAX_STDOUT_BYTES)
            .await
            .with_context(|| format!("failed to read stdout log: {}", stdout_path.display()))?
            .text
    } else {
        assistant_text.clone()
    };
    let (confidence, quality_score) =
        structured_scores(serde_json::from_str::<Value>(contract.trim()).ok().as_ref());

    let output = AgentOutput::new(
        run_uuid,
        agent_id.to_string(),
        phase.to_string(),
        final_exit_code,
        assistant_text,
        redact_text(&stderr, redaction_patterns),
    )
    .with_artifacts(artifacts)
    .with_metrics(ExecutionMetrics {
        tokens_consumed: (tokens > 0).then_some(tokens),
        ..ExecutionMetrics::default()
    })
    .with_confidence(confidence)
    .with_quality_score(quality_score);
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

    /// Writes `stdout`/`stderr` artifacts and folds `events` over them.
    async fn fold(
        stdout: &str,
        events: Vec<DriverEvent>,
    ) -> (super::ValidatedOutput, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("tempdir");
        let stdout_path = directory.path().join("stdout.log");
        let stderr_path = directory.path().join("stderr.log");
        std::fs::write(&stdout_path, stdout).expect("stdout");
        std::fs::write(&stderr_path, "").expect("stderr");
        let validated = validate_driver_events_stage(
            "warehouse_reply",
            Uuid::new_v4(),
            "driver-agent",
            0,
            &events,
            &stdout_path,
            &stderr_path,
            &[],
        )
        .await
        .expect("fold driver stream");
        (validated, directory)
    }

    fn finished() -> DriverEvent {
        DriverEvent::Finished {
            outcome: DriverOutcome::Success,
            exit_code: 0,
            exit_signal: None,
        }
    }

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
                exit_signal: None,
            },
        ];

        let validated = validate_driver_events_stage(
            "implement",
            Uuid::new_v4(),
            "driver-agent",
            0,
            &events,
            &stdout_path,
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
        let stdout_path = directory.path().join("stdout.log");
        let stderr_path = directory.path().join("stderr.log");
        std::fs::write(&stdout_path, "").expect("stdout");
        std::fs::write(&stderr_path, "provider failed").expect("stderr");
        let events = vec![DriverEvent::Finished {
            outcome: DriverOutcome::Failed,
            exit_code: 1,
            exit_signal: None,
        }];

        let validated = validate_driver_events_stage(
            "qa",
            Uuid::new_v4(),
            "shell-driver",
            1,
            &events,
            &stdout_path,
            &stderr_path,
            &[],
        )
        .await
        .expect("fold failed terminal");

        assert!(!validated.success);
        assert_eq!(validated.final_exit_code, 1);
        assert_eq!(validated.validation_status, "failed");
    }

    /// The shell driver emits only a terminal event, so its structured contract reaches the
    /// fold as the stdout artifact and nowhere else. Dropping it defaults confidence to 1.0
    /// and silently disables every low-confidence policy downstream.
    #[tokio::test]
    async fn shell_driver_structured_contract_survives_the_fold() {
        let (validated, _directory) = fold(
            r#"{"artifacts":[],"confidence":0.4,"quality_score":0.8}"#,
            vec![finished()],
        )
        .await;

        assert!(validated.success);
        assert_eq!(validated.redacted_output.confidence, 0.4);
        assert_eq!(validated.redacted_output.quality_score, 0.8);
    }

    /// When the driver does normalize the payload, that is the authoritative copy — the
    /// stdout artifact is not consulted, and cannot contradict it.
    #[tokio::test]
    async fn normalized_assistant_text_outranks_the_stdout_artifact() {
        let (validated, _directory) = fold(
            r#"{"confidence":0.9,"quality_score":0.9}"#,
            vec![
                DriverEvent::AssistantText(r#"{"confidence":0.2,"quality_score":0.3}"#.to_string()),
                finished(),
            ],
        )
        .await;

        assert_eq!(validated.redacted_output.confidence, 0.2);
        assert_eq!(validated.redacted_output.quality_score, 0.3);
    }

    /// An agent that emits prose, or nothing at all, is not making a claim about its own
    /// confidence — it must not be read as a low-confidence one.
    #[tokio::test]
    async fn payload_without_the_contract_keeps_the_defaults() {
        for payload in ["", "  ", "all done, no JSON here", "[1,2,3]"] {
            let (validated, _directory) = fold(payload, vec![finished()]).await;
            assert_eq!(
                validated.redacted_output.confidence, 1.0,
                "payload {payload:?} should not lower confidence"
            );
            assert_eq!(validated.redacted_output.quality_score, 1.0);
        }
    }
}
