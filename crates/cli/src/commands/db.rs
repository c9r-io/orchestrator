use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use serde_json::json;
use tonic::transport::Channel;

use crate::{DbCommands, DbMigrationCommands, OutputFormat};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: DbCommands,
) -> Result<()> {
    match cmd {
        DbCommands::Status { output } => {
            let resp = client
                .db_status(orchestrator_proto::DbStatusRequest {})
                .await?
                .into_inner();
            print_status(&resp, output)
        }
        DbCommands::Migrations(DbMigrationCommands::List { output }) => {
            let resp = client
                .db_migrations_list(orchestrator_proto::DbMigrationsListRequest {})
                .await?
                .into_inner();
            print_migrations(&resp, output)
        }
        DbCommands::Vacuum => {
            let resp = client
                .db_vacuum(orchestrator_proto::DbVacuumRequest {})
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
        DbCommands::Cleanup { older_than_days } => {
            let resp = client
                .db_log_cleanup(orchestrator_proto::DbLogCleanupRequest {
                    older_than_days,
                })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }
    }
}

fn print_status(resp: &orchestrator_proto::DbStatusResponse, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "db_path": resp.db_path,
                    "current_version": resp.current_version,
                    "target_version": resp.target_version,
                    "pending_versions": resp.pending_versions,
                    "pending_names": resp.pending_names,
                    "is_current": resp.is_current,
                    "db_size_bytes": resp.db_size_bytes,
                    "logs_size_bytes": resp.logs_size_bytes,
                    "archive_size_bytes": resp.archive_size_bytes,
                }))?
            );
            Ok(())
        }
        OutputFormat::Table => {
            println!("DB Path:          {}", resp.db_path);
            println!("Current Version:  {}", resp.current_version);
            println!("Target Version:   {}", resp.target_version);
            println!(
                "Is Current:       {}",
                if resp.is_current { "yes" } else { "no" }
            );
            if resp.pending_versions.is_empty() {
                println!("Pending:          none");
            } else {
                println!(
                    "Pending Versions: {}",
                    resp.pending_versions
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                println!("Pending Names:    {}", resp.pending_names.join(", "));
            }
            println!();
            println!("DB Size:          {}", format_bytes(resp.db_size_bytes));
            println!("Logs Size:        {}", format_bytes(resp.logs_size_bytes));
            println!("Archive Size:     {}", format_bytes(resp.archive_size_bytes));
            Ok(())
        }
        OutputFormat::Yaml => anyhow::bail!("db commands support only table or json output"),
    }
}

fn print_migrations(
    resp: &orchestrator_proto::DbMigrationsListResponse,
    output: OutputFormat,
) -> Result<()> {
    match output {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "db_path": resp.db_path,
                    "current_version": resp.current_version,
                    "target_version": resp.target_version,
                    "migrations": resp.migrations.iter().map(|migration| json!({
                        "version": migration.version,
                        "name": migration.name,
                        "applied": migration.applied,
                    })).collect::<Vec<_>>(),
                }))?
            );
            Ok(())
        }
        OutputFormat::Table => {
            println!("DB Path:          {}", resp.db_path);
            println!("Current Version:  {}", resp.current_version);
            println!("Target Version:   {}", resp.target_version);
            println!();
            println!("{:<8} {:<10} NAME", "VERSION", "STATE");
            for migration in &resp.migrations {
                println!(
                    "{:<8} {:<10} {}",
                    migration.version,
                    if migration.applied {
                        "applied"
                    } else {
                        "pending"
                    },
                    migration.name
                );
            }
            Ok(())
        }
        OutputFormat::Yaml => anyhow::bail!("db commands support only table or json output"),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_status_accepts_json_output() {
        let resp = orchestrator_proto::DbStatusResponse {
            db_path: "data/agent_orchestrator.db".into(),
            current_version: 1,
            target_version: 2,
            pending_versions: vec![2],
            pending_names: vec!["m0002".into()],
            is_current: false,
            db_size_bytes: 1048576,
            logs_size_bytes: 2097152,
            archive_size_bytes: 0,
        };

        print_status(&resp, OutputFormat::Json).expect("print json");
    }

    #[test]
    fn print_migrations_accepts_table_output() {
        let resp = orchestrator_proto::DbMigrationsListResponse {
            db_path: "data/agent_orchestrator.db".into(),
            current_version: 1,
            target_version: 2,
            migrations: vec![
                orchestrator_proto::DbMigration {
                    version: 1,
                    name: "m0001".into(),
                    applied: true,
                },
                orchestrator_proto::DbMigration {
                    version: 2,
                    name: "m0002".into(),
                    applied: false,
                },
            ],
        };

        print_migrations(&resp, OutputFormat::Table).expect("print table");
    }
}
