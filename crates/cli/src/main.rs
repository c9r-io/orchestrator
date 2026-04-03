//! Command-line client for the Agent Orchestrator daemon and control plane.
//!
//! This binary exposes task, resource, and debugging workflows over gRPC.
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

mod cli;
mod client;
mod commands;
mod output;

use anyhow::{Context, Result};
use clap::Parser;

/// Re-exported CLI argument model for integration tests and helper modules.
pub use cli::{
    AgentCommands, Cli, Commands, DaemonCommands, DbCommands, DbMigrationCommands, DebugCommands,
    EventCommands, ManifestCommands, OutputFormat, QaCommands, SandboxProbeCommands,
    SecretCommands, SecretKeyCommands, StoreCommands, TaskCommands, ToolCommands, TriggerCommands,
};

fn main() -> Result<()> {
    configure_sigpipe();

    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;

    rt.block_on(run(cli))
}

fn configure_sigpipe() {
    #[cfg(unix)]
    // SAFETY: `libc::signal` is a POSIX-standard function. Called before the
    // async runtime starts, so no signal-handler races are possible. Restoring
    // SIGPIPE to SIG_DFL is a well-defined operation with no preconditions.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let Cli {
        command,
        control_plane_config,
        ..
    } = cli;
    match command {
        Commands::Version { json } => {
            commands::version::run(control_plane_config.as_deref(), json).await
        }
        Commands::Debug {
            component: _,
            command: Some(debug_command),
        } => commands::debug::run_local(debug_command).await,
        Commands::Daemon(cmd) => commands::daemon::dispatch(cmd).await,
        Commands::Tool(cmd) => commands::tool::dispatch(cmd, control_plane_config.as_deref()).await,
        command => {
            let mut client = client::connect(control_plane_config.as_deref()).await?;
            commands::dispatch(&mut client, command).await
        }
    }
}
