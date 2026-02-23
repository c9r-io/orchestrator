use crate::collab::{parse_artifacts_from_output, AgentOutput};
use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

pub struct ValidationOutcome {
    pub output: AgentOutput,
    pub status: &'static str,
    pub error: Option<String>,
}

fn is_strict_phase(phase: &str) -> bool {
    matches!(phase, "qa" | "fix" | "retest" | "guard")
}

pub fn validate_phase_output(
    phase: &str,
    run_id: Uuid,
    agent_id: &str,
    exit_code: i64,
    stdout: &str,
    stderr: &str,
) -> Result<ValidationOutcome> {
    let strict = is_strict_phase(phase);
    let parsed_json = serde_json::from_str::<Value>(stdout);

    if strict && parsed_json.is_err() {
        let output = AgentOutput::new(
            run_id,
            agent_id.to_string(),
            phase.to_string(),
            exit_code,
            stdout.to_string(),
            stderr.to_string(),
        );
        return Ok(ValidationOutcome {
            output,
            status: "failed",
            error: Some("strict phase requires JSON stdout".to_string()),
        });
    }

    let parsed = parsed_json.ok();
    let confidence = parsed
        .as_ref()
        .and_then(|v| v.get("confidence"))
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(1.0);
    let quality_score = parsed
        .as_ref()
        .and_then(|v| v.get("quality_score"))
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(1.0);

    let artifacts = match &parsed {
        Some(v) => {
            if let Some(arr) = v.get("artifacts") {
                parse_artifacts_from_output(&serde_json::to_string(arr).unwrap_or_default())
            } else {
                parse_artifacts_from_output(stdout)
            }
        }
        None => parse_artifacts_from_output(stdout),
    };

    let output = AgentOutput::new(
        run_id,
        agent_id.to_string(),
        phase.to_string(),
        exit_code,
        stdout.to_string(),
        stderr.to_string(),
    )
    .with_artifacts(artifacts)
    .with_confidence(confidence)
    .with_quality_score(quality_score);

    Ok(ValidationOutcome {
        output,
        status: "passed",
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_phase_requires_json() {
        let outcome = validate_phase_output("qa", Uuid::new_v4(), "agent", 0, "plain-text", "")
            .expect("validation should return outcome");
        assert_eq!(outcome.status, "failed");
        assert!(outcome.error.is_some());
    }

    #[test]
    fn strict_phase_accepts_json() {
        let stdout = r#"{"confidence":0.7,"quality_score":0.8,"artifacts":[{"kind":"ticket","severity":"high","category":"bug"}]}"#;
        let outcome = validate_phase_output("qa", Uuid::new_v4(), "agent", 0, stdout, "")
            .expect("validation should return outcome");
        assert_eq!(outcome.status, "passed");
        assert_eq!(outcome.output.artifacts.len(), 1);
    }
}
