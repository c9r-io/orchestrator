use super::OutputFormat;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum QaCommands {
    #[command(subcommand)]
    Project(QaProjectCommands),

    /// Validate QA concurrency guardrails and sqlite settings
    Doctor {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum QaProjectCommands {
    /// Create or update an isolated QA project scaffold
    Create {
        #[arg(value_name = "PROJECT_ID")]
        project_id: String,

        #[arg(long, default_value = "default")]
        from_workspace: String,

        #[arg(long)]
        workflow: Option<String>,

        #[arg(long)]
        workspace: Option<String>,

        #[arg(long)]
        root_path: Option<String>,

        #[arg(long = "qa-target")]
        qa_target: Vec<String>,

        #[arg(long, default_value = "docs/ticket")]
        ticket_dir: String,

        #[arg(short, long)]
        force: bool,
    },

    /// Reset one QA project data/resources without deleting the sqlite database
    Reset {
        #[arg(value_name = "PROJECT_ID")]
        project_id: String,

        /// Keep project config and only clean task/runtime rows
        #[arg(long)]
        keep_config: bool,

        #[arg(short, long)]
        force: bool,
    },
}
