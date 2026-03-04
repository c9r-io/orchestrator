use crate::cli::{generate_completion, CompletionCommands, DbCommands, VerifyCommands};
use crate::cli_handler::cli_runtime;
use crate::config_load::read_active_config;
use crate::db::reset_db;
use crate::scheduler::safety::verify_binary_snapshot;
use anyhow::Result;
use clap_complete::Shell;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_mock_file(path: &std::path::Path, content: &[u8]) {
        let parent = path.parent().expect("mock file parent");
        std::fs::create_dir_all(parent).expect("create parent dirs");
        std::fs::write(path, content).expect("write mock file");
    }

    #[tokio::test]
    async fn test_handle_verify_binary_snapshot() {
        let temp_dir = std::env::temp_dir().join(format!(
            "system-test-verify-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let content = b"mock binary for handle_verify test";
        let binary_path = temp_dir.join("core/target/release/agent-orchestrator");
        let stable_path = temp_dir.join(".stable");
        create_mock_file(&binary_path, content);
        create_mock_file(&stable_path, content);

        let result = verify_binary_snapshot(&temp_dir)
            .await
            .expect("verify_binary_snapshot should succeed");

        assert!(result.verified, "matching files should be verified");

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}

use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_debug(&self, component: Option<&str>) -> Result<i32> {
        let comp = component.unwrap_or("state");

        match comp {
            "state" => {
                println!("Debug Information");
                println!("=================");
                println!();
                println!("Note: MessageBus is an internal component.");
                println!("Use 'orchestrator task list' and 'orchestrator task logs' for runtime debugging.");
                println!();
                println!("Available debug components:");
                println!("  state     - Show runtime state info (this)");
                println!("  config    - Show active configuration");
                println!("  messagebus - Show MessageBus status (internal)");
                Ok(0)
            }
            "config" => {
                let config = read_active_config(&self.state)?;
                println!("Active Configuration:");
                println!(
                    "{}",
                    serde_yaml::to_string(&config.config).unwrap_or_default()
                );
                Ok(0)
            }
            "messagebus" => {
                println!("MessageBus Debug Information");
                println!("============================");
                println!();
                println!("MessageBus is an internal component for agent-to-agent communication.");
                println!(
                    "It is initialized in InnerState and used for publishing/subscribing messages."
                );
                println!();
                println!("Implementation location: src/collab.rs (MessageBus)");
                println!();
                println!("To verify MessageBus is working:");
                println!("  1. Run a task with multiple agents");
                println!("  2. Check logs for message_bus events");
                Ok(0)
            }
            _ => {
                eprintln!("Unknown debug component: {}", comp);
                eprintln!("Available: state, config, messagebus");
                Ok(1)
            }
        }
    }

    pub(super) fn handle_db(&self, cmd: &DbCommands) -> Result<i32> {
        match cmd {
            DbCommands::Reset {
                force,
                include_history,
                include_config,
            } => {
                if !force {
                    eprintln!("Use --force to confirm database reset");
                    return Ok(1);
                }
                reset_db(&self.state, *include_history, *include_config)?;
                println!("Database reset completed");
                if *include_config {
                    println!("All config versions deleted (next apply starts from blank)");
                } else if *include_history {
                    println!("Config version history cleared (active version preserved)");
                }
                Ok(0)
            }
        }
    }

    pub(super) fn handle_completion(&self, cmd: &CompletionCommands) -> Result<i32> {
        let shell = match cmd {
            CompletionCommands::Bash => Shell::Bash,
            CompletionCommands::Zsh => Shell::Zsh,
            CompletionCommands::Fish => Shell::Fish,
            CompletionCommands::PowerShell => Shell::PowerShell,
        };
        generate_completion(shell);
        Ok(0)
    }

    pub(super) fn handle_verify(&self, cmd: &VerifyCommands) -> Result<i32> {
        match cmd {
            VerifyCommands::BinarySnapshot { root } => {
                let workspace_root = match root {
                    Some(path) => PathBuf::from(path),
                    None => std::env::current_dir()?,
                };

                let rt = cli_runtime();
                let result = rt.block_on(verify_binary_snapshot(&workspace_root))?;

                if result.verified {
                    println!("✓ Binary snapshot verified");
                } else {
                    println!("✗ Binary snapshot MISMATCH");
                }
                println!("  Original (stable): {}", result.original_checksum);
                println!("  Current (release): {}", result.current_checksum);
                println!("  Stable path: {}", result.stable_path.display());
                println!("  Binary path: {}", result.binary_path.display());
                if let Some(ref manifest) = result.manifest {
                    println!("  Manifest:");
                    println!("    task_id:    {}", manifest.task_id);
                    println!("    cycle:      {}", manifest.cycle);
                    println!("    created_at: {}", manifest.created_at);
                    println!("    size_bytes: {}", manifest.size_bytes);
                }
                if result.verified {
                    Ok(0)
                } else {
                    eprintln!("\nTo restore the stable binary, run:");
                    eprintln!(
                        "  cp {} {}",
                        result.stable_path.display(),
                        result.binary_path.display()
                    );
                    Ok(1)
                }
            }
        }
    }
}
