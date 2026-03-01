use crate::cli::OutputFormat;
use crate::config_load::read_active_config;
use crate::scheduler::check::{run_checks, CheckReport};
use crate::scheduler::trace::Severity;
use anyhow::Result;

use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_check(&self, workflow: Option<&str>, output: OutputFormat) -> Result<i32> {
        let active = read_active_config(&self.state)?;
        let report = run_checks(&active, &self.state.app_root, workflow);

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

#[cfg(test)]
mod tests {
    use super::super::CliHandler;
    use crate::cli::{Cli, Commands, OutputFormat};
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
}
