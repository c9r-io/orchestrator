use crate::collab::{parse_artifacts_from_output, AgentOutput};
use crate::config::{BuildError, BuildErrorLevel, TestFailure};
use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

pub struct ValidationOutcome {
    pub output: AgentOutput,
    pub status: &'static str,
    pub error: Option<String>,
}

fn detect_fatal_agent_error(stdout: &str, stderr: &str) -> Option<&'static str> {
    // Scan stderr fully — provider errors (rate limits, auth failures) land here.
    // For stdout, skip JSON lines to avoid false positives from stream-json tool
    // outputs that embed source code containing error-pattern strings.
    let stderr_lower = stderr.to_ascii_lowercase();
    let stdout_plain: String = stdout
        .lines()
        .filter(|line| !line.starts_with('{'))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let combined = format!("{}\n{}", stdout_plain, stderr_lower);
    let patterns = [
        ("rate-limited", "provider rate limit exceeded"),
        ("rate limited", "provider rate limit exceeded"),
        ("quota exceeded", "provider quota exceeded"),
        ("quota exhausted", "provider quota exhausted"),
        ("quota resets in", "provider quota exhausted"),
        ("authentication failed", "provider authentication failed"),
        ("invalid api key", "provider authentication failed"),
    ];

    patterns
        .iter()
        .find_map(|(needle, reason)| combined.contains(needle).then_some(*reason))
}

fn is_strict_phase(phase: &str) -> bool {
    // Only phases that use simple echo-style agents with single-JSON-object stdout.
    // SDLC phases (qa_testing, qa_doc_gen, ticket_fix, align_tests, doc_governance)
    // use interactive CLI agents with stream-json output (multiple JSON lines),
    // which cannot be parsed as a single JSON value.
    matches!(phase, "qa" | "fix" | "retest" | "guard")
}

/// Returns true for phases that produce build/test structured output
fn is_build_phase(phase: &str) -> bool {
    matches!(phase, "build" | "lint")
}

fn is_test_phase(phase: &str) -> bool {
    phase == "test"
}

pub fn validate_phase_output(
    phase: &str,
    run_id: Uuid,
    agent_id: &str,
    exit_code: i64,
    stdout: &str,
    stderr: &str,
) -> Result<ValidationOutcome> {
    if let Some(reason) = detect_fatal_agent_error(stdout, stderr) {
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
            error: Some(reason.to_string()),
        });
    }

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

    // Parse structured build errors for build/lint phases
    let build_errors = if is_build_phase(phase) {
        parsed
            .as_ref()
            .and_then(|v| v.get("build_errors"))
            .and_then(|v| serde_json::from_value::<Vec<BuildError>>(v.clone()).ok())
            .unwrap_or_else(|| parse_build_errors_from_text(stderr, stdout))
    } else {
        Vec::new()
    };

    // Parse structured test failures for test phases
    let test_failures = if is_test_phase(phase) {
        parsed
            .as_ref()
            .and_then(|v| v.get("test_failures"))
            .and_then(|v| serde_json::from_value::<Vec<TestFailure>>(v.clone()).ok())
            .unwrap_or_else(|| parse_test_failures_from_text(stderr, stdout))
    } else {
        Vec::new()
    };

    let mut output = AgentOutput::new(
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

    output.build_errors = build_errors;
    output.test_failures = test_failures;

    Ok(ValidationOutcome {
        output,
        status: "passed",
        error: None,
    })
}

/// Parse build errors from compiler output (supports rustc/cargo format)
fn parse_build_errors_from_text(stderr: &str, stdout: &str) -> Vec<BuildError> {
    let mut errors = Vec::new();
    let combined = format!("{}\n{}", stderr, stdout);

    for line in combined.lines() {
        // Match rustc error format: "error[E0308]: mismatched types"
        // or "error: cannot find ..."
        // with location: " --> src/main.rs:10:5"
        if line.starts_with("error") {
            errors.push(BuildError {
                file: None,
                line: None,
                column: None,
                message: line.to_string(),
                level: BuildErrorLevel::Error,
            });
        } else if line.starts_with("warning") {
            errors.push(BuildError {
                file: None,
                line: None,
                column: None,
                message: line.to_string(),
                level: BuildErrorLevel::Warning,
            });
        } else if line.trim_start().starts_with("--> ") {
            // Parse location line: " --> src/main.rs:10:5"
            if let Some(last_error) = errors.last_mut() {
                let location = line.trim_start().trim_start_matches("--> ");
                let parts: Vec<&str> = location.rsplitn(3, ':').collect();
                if parts.len() >= 3 {
                    last_error.column = parts[0].parse().ok();
                    last_error.line = parts[1].parse().ok();
                    last_error.file = Some(parts[2].to_string());
                } else if parts.len() == 2 {
                    last_error.line = parts[0].parse().ok();
                    last_error.file = Some(parts[1].to_string());
                }
            }
        }
    }

    errors
}

/// Parse test failures from test runner output (supports cargo test format)
fn parse_test_failures_from_text(stderr: &str, stdout: &str) -> Vec<TestFailure> {
    let mut failures = Vec::new();
    let combined = format!("{}\n{}", stdout, stderr);

    let mut in_failure_block = false;
    let mut current_test: Option<String> = None;
    let mut current_message = String::new();

    for line in combined.lines() {
        // Match "---- test_name stdout ----" (cargo test failure block)
        if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            // Save previous failure if any
            if let Some(test_name) = current_test.take() {
                failures.push(TestFailure {
                    test_name,
                    file: None,
                    line: None,
                    message: current_message.trim().to_string(),
                    stdout: None,
                });
            }
            let name = line
                .trim_start_matches("---- ")
                .trim_end_matches(" stdout ----");
            current_test = Some(name.to_string());
            current_message.clear();
            in_failure_block = true;
        } else if in_failure_block {
            if line.starts_with("---- ") || line.starts_with("failures:") {
                // Save current and reset
                if let Some(test_name) = current_test.take() {
                    failures.push(TestFailure {
                        test_name,
                        file: None,
                        line: None,
                        message: current_message.trim().to_string(),
                        stdout: None,
                    });
                }
                current_message.clear();
                in_failure_block = false;
            } else {
                current_message.push_str(line);
                current_message.push('\n');
            }
        }
        // Also catch "test name ... FAILED" lines
        else if line.contains("... FAILED") && line.starts_with("test ") {
            let test_name = line
                .trim_start_matches("test ")
                .split(" ...")
                .next()
                .unwrap_or("unknown")
                .to_string();
            // Only add if not already captured in failure block
            if !failures.iter().any(|f| f.test_name == test_name) {
                failures.push(TestFailure {
                    test_name,
                    file: None,
                    line: None,
                    message: String::new(),
                    stdout: None,
                });
            }
        }
    }

    // Save last failure block
    if let Some(test_name) = current_test.take() {
        failures.push(TestFailure {
            test_name,
            file: None,
            line: None,
            message: current_message.trim().to_string(),
            stdout: None,
        });
    }

    failures
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

    #[test]
    fn build_phase_parses_errors() {
        let stderr = r#"error[E0308]: mismatched types
 --> src/main.rs:10:5
warning: unused variable
 --> src/lib.rs:3:9"#;
        let outcome = validate_phase_output("build", Uuid::new_v4(), "agent", 1, "", stderr)
            .expect("validation should return outcome");
        assert_eq!(outcome.output.build_errors.len(), 2);
        assert_eq!(outcome.output.build_errors[0].level, BuildErrorLevel::Error);
        assert_eq!(
            outcome.output.build_errors[0].file.as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(outcome.output.build_errors[0].line, Some(10));
    }

    #[test]
    fn test_phase_parses_failures() {
        let stdout = "test my_module::test_foo ... FAILED\ntest my_module::test_bar ... ok\n\n---- my_module::test_foo stdout ----\nthread 'my_module::test_foo' panicked at 'assertion failed'\n\nfailures:\n    my_module::test_foo\n";
        let outcome = validate_phase_output("test", Uuid::new_v4(), "agent", 1, stdout, "")
            .expect("validation should return outcome");
        // The "test ... FAILED" line is detected, and then the failure block merges with it
        assert!(!outcome.output.test_failures.is_empty());
        assert!(outcome
            .output
            .test_failures
            .iter()
            .any(|f| f.test_name == "my_module::test_foo"));
    }

    #[test]
    fn non_build_phase_has_no_build_errors() {
        let outcome = validate_phase_output("implement", Uuid::new_v4(), "agent", 0, "done", "")
            .expect("validation should return outcome");
        assert!(outcome.output.build_errors.is_empty());
        assert!(outcome.output.test_failures.is_empty());
    }

    #[test]
    fn fatal_provider_error_marks_run_failed_even_with_zero_exit_code() {
        let stderr = "Error: All 1 account(s) rate-limited for claude. Quota resets in 116h 44m.";
        let outcome = validate_phase_output("implement", Uuid::new_v4(), "agent", 0, "", stderr)
            .expect("validation should return outcome");
        assert_eq!(outcome.status, "failed");
        assert_eq!(
            outcome.error.as_deref(),
            Some("provider rate limit exceeded")
        );
    }

    #[test]
    fn fatal_provider_auth_error_marks_run_failed() {
        let stderr = "authentication failed: invalid API key";
        let outcome = validate_phase_output("align_tests", Uuid::new_v4(), "agent", 0, "", stderr)
            .expect("validation should return outcome");
        assert_eq!(outcome.status, "failed");
        assert_eq!(
            outcome.error.as_deref(),
            Some("provider authentication failed")
        );
    }

    #[test]
    fn build_phase_parses_warnings() {
        let stderr = "warning: unused variable `x`\n --> src/lib.rs:5:13";
        let outcome = validate_phase_output("build", Uuid::new_v4(), "agent", 0, "", stderr)
            .expect("validation should return outcome");
        assert_eq!(outcome.output.build_errors.len(), 1);
        assert_eq!(
            outcome.output.build_errors[0].level,
            BuildErrorLevel::Warning
        );
        assert_eq!(
            outcome.output.build_errors[0].file.as_deref(),
            Some("src/lib.rs")
        );
        assert_eq!(outcome.output.build_errors[0].line, Some(5));
    }

    #[test]
    fn sdlc_phases_accept_stream_json_output() {
        // SDLC phases use interactive CLI agents with stream-json output (multiple
        // JSON lines), so they must NOT be strict (single-JSON validation would fail).
        let sdlc_phases = [
            "qa_testing",
            "qa_doc_gen",
            "ticket_fix",
            "align_tests",
            "doc_governance",
        ];
        let stream_json = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"result","result":"done"}"#,
            "\n",
        );
        for phase in sdlc_phases {
            let outcome = validate_phase_output(phase, Uuid::new_v4(), "agent", 0, stream_json, "")
                .expect("validation should return outcome");
            assert_eq!(
                outcome.status, "passed",
                "phase {} should accept stream-json",
                phase
            );
        }
    }

    #[test]
    fn sdlc_phases_accept_plain_text_output() {
        // SDLC phases should also accept plain text (non-JSON) output without failing.
        let sdlc_phases = [
            "qa_testing",
            "qa_doc_gen",
            "ticket_fix",
            "align_tests",
            "doc_governance",
        ];
        for phase in sdlc_phases {
            let outcome =
                validate_phase_output(phase, Uuid::new_v4(), "agent", 0, "plain text output", "")
                    .expect("validation should return outcome");
            assert_eq!(
                outcome.status, "passed",
                "phase {} should accept plain text",
                phase
            );
        }
    }

    #[test]
    fn stream_json_with_embedded_error_patterns_no_false_positive() {
        // Stream-json agents emit tool outputs as JSON lines. When an agent reads
        // source files containing error-detection patterns (e.g. "authentication failed"),
        // those strings appear inside JSON objects in stdout. This must NOT trigger
        // a fatal error false positive.
        let stream_json_stdout = concat!(
            r#"{"type":"system","subtype":"init","model":"test"}"#,
            "\n",
            r#"{"type":"tool_result","content":"(\"authentication failed\", \"provider authentication failed\")"}"#,
            "\n",
            r#"{"type":"tool_result","content":"(\"rate-limited\", \"provider rate limit exceeded\")"}"#,
            "\n",
            r#"{"type":"result","result":"done"}"#,
            "\n",
        );
        let outcome = validate_phase_output(
            "implement",
            Uuid::new_v4(),
            "agent",
            0,
            stream_json_stdout,
            "",
        )
        .expect("validation should return outcome");
        assert_eq!(outcome.status, "passed");
        assert!(outcome.error.is_none());
    }

    #[test]
    fn plain_text_stdout_with_error_pattern_still_detected() {
        // Non-JSON stdout lines containing error patterns should still be caught.
        let stdout = "Error: authentication failed for provider";
        let outcome = validate_phase_output("implement", Uuid::new_v4(), "agent", 0, stdout, "")
            .expect("validation should return outcome");
        assert_eq!(outcome.status, "failed");
        assert_eq!(
            outcome.error.as_deref(),
            Some("provider authentication failed")
        );
    }
}
