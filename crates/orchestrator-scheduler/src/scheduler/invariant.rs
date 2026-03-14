use agent_orchestrator::config::{InvariantCheckPoint, InvariantConfig, InvariantResult, OnViolation};
use anyhow::Result;
use std::path::Path;
use tracing::warn;

/// Evaluate all invariants that match the given checkpoint.
pub fn evaluate_invariants(
    invariants: &[InvariantConfig],
    checkpoint: InvariantCheckPoint,
    workspace_root: &Path,
) -> Result<Vec<InvariantResult>> {
    let mut results = Vec::new();
    for inv in invariants {
        if !inv.check_at.contains(&checkpoint) {
            continue;
        }
        let result = run_single_invariant(inv, workspace_root)?;
        results.push(result);
    }
    Ok(results)
}

/// Check if any invariant result indicates a violation that should halt execution.
pub fn has_halting_violation(results: &[InvariantResult]) -> bool {
    results
        .iter()
        .any(|r| !r.passed && r.on_violation == OnViolation::Halt)
}

/// Check if any invariant result indicates a violation that should trigger rollback.
pub fn has_rollback_violation(results: &[InvariantResult]) -> bool {
    results
        .iter()
        .any(|r| !r.passed && r.on_violation == OnViolation::Rollback)
}

fn run_single_invariant(
    invariant: &InvariantConfig,
    workspace_root: &Path,
) -> Result<InvariantResult> {
    // Check protected files first
    if !invariant.protected_files.is_empty() {
        if let Some(violation_msg) = check_protected_files(invariant, workspace_root) {
            return Ok(InvariantResult {
                name: invariant.name.clone(),
                passed: false,
                message: violation_msg,
                on_violation: invariant.on_violation,
            });
        }
    }

    // Run command if specified
    if let Some(ref command) = invariant.command {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(workspace_root)
            .output();

        match output {
            Ok(out) => {
                let exit_code = out.status.code().unwrap_or(-1);
                let expected_exit = invariant.expect_exit.unwrap_or(0);

                if exit_code != expected_exit {
                    return Ok(InvariantResult {
                        name: invariant.name.clone(),
                        passed: false,
                        message: format!(
                            "command exited with {} (expected {}): {}",
                            exit_code,
                            expected_exit,
                            String::from_utf8_lossy(&out.stderr).trim()
                        ),
                        on_violation: invariant.on_violation,
                    });
                }

                // If assert_expr is specified, evaluate it
                if let Some(ref expr) = invariant.assert_expr {
                    let stdout_str = String::from_utf8_lossy(&out.stdout).to_string();
                    let passed = evaluate_invariant_assertion(expr, exit_code, &stdout_str);
                    if !passed {
                        return Ok(InvariantResult {
                            name: invariant.name.clone(),
                            passed: false,
                            message: format!("assertion failed: {}", expr),
                            on_violation: invariant.on_violation,
                        });
                    }
                }
            }
            Err(e) => {
                warn!(invariant = %invariant.name, error = %e, "invariant command failed to execute");
                return Ok(InvariantResult {
                    name: invariant.name.clone(),
                    passed: false,
                    message: format!("command execution failed: {}", e),
                    on_violation: invariant.on_violation,
                });
            }
        }
    }

    Ok(InvariantResult {
        name: invariant.name.clone(),
        passed: true,
        message: String::new(),
        on_violation: invariant.on_violation,
    })
}

/// Check if protected files have been modified (via git diff).
fn check_protected_files(invariant: &InvariantConfig, workspace_root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None; // Can't check, don't block
    }

    let changed_files: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap_or("")
        .lines()
        .collect();

    for pattern in &invariant.protected_files {
        for changed in &changed_files {
            if file_matches_pattern(changed, pattern) {
                return Some(format!(
                    "protected file '{}' was modified (pattern: '{}')",
                    changed, pattern
                ));
            }
        }
    }
    None
}

/// Simple glob matching: supports `*` as wildcard.
fn file_matches_pattern(file: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        // Simple prefix/suffix matching
        if let Some(prefix) = pattern.strip_suffix('*') {
            return file.starts_with(prefix);
        }
        if let Some(suffix) = pattern.strip_prefix('*') {
            return file.ends_with(suffix);
        }
    }
    file == pattern
}

/// Evaluate a simple invariant assertion expression.
/// Supports basic comparisons: `exit_code == N`, `exit_code != N`.
fn evaluate_invariant_assertion(expr: &str, exit_code: i32, _stdout: &str) -> bool {
    let expr = expr.trim();

    if let Some(rest) = expr.strip_prefix("exit_code == ") {
        if let Ok(expected) = rest.trim().parse::<i32>() {
            return exit_code == expected;
        }
    }
    if let Some(rest) = expr.strip_prefix("exit_code != ") {
        if let Ok(expected) = rest.trim().parse::<i32>() {
            return exit_code != expected;
        }
    }

    // Default: treat non-zero exit as failure
    warn!(
        expr = expr,
        "unsupported invariant assertion expression, defaulting to exit_code == 0"
    );
    exit_code == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::config::{InvariantCheckPoint, InvariantConfig, OnViolation};

    fn make_invariant(name: &str, command: Option<&str>) -> InvariantConfig {
        InvariantConfig {
            name: name.to_string(),
            description: String::new(),
            command: command.map(|s| s.to_string()),
            expect_exit: None,
            capture_as: None,
            assert_expr: None,
            immutable: false,
            check_at: vec![InvariantCheckPoint::AfterImplement],
            on_violation: OnViolation::Halt,
            protected_files: vec![],
        }
    }

    #[test]
    fn test_evaluate_invariants_filters_by_checkpoint() {
        let inv1 = InvariantConfig {
            check_at: vec![InvariantCheckPoint::BeforeCycle],
            ..make_invariant("inv1", Some("true"))
        };
        let inv2 = InvariantConfig {
            check_at: vec![InvariantCheckPoint::AfterImplement],
            ..make_invariant("inv2", Some("true"))
        };

        let results = evaluate_invariants(
            &[inv1, inv2],
            InvariantCheckPoint::BeforeCycle,
            Path::new("/tmp"),
        )
        .expect("evaluate");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "inv1");
    }

    #[test]
    fn test_passing_invariant() {
        let inv = make_invariant("pass", Some("true"));
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(result.passed);
    }

    #[test]
    fn test_failing_invariant() {
        let inv = make_invariant("fail", Some("false"));
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(!result.passed);
    }

    #[test]
    fn test_invariant_with_expected_exit() {
        let inv = InvariantConfig {
            expect_exit: Some(1),
            ..make_invariant("expect_1", Some("false"))
        };
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(result.passed);
    }

    #[test]
    fn test_has_halting_violation() {
        let results = vec![
            InvariantResult {
                name: "ok".to_string(),
                passed: true,
                message: String::new(),
                on_violation: OnViolation::Halt,
            },
            InvariantResult {
                name: "bad".to_string(),
                passed: false,
                message: "failed".to_string(),
                on_violation: OnViolation::Halt,
            },
        ];
        assert!(has_halting_violation(&results));
    }

    #[test]
    fn test_no_halting_violation_when_warn() {
        let results = vec![InvariantResult {
            name: "warn".to_string(),
            passed: false,
            message: "failed".to_string(),
            on_violation: OnViolation::Warn,
        }];
        assert!(!has_halting_violation(&results));
    }

    #[test]
    fn test_file_matches_pattern_exact() {
        assert!(file_matches_pattern("Cargo.toml", "Cargo.toml"));
        assert!(!file_matches_pattern("Cargo.lock", "Cargo.toml"));
    }

    #[test]
    fn test_file_matches_pattern_prefix_glob() {
        assert!(file_matches_pattern("src/main.rs", "src/*"));
        assert!(!file_matches_pattern("tests/main.rs", "src/*"));
    }

    #[test]
    fn test_file_matches_pattern_suffix_glob() {
        assert!(file_matches_pattern("src/main.rs", "*.rs"));
        assert!(!file_matches_pattern("src/main.py", "*.rs"));
    }

    #[test]
    fn test_evaluate_assertion_eq() {
        assert!(evaluate_invariant_assertion("exit_code == 0", 0, ""));
        assert!(!evaluate_invariant_assertion("exit_code == 0", 1, ""));
    }

    #[test]
    fn test_evaluate_assertion_neq() {
        assert!(evaluate_invariant_assertion("exit_code != 0", 1, ""));
        assert!(!evaluate_invariant_assertion("exit_code != 0", 0, ""));
    }

    #[test]
    fn test_has_rollback_violation() {
        let results = vec![
            InvariantResult {
                name: "ok".to_string(),
                passed: true,
                message: String::new(),
                on_violation: OnViolation::Rollback,
            },
            InvariantResult {
                name: "bad".to_string(),
                passed: false,
                message: "regression".to_string(),
                on_violation: OnViolation::Rollback,
            },
        ];
        assert!(has_rollback_violation(&results));
    }

    #[test]
    fn test_no_rollback_violation_when_all_pass() {
        let results = vec![InvariantResult {
            name: "ok".to_string(),
            passed: true,
            message: String::new(),
            on_violation: OnViolation::Rollback,
        }];
        assert!(!has_rollback_violation(&results));
    }

    #[test]
    fn test_no_rollback_violation_when_halt() {
        let results = vec![InvariantResult {
            name: "bad".to_string(),
            passed: false,
            message: "failed".to_string(),
            on_violation: OnViolation::Halt,
        }];
        assert!(!has_rollback_violation(&results));
    }

    #[test]
    fn test_has_halting_violation_empty_results() {
        assert!(!has_halting_violation(&[]));
    }

    #[test]
    fn test_has_rollback_violation_empty_results() {
        assert!(!has_rollback_violation(&[]));
    }

    #[test]
    fn test_evaluate_assertion_unsupported_expression() {
        // Unsupported expression defaults to exit_code == 0
        assert!(evaluate_invariant_assertion("some_unknown_thing", 0, ""));
        assert!(!evaluate_invariant_assertion("some_unknown_thing", 1, ""));
    }

    #[test]
    fn test_evaluate_assertion_invalid_number() {
        // Invalid parse in exit_code == should default to exit_code == 0
        assert!(evaluate_invariant_assertion("exit_code == abc", 0, ""));
        assert!(!evaluate_invariant_assertion("exit_code == abc", 1, ""));
    }

    #[test]
    fn test_evaluate_assertion_whitespace_handling() {
        assert!(evaluate_invariant_assertion("  exit_code == 0  ", 0, ""));
    }

    #[test]
    fn test_invariant_no_command_passes() {
        let inv = make_invariant("no_cmd", None);
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(result.passed);
        assert!(result.message.is_empty());
    }

    #[test]
    fn test_evaluate_invariants_empty_list() {
        let results = evaluate_invariants(&[], InvariantCheckPoint::BeforeCycle, Path::new("/tmp"))
            .expect("evaluate");
        assert!(results.is_empty());
    }

    #[test]
    fn test_evaluate_invariants_all_matching_checkpoints() {
        let inv1 = InvariantConfig {
            check_at: vec![InvariantCheckPoint::BeforeCycle],
            ..make_invariant("inv1", Some("true"))
        };
        let inv2 = InvariantConfig {
            check_at: vec![InvariantCheckPoint::BeforeCycle],
            ..make_invariant("inv2", Some("true"))
        };
        let results = evaluate_invariants(
            &[inv1, inv2],
            InvariantCheckPoint::BeforeCycle,
            Path::new("/tmp"),
        )
        .expect("evaluate");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_invariant_with_assert_expr_passing() {
        let inv = InvariantConfig {
            assert_expr: Some("exit_code == 0".to_string()),
            ..make_invariant("assert_pass", Some("true"))
        };
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(result.passed);
    }

    #[test]
    fn test_invariant_with_assert_expr_failing() {
        let inv = InvariantConfig {
            assert_expr: Some("exit_code != 0".to_string()),
            ..make_invariant("assert_fail", Some("true"))
        };
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(!result.passed);
        assert!(result.message.contains("assertion failed"));
    }

    #[test]
    fn test_invariant_command_error_reported() {
        let inv = make_invariant("bad_cmd", Some("/nonexistent/binary/xyz_12345"));
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(!result.passed);
        // Command itself fails to execute (not just non-zero exit)
        // On macOS sh -c with a bad binary returns exit 127, which doesn't match expect_exit=0
        assert!(!result.message.is_empty());
    }

    #[test]
    fn test_file_matches_pattern_no_wildcard_exact() {
        assert!(file_matches_pattern("main.rs", "main.rs"));
        assert!(!file_matches_pattern("other.rs", "main.rs"));
    }

    #[test]
    fn test_invariant_on_violation_preserved() {
        let inv = InvariantConfig {
            on_violation: OnViolation::Warn,
            ..make_invariant("warn_inv", Some("false"))
        };
        let result = run_single_invariant(&inv, Path::new("/tmp")).expect("run invariant");
        assert!(!result.passed);
        assert_eq!(result.on_violation, OnViolation::Warn);
    }
}
