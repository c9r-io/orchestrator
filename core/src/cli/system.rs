use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum DbCommands {
    Reset {
        #[arg(short, long)]
        force: bool,

        /// Also clear config version history (preserves current active config)
        #[arg(long)]
        include_history: bool,

        /// Delete ALL config versions (full reset for test isolation)
        #[arg(long)]
        include_config: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum CompletionCommands {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

#[derive(Subcommand, Debug, Clone)]
pub enum VerifyCommands {
    /// Verify binary snapshot matches current binary (survival smoke test)
    BinarySnapshot {
        /// Workspace root path (default: current directory)
        #[arg(short, long)]
        root: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigLifecycleCommands {
    /// Show self-heal audit log
    HealLog {
        /// Maximum number of entries to display
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Backfill missing step_scope in legacy event payloads
    BackfillEvents {
        /// Force backfill without confirmation (bulk database UPDATE)
        #[arg(short, long)]
        force: bool,
    },
}
