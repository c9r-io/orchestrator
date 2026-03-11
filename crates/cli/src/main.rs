#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]

mod cli;
mod client;
mod commands;
mod output;

use anyhow::{Context, Result};
use clap::Parser;

pub use cli::{
    Cli, Commands, DebugCommands, ManifestCommands, OutputFormat, SandboxProbeCommands,
    StoreCommands, TaskCommands,
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
    unsafe {
        // Restore default SIGPIPE handling so piped CLI output exits quietly
        // when the downstream reader closes early (`head`, `grep -m1`, etc.).
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
        command => {
            let mut client = client::connect(control_plane_config.as_deref()).await?;
            commands::dispatch(&mut client, command).await
        }
    }
}
