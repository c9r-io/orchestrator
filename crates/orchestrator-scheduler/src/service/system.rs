use crate::scheduler::check::{run_checks, CheckReport, CheckResult};
use agent_orchestrator::config_load::read_active_config;
use agent_orchestrator::error::Result;
use agent_orchestrator::state::InnerState;

/// Rendered result of `orchestrator check`.
#[derive(Debug, Clone)]
pub struct RenderedCheckReport {
    /// Structured report returned by the scheduler checks.
    pub report: CheckReport,
    /// Human-readable report body.
    pub content: String,
    /// Process exit code recommended for the check result.
    pub exit_code: i32,
}

/// Run preflight checks. Returns (content, exit_code).
/// When `project_id` is Some, checks are scoped to that project's resources.
pub fn run_check(
    state: &InnerState,
    workflow: Option<&str>,
    output_format: &str,
    project_id: Option<&str>,
) -> Result<RenderedCheckReport> {
    let active = read_active_config(state)?;
    let report = run_checks(&active, &state.data_dir, workflow, project_id);

    let content = match output_format {
        "json" => serde_json::to_string_pretty(&report)?,
        "yaml" => serde_yaml::to_string(&report)?,
        _ => {
            let mut buf = String::new();
            buf.push_str("orchestrator check \u{2014} preflight validation\n\n");
            for check in &report.checks {
                let icon = if check.passed {
                    "\u{2713}"
                } else {
                    match check.severity {
                        agent_orchestrator::anomaly::Severity::Error => "\u{2717}",
                        agent_orchestrator::anomaly::Severity::Warning => "\u{26a0}",
                        agent_orchestrator::anomaly::Severity::Info => "\u{2139}",
                    }
                };
                buf.push_str(&format!("{} [{}] {}\n", icon, check.rule, check.message));
                if let Some(actual) = check.actual.as_deref() {
                    buf.push_str(&format!("  actual: {actual}\n"));
                }
                if let Some(expected) = check.expected.as_deref() {
                    buf.push_str(&format!("  expected: {expected}\n"));
                }
                if let Some(risk) = check.risk.as_deref() {
                    buf.push_str(&format!("  risk: {risk}\n"));
                }
                if let Some(fix) = check.suggested_fix.as_deref() {
                    buf.push_str(&format!("  suggested_fix: {fix}\n"));
                }
            }
            buf.push_str(&format!(
                "\n{} passed, {} errors, {} warnings\n",
                report.summary.passed, report.summary.errors, report.summary.warnings
            ));
            buf
        }
    };

    let exit_code = if report.summary.errors > 0 { 1 } else { 0 };
    Ok(RenderedCheckReport {
        report,
        content,
        exit_code,
    })
}

/// Converts a preflight check result into the protobuf diagnostic entry shape.
pub fn diagnostic_entry_from_check(check: &CheckResult) -> orchestrator_proto::DiagnosticEntry {
    orchestrator_proto::DiagnosticEntry {
        source: check.source.clone(),
        rule: check.rule.clone(),
        severity: format!("{:?}", check.severity).to_lowercase(),
        passed: check.passed,
        blocking: check.blocking,
        message: check.message.clone(),
        context: check.context.clone(),
        scope: check.scope.clone(),
        actual: check.actual.clone(),
        expected: check.expected.clone(),
        risk: check.risk.clone(),
        suggested_fix: check.suggested_fix.clone(),
    }
}
