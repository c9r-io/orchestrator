mod check;
mod definition;
mod edit;
mod output;
mod parse;
mod qa;
mod resource;
mod system;
mod task;
mod task_exec;
mod task_session;
mod task_worker;

use crate::cli::{Cli, Commands};
use crate::state::InnerState;
use anyhow::Result;
use std::sync::{Arc, OnceLock};

pub(super) fn cli_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to initialize shared tokio runtime for CLI")
    })
}

pub struct CliHandler {
    state: Arc<InnerState>,
}

impl CliHandler {
    pub fn new(state: Arc<InnerState>) -> Self {
        Self { state }
    }

    pub fn execute(&self, cli: &Cli) -> Result<i32> {
        match &cli.command {
            Commands::Init { .. } => {
                // Init command is handled in main.rs before reaching here
                Ok(0)
            }
            Commands::Apply { .. } => {
                unreachable!("apply is handled as a preflight command in main.rs")
            }
            Commands::Get {
                resource,
                output,
                selector,
            } => self.handle_get(resource, *output, selector.as_deref()),
            Commands::Describe { resource, output } => self.handle_describe(resource, *output),
            Commands::Delete { resource, force } => self.handle_delete(resource, *force),
            Commands::Task(cmd) => self.handle_task(cmd),
            Commands::Workspace(cmd) => self.handle_workspace(cmd),
            Commands::Agent(cmd) => self.handle_agent(cmd),
            Commands::Workflow(cmd) => self.handle_workflow(cmd),
            Commands::Manifest(cmd) => self.handle_manifest(cmd),
            Commands::Edit(cmd) => self.handle_edit(cmd),
            Commands::Db(cmd) => self.handle_db(cmd),
            Commands::Qa(cmd) => self.handle_qa(cmd),
            Commands::Completion(cmd) => self.handle_completion(cmd),
            Commands::Debug { component } => self.handle_debug(component.as_deref()),
            Commands::Exec {
                stdin,
                tty,
                target,
                command,
            } => self.handle_exec(*stdin, *tty, target, command),
            Commands::Verify(cmd) => self.handle_verify(cmd),
            Commands::Check { workflow, output } => {
                self.handle_check(workflow.as_deref(), *output)
            }
        }
    }
}
