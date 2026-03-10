use crate::output_validation::validate_phase_output;
use crate::runner::redact_text;
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
        validation_error: validation.error,
        sandbox_denied: false,
        sandbox_event_type: None,
        sandbox_denial_reason: None,
        sandbox_denial_stderr_excerpt: None,
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    })
}
