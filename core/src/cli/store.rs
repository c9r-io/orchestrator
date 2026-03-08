use super::OutputFormat;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum StoreCommands {
    /// Get a value from a workflow store
    Get {
        /// Store name
        store: String,

        /// Entry key
        key: String,

        /// Project ID
        #[arg(short, long, default_value = "")]
        project: String,
    },

    /// Put a value into a workflow store
    Put {
        /// Store name
        store: String,

        /// Entry key
        key: String,

        /// JSON value
        value: String,

        /// Project ID
        #[arg(short, long, default_value = "")]
        project: String,

        /// Task ID for attribution
        #[arg(short, long, default_value = "")]
        task_id: String,
    },

    /// Delete a value from a workflow store
    Delete {
        /// Store name
        store: String,

        /// Entry key
        key: String,

        /// Project ID
        #[arg(short, long, default_value = "")]
        project: String,
    },

    /// List entries in a workflow store
    #[command(alias = "ls")]
    List {
        /// Store name
        store: String,

        /// Project ID
        #[arg(short, long, default_value = "")]
        project: String,

        /// Maximum entries to return
        #[arg(short, long, default_value = "100")]
        limit: u64,

        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: u64,

        /// Output format
        #[arg(short = 'o', long, default_value = "table")]
        output: OutputFormat,
    },

    /// Prune old entries from a workflow store
    Prune {
        /// Store name
        store: String,

        /// Project ID
        #[arg(short, long, default_value = "")]
        project: String,
    },
}
