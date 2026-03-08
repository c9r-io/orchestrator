use super::OutputFormat;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum WorkspaceCommands {
    #[command(alias = "ls")]
    List {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },

    #[command(alias = "get")]
    Info {
        #[arg(value_name = "WORKSPACE_ID")]
        workspace_id: String,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    Create {
        #[arg(value_name = "NAME")]
        name: String,

        #[arg(long = "root-path")]
        root_path: String,

        #[arg(long = "qa-target")]
        qa_target: Vec<String>,

        #[arg(long = "ticket-dir", default_value = "docs/ticket")]
        ticket_dir: String,

        #[arg(long = "label")]
        labels: Vec<String>,

        #[arg(long = "annotation")]
        annotations: Vec<String>,

        #[arg(long)]
        dry_run: bool,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum AgentCommands {
    Create {
        #[arg(value_name = "NAME")]
        name: String,

        #[arg(long = "command")]
        command: String,

        #[arg(long = "capability")]
        capability: Vec<String>,

        #[arg(long = "label")]
        labels: Vec<String>,

        #[arg(long = "annotation")]
        annotations: Vec<String>,

        #[arg(long)]
        dry_run: bool,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum WorkflowCommands {
    Create {
        #[arg(value_name = "NAME")]
        name: String,

        #[arg(long = "step", required = true)]
        step: Vec<String>,

        #[arg(long = "loop-mode", default_value = "once")]
        loop_mode: String,

        #[arg(long = "max-cycles")]
        max_cycles: Option<u32>,

        #[arg(long = "label")]
        labels: Vec<String>,

        #[arg(long = "annotation")]
        annotations: Vec<String>,

        #[arg(long)]
        dry_run: bool,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum EditCommands {
    #[command(alias = "ex")]
    Export {
        #[arg(value_name = "RESOURCE")]
        selector: String,
    },

    #[command(alias = "op")]
    Open {
        #[arg(value_name = "RESOURCE")]
        selector: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ManifestCommands {
    Validate {
        #[arg(short = 'f', long = "file")]
        file: String,
    },

    Export {
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,

        #[arg(short = 'f', long = "file")]
        file: Option<String>,
    },
}
