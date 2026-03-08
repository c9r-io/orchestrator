use crate::config_load::read_active_config;
use crate::scheduler::check::run_checks;
use crate::scheduler_service::{pending_task_count, worker_stop_signal_path};
use crate::state::InnerState;
use anyhow::Result;

/// Get debug information for a component.
pub fn debug_info(state: &InnerState, component: Option<&str>) -> Result<String> {
    let comp = component.unwrap_or("state");
    match comp {
        "state" => Ok(
            "Debug Information\n=================\n\nAvailable: state, config, messagebus\n"
                .to_string(),
        ),
        "config" => {
            let config = read_active_config(state)?;
            Ok(format!(
                "Active Configuration:\n{}",
                serde_yml::to_string(&config.config).unwrap_or_default()
            ))
        }
        "messagebus" => Ok(
            "MessageBus Debug Information\n============================\n\nMessageBus is an internal component.\n"
                .to_string(),
        ),
        _ => Ok(format!("Unknown debug component: {}\nAvailable: state, config, messagebus\n", comp)),
    }
}

/// Get worker status.
pub async fn worker_status(
    state: &InnerState,
) -> Result<orchestrator_proto::WorkerStatusResponse> {
    let pending = pending_task_count(state).await?;
    let stop_signal = worker_stop_signal_path(state).exists();

    Ok(orchestrator_proto::WorkerStatusResponse {
        pending_tasks: pending,
        stop_signal,
        active_workers: 0, // TODO: track active worker count
    })
}

/// Run preflight checks. Returns (content, exit_code).
pub fn run_check(
    state: &InnerState,
    workflow: Option<&str>,
    output_format: &str,
) -> Result<(String, i32)> {
    let active = read_active_config(state)?;
    let report = run_checks(&active, &state.app_root, workflow);

    let content = match output_format {
        "json" => serde_json::to_string_pretty(&report)?,
        "yaml" => serde_yml::to_string(&report)?,
        _ => {
            let mut buf = String::new();
            buf.push_str("orchestrator check — preflight validation\n\n");
            for check in &report.checks {
                let icon = if check.passed {
                    "\u{2713}"
                } else {
                    match check.severity {
                        crate::anomaly::Severity::Error => "\u{2717}",
                        crate::anomaly::Severity::Warning => "\u{26a0}",
                        crate::anomaly::Severity::Info => "\u{2139}",
                    }
                };
                buf.push_str(&format!("{} {}\n", icon, check.message));
            }
            buf.push_str(&format!(
                "\n{} passed, {} errors, {} warnings\n",
                report.summary.passed, report.summary.errors, report.summary.warnings
            ));
            buf
        }
    };

    let exit_code = if report.summary.errors > 0 { 1 } else { 0 };
    Ok((content, exit_code))
}
