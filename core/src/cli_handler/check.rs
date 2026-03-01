use crate::cli::OutputFormat;
use crate::config_load::{read_active_config, ConfigSelfHealReport};
use crate::scheduler::check::{run_checks, CheckReport};
use crate::scheduler::trace::Severity;
use anyhow::Result;

use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_check(&self, workflow: Option<&str>, output: OutputFormat) -> Result<i32> {
        let active = read_active_config(&self.state)?;
        let mut report = run_checks(&active, &self.state.app_root, workflow);
        if let Ok(notice) = self.state.active_config_notice.read() {
            if let Some(notice) = notice.as_ref() {
                append_active_config_notice(&mut report, notice);
            }
        }

        match output {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(&report).unwrap());
            }
            OutputFormat::Table => {
                self.render_check_table(&report);
            }
        }

        if report.summary.errors > 0 {
            Ok(1)
        } else {
            Ok(0)
        }
    }

    fn render_check_table(&self, report: &CheckReport) {
        println!("orchestrator check — preflight validation\n");

        for check in &report.checks {
            let icon = if check.passed {
                "\u{2713}" // ✓
            } else {
                match check.severity {
                    Severity::Error => "\u{2717}",   // ✗
                    Severity::Warning => "\u{26a0}", // ⚠
                    Severity::Info => "\u{2139}",    // ℹ
                }
            };
            println!("{} {}", icon, check.message);
        }

        println!(
            "\n{} passed, {} errors, {} warnings",
            report.summary.passed, report.summary.errors, report.summary.warnings
        );
    }
}

fn append_active_config_notice(report: &mut CheckReport, notice: &ConfigSelfHealReport) {
    report.checks.push(crate::scheduler::check::CheckResult {
        rule: "config_auto_healed".into(),
        severity: Severity::Warning,
        passed: false,
        message: format!(
            "active config was auto-healed from persisted drift ({} changes, version {})",
            notice.changes.len(),
            notice.healed_version
        ),
        context: Some(notice.original_error.clone()),
    });
    report.summary.total += 1;
    report.summary.warnings += 1;
}

#[cfg(test)]
mod tests {
    use super::super::CliHandler;
    use super::append_active_config_notice;
    use crate::cli::{Cli, Commands, OutputFormat};
    use crate::config_load::{ConfigSelfHealChange, ConfigSelfHealReport, ConfigSelfHealRule};
    use crate::scheduler::check::{CheckReport, CheckSummary};
    use crate::scheduler::trace::Severity;
    use crate::test_utils::TestState;

    #[test]
    fn check_default_returns_success() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state);

        let cli = Cli {
            command: Commands::Check {
                workflow: None,
                output: OutputFormat::Table,
            },
            verbose: false,
        };

        assert_eq!(handler.execute(&cli).expect("check should succeed"), 0);
    }

    #[test]
    fn check_json_output_returns_success() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state);

        let cli = Cli {
            command: Commands::Check {
                workflow: None,
                output: OutputFormat::Json,
            },
            verbose: false,
        };

        assert_eq!(handler.execute(&cli).expect("check json should succeed"), 0);
    }

    #[test]
    fn check_with_workflow_filter() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state);

        let cli = Cli {
            command: Commands::Check {
                workflow: Some("self-bootstrap".to_string()),
                output: OutputFormat::Table,
            },
            verbose: false,
        };

        // Should succeed even if workflow doesn't exist (just no checks for it)
        assert_eq!(
            handler
                .execute(&cli)
                .expect("check with workflow filter should succeed"),
            0
        );
    }

    #[test]
    fn append_active_config_notice_adds_warning_check() {
        let mut report = CheckReport {
            checks: Vec::new(),
            summary: CheckSummary {
                total: 0,
                passed: 0,
                errors: 0,
                warnings: 0,
            },
        };
        let notice = ConfigSelfHealReport {
            original_error: "legacy drift".to_string(),
            healed_version: 7,
            healed_at: "2026-01-01T00:00:00Z".to_string(),
            changes: vec![ConfigSelfHealChange {
                workflow_id: "wf".to_string(),
                step_id: "self_test".to_string(),
                rule: ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep,
                detail: "removed legacy required_capability".to_string(),
            }],
        };

        append_active_config_notice(&mut report, &notice);

        assert_eq!(report.summary.total, 1);
        assert_eq!(report.summary.warnings, 1);
        let check = report
            .checks
            .first()
            .expect("expected injected config_auto_healed check");
        assert_eq!(check.rule, "config_auto_healed");
        assert_eq!(check.severity, Severity::Warning);
        assert!(!check.passed);
        assert!(check.message.contains("version 7"));
        assert_eq!(check.context.as_deref(), Some("legacy drift"));
    }
}
