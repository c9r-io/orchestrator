use crate::collab::{AgentOutput, parse_artifacts_from_output};
use crate::config::{BuildError, BuildErrorLevel, TestFailure};
use anyhow::Result;
use serde_json::Value;
use uuid::Uuid;

/// Outcome of validating one agent phase output payload.
pub struct ValidationOutcome {
    /// Parsed and enriched agent output.
    pub output: AgentOutput,
    /// Validation status string reported to the scheduler.
    pub status: &'static str,
    /// Optional validation error message.
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
    //
    // The `phase` parameter may be a step ID (e.g., "run_qa") rather than the
    // canonical step type (e.g., "qa"), because `TaskExecutionStep` only carries
    // the step `id`.  Match both exact types and IDs that end with `_<type>`,
    // but exclude known SDLC phases that happen to share a suffix (e.g., ticket_fix).
    const SDLC_PHASES: &[&str] = &[
        "qa_testing",
        "qa_doc_gen",
        "ticket_fix",
        "align_tests",
        "doc_governance",
    ];
    if SDLC_PHASES.contains(&phase) {
        return false;
    }
    const STRICT: &[&str] = &["qa", "fix", "retest", "guard", "adaptive_plan"];
    STRICT
        .iter()
        .any(|s| phase == *s || phase.ends_with(&format!("_{}", s)))
}

/// Returns true for phases that produce build/test structured output
fn is_build_phase(phase: &str) -> bool {
    matches!(phase, "build" | "lint")
}

fn is_test_phase(phase: &str) -> bool {
    phase == "test"
}

/// Validates one phase output payload and extracts structured diagnostics.
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

/// Line-scanning parser for compiler/test diagnostic output.
/// Implementors define per-line state transitions; the shared driver handles
/// combining stderr+stdout and iterating lines.
trait DiagnosticParser: Default {
    type Item;
    fn process_line(&mut self, line: &str);
    fn finish(self) -> Vec<Self::Item>;
}

fn parse_diagnostic_output<P: DiagnosticParser>(stderr: &str, stdout: &str) -> Vec<P::Item> {
    let combined = format!("{}\n{}", stderr, stdout);
    let mut parser = P::default();
    for line in combined.lines() {
        parser.process_line(line);
    }
    parser.finish()
}

/// Extract file, line, and column from a rustc location line like " --> src/main.rs:10:5"
fn parse_location_line(line: &str) -> (Option<String>, Option<u32>, Option<u32>) {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("--> ") {
        return (None, None, None);
    }
    let location = trimmed.trim_start_matches("--> ");
    if location.is_empty() {
        return (None, None, None);
    }
    let parts: Vec<&str> = location.rsplitn(3, ':').collect();
    if parts.len() >= 3 {
        (
            Some(parts[2].to_string()),
            parts[1].parse().ok(),
            parts[0].parse().ok(),
        )
    } else if parts.len() == 2 {
        (Some(parts[1].to_string()), parts[0].parse().ok(), None)
    } else {
        (None, None, None)
    }
}

// ---------------------------------------------------------------------------
// BuildErrorParser
// ---------------------------------------------------------------------------

#[derive(Default)]
struct BuildErrorParser {
    errors: Vec<BuildError>,
}

impl DiagnosticParser for BuildErrorParser {
    type Item = BuildError;

    fn process_line(&mut self, line: &str) {
        if line.starts_with("error") {
            self.errors.push(BuildError {
                file: None,
                line: None,
                column: None,
                message: line.to_string(),
                level: BuildErrorLevel::Error,
            });
        } else if line.starts_with("warning") {
            self.errors.push(BuildError {
                file: None,
                line: None,
                column: None,
                message: line.to_string(),
                level: BuildErrorLevel::Warning,
            });
        } else if line.trim_start().starts_with("--> ") {
            if let Some(last_error) = self.errors.last_mut() {
                let (file, line_num, col) = parse_location_line(line);
                last_error.file = file;
                last_error.line = line_num;
                last_error.column = col;
            }
        }
    }

    fn finish(self) -> Vec<BuildError> {
        self.errors
    }
}

fn parse_build_errors_from_text(stderr: &str, stdout: &str) -> Vec<BuildError> {
    parse_diagnostic_output::<BuildErrorParser>(stderr, stdout)
}

// ---------------------------------------------------------------------------
// TestFailureParser
// ---------------------------------------------------------------------------

#[derive(Default)]
struct TestFailureParser {
    failures: Vec<TestFailure>,
    in_failure_block: bool,
    current_test: Option<String>,
    current_message: String,
}

impl TestFailureParser {
    fn flush_current(&mut self) {
        if let Some(test_name) = self.current_test.take() {
            self.failures.push(TestFailure {
                test_name,
                file: None,
                line: None,
                message: self.current_message.trim().to_string(),
                stdout: None,
            });
        }
        self.current_message.clear();
    }
}

impl DiagnosticParser for TestFailureParser {
    type Item = TestFailure;

    fn process_line(&mut self, line: &str) {
        if line.starts_with("---- ") && line.ends_with(" stdout ----") {
            self.flush_current();
            let name = line
                .trim_start_matches("---- ")
                .trim_end_matches(" stdout ----");
            self.current_test = Some(name.to_string());
            self.in_failure_block = true;
        } else if self.in_failure_block {
            if line.starts_with("---- ") || line.starts_with("failures:") {
                self.flush_current();
                self.in_failure_block = false;
            } else {
                self.current_message.push_str(line);
                self.current_message.push('\n');
            }
        } else if line.contains("... FAILED") && line.starts_with("test ") {
            let test_name = line
                .trim_start_matches("test ")
                .split(" ...")
                .next()
                .unwrap_or("unknown")
                .to_string();
            if !self.failures.iter().any(|f| f.test_name == test_name) {
                self.failures.push(TestFailure {
                    test_name,
                    file: None,
                    line: None,
                    message: String::new(),
                    stdout: None,
                });
            }
        }
    }

    fn finish(mut self) -> Vec<TestFailure> {
        self.flush_current();
        self.failures
    }
}

fn parse_test_failures_from_text(stderr: &str, stdout: &str) -> Vec<TestFailure> {
    parse_diagnostic_output::<TestFailureParser>(stderr, stdout)
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
    fn strict_phase_suffix_match_requires_json() {
        // Step IDs like "run_qa" should be treated as strict (matching "_qa" suffix)
        let outcome = validate_phase_output("run_qa", Uuid::new_v4(), "agent", 0, "plain-text", "")
            .expect("validation should return outcome");
        assert_eq!(
            outcome.status, "failed",
            "step ID 'run_qa' should be strict via suffix match"
        );
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
        assert!(
            outcome
                .output
                .test_failures
                .iter()
                .any(|f| f.test_name == "my_module::test_foo")
        );
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

    #[test]
    fn diagnostic_parser_trait_build_errors_direct() {
        let errors = parse_diagnostic_output::<BuildErrorParser>(
            "error[E0308]: mismatch\n --> src/main.rs:10:5",
            "",
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file.as_deref(), Some("src/main.rs"));
        assert_eq!(errors[0].line, Some(10));
    }

    #[test]
    fn diagnostic_parser_trait_test_failures_direct() {
        let failures = parse_diagnostic_output::<TestFailureParser>(
            "",
            "---- foo stdout ----\npanicked\nfailures:\n",
        );
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].test_name, "foo");
        assert_eq!(failures[0].message, "panicked");
    }

    #[test]
    fn diagnostic_parser_combine_order_consistent() {
        // Both parsers now use stderr\nstdout order via parse_diagnostic_output.
        // Build errors in stderr should be found.
        let errors = parse_build_errors_from_text("error: in stderr", "");
        assert_eq!(errors.len(), 1);
        // Test failures in stdout should be found.
        let failures = parse_test_failures_from_text("", "test bar ... FAILED");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].test_name, "bar");
    }

    #[test]
    fn parse_location_line_full() {
        let (file, line, col) = parse_location_line(" --> src/main.rs:10:5");
        assert_eq!(file.as_deref(), Some("src/main.rs"));
        assert_eq!(line, Some(10));
        assert_eq!(col, Some(5));
    }

    #[test]
    fn parse_location_line_no_column() {
        let (file, line, col) = parse_location_line(" --> src/lib.rs:3");
        assert_eq!(file.as_deref(), Some("src/lib.rs"));
        assert_eq!(line, Some(3));
        assert_eq!(col, None);
    }

    #[test]
    fn parse_location_line_not_a_location() {
        let (file, line, col) = parse_location_line("not a location line");
        assert!(file.is_none());
        assert!(line.is_none());
        assert!(col.is_none());
    }

    #[test]
    fn parse_location_line_empty_after_arrow() {
        let (file, line, col) = parse_location_line(" --> ");
        assert!(file.is_none());
        assert!(line.is_none());
        assert!(col.is_none());
    }

    #[test]
    fn test_failure_parser_combine_order() {
        // The refactored code combines as stderr\nstdout (stderr first).
        // Test failures typically appear in stdout. Verify they are still found
        // when stderr has unrelated content prepended.
        let stderr = "Compiling my_crate v0.1.0\nFinished test target";
        let stdout = "\
---- foo::bar stdout ----\n\
thread 'foo::bar' panicked at 'assert_eq failed'\n\
\n\
failures:\n\
    foo::bar\n";
        let failures = parse_test_failures_from_text(stderr, stdout);
        assert_eq!(failures.len(), 1, "should find exactly one failure");
        assert_eq!(failures[0].test_name, "foo::bar");
        assert!(
            failures[0].message.contains("panicked"),
            "message should contain panic text"
        );
    }

    #[test]
    fn build_errors_multiple_interleaved() {
        // Multiple errors and warnings interleaved with location lines.
        let stderr = "\
error[E0308]: mismatched types\n\
 --> src/main.rs:10:5\n\
warning: unused variable `x`\n\
 --> src/lib.rs:3:9\n\
error[E0433]: unresolved import\n\
 --> src/util.rs:1:5";
        let errors = parse_build_errors_from_text(stderr, "");
        assert_eq!(errors.len(), 3);
        // First: error with location
        assert_eq!(errors[0].level, BuildErrorLevel::Error);
        assert_eq!(errors[0].file.as_deref(), Some("src/main.rs"));
        assert_eq!(errors[0].line, Some(10));
        assert_eq!(errors[0].column, Some(5));
        // Second: warning with location
        assert_eq!(errors[1].level, BuildErrorLevel::Warning);
        assert_eq!(errors[1].file.as_deref(), Some("src/lib.rs"));
        assert_eq!(errors[1].line, Some(3));
        assert_eq!(errors[1].column, Some(9));
        // Third: error with location
        assert_eq!(errors[2].level, BuildErrorLevel::Error);
        assert_eq!(errors[2].file.as_deref(), Some("src/util.rs"));
        assert_eq!(errors[2].line, Some(1));
        assert_eq!(errors[2].column, Some(5));
    }

    #[test]
    fn test_failure_parser_last_block_no_delimiter() {
        // A failure block at the end of output with no trailing "failures:" line.
        // The finish() method should flush the in-progress block.
        let stdout = "\
---- my_mod::test_alpha stdout ----\n\
thread 'my_mod::test_alpha' panicked at 'value was None'\n\
note: run with `RUST_BACKTRACE=1`";
        let failures = parse_test_failures_from_text("", stdout);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].test_name, "my_mod::test_alpha");
        assert!(
            failures[0].message.contains("panicked"),
            "should capture the panic message"
        );
    }

    #[test]
    fn parse_location_line_windows_path() {
        // Windows-style paths use backslashes. The rsplitn(':') approach splits
        // from the right, so `C:\src\main.rs:10:5` should parse with the full
        // path preserved (everything left of the last two colons).
        let (file, line, col) = parse_location_line(r" --> C:\src\main.rs:10:5");
        assert_eq!(file.as_deref(), Some(r"C:\src\main.rs"));
        assert_eq!(line, Some(10));
        assert_eq!(col, Some(5));
    }
}
