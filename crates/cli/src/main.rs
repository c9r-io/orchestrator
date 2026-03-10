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
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;

    rt.block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Version => commands::version::run().await,
        Commands::Debug {
            component: _,
            command: Some(debug_command),
        } => commands::debug::run_local(debug_command).await,
        command => {
            let mut client = client::connect().await?;
            commands::dispatch(&mut client, command).await
        }
    }
}
