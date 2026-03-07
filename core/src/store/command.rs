//! Command adapter — shell-based generic backend for user-defined providers.

use crate::config::StoreBackendCommands;
use crate::store::{StoreEntry, StoreOp, StoreOpResult};
use anyhow::{anyhow, Result};

pub struct CommandAdapter;

impl CommandAdapter {
    pub async fn execute(
        &self,
        commands: &StoreBackendCommands,
        op: StoreOp,
    ) -> Result<StoreOpResult> {
        let (cmd_template, env_vars, parse_mode) = match &op {
            StoreOp::Get {
                store_name,
                project_id,
                key,
            } => (
                &commands.get,
                vec![
                    ("STORE_NAME", store_name.as_str()),
                    ("PROJECT_ID", project_id.as_str()),
                    ("KEY", key.as_str()),
                ],
                ParseMode::Value,
            ),
            StoreOp::Put {
                store_name,
                project_id,
                key,
                value,
                task_id,
            } => (
                &commands.put,
                vec![
                    ("STORE_NAME", store_name.as_str()),
                    ("PROJECT_ID", project_id.as_str()),
                    ("KEY", key.as_str()),
                    ("VALUE", value.as_str()),
                    ("TASK_ID", task_id.as_str()),
                ],
                ParseMode::None,
            ),
            StoreOp::Delete {
                store_name,
                project_id,
                key,
            } => (
                &commands.delete,
                vec![
                    ("STORE_NAME", store_name.as_str()),
                    ("PROJECT_ID", project_id.as_str()),
                    ("KEY", key.as_str()),
                ],
                ParseMode::None,
            ),
            StoreOp::List {
                store_name,
                project_id,
                ..
            } => (
                &commands.list,
                vec![
                    ("STORE_NAME", store_name.as_str()),
                    ("PROJECT_ID", project_id.as_str()),
                ],
                ParseMode::Entries,
            ),
            StoreOp::Prune {
                store_name,
                project_id,
                ..
            } => {
                let cmd = commands
                    .prune
                    .as_ref()
                    .ok_or_else(|| anyhow!("provider does not support prune operation"))?;
                (
                    cmd,
                    vec![
                        ("STORE_NAME", store_name.as_str()),
                        ("PROJECT_ID", project_id.as_str()),
                    ],
                    ParseMode::None,
                )
            }
        };

        // Build env vars, handling the limit/offset/max_entries/ttl_days cases
        let mut envs: Vec<(String, String)> = env_vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Add numeric env vars for list/prune
        match &op {
            StoreOp::List { limit, offset, .. } => {
                envs.retain(|e| e.0 != "LIMIT" && e.0 != "OFFSET");
                envs.push(("LIMIT".to_string(), limit.to_string()));
                envs.push(("OFFSET".to_string(), offset.to_string()));
            }
            StoreOp::Prune {
                max_entries,
                ttl_days,
                ..
            } => {
                if let Some(max) = max_entries {
                    envs.push(("MAX_ENTRIES".to_string(), max.to_string()));
                }
                if let Some(ttl) = ttl_days {
                    envs.push(("TTL_DAYS".to_string(), ttl.to_string()));
                }
            }
            _ => {}
        }

        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd_template)
            .envs(envs)
            .output()
            .await
            .map_err(|e| anyhow!("failed to execute provider command: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "provider command failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

        match parse_mode {
            ParseMode::Value => {
                if stdout.is_empty() {
                    Ok(StoreOpResult::Value(None))
                } else {
                    let value: serde_json::Value = serde_json::from_str(&stdout)
                        .map_err(|e| anyhow!("failed to parse provider get output: {}", e))?;
                    Ok(StoreOpResult::Value(Some(value)))
                }
            }
            ParseMode::Entries => {
                if stdout.is_empty() {
                    Ok(StoreOpResult::Entries(vec![]))
                } else {
                    let entries: Vec<StoreEntry> = serde_json::from_str(&stdout)
                        .map_err(|e| anyhow!("failed to parse provider list output: {}", e))?;
                    Ok(StoreOpResult::Entries(entries))
                }
            }
            ParseMode::None => Ok(StoreOpResult::Ok),
        }
    }
}

enum ParseMode {
    Value,
    Entries,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StoreBackendCommands;

    #[tokio::test]
    async fn command_adapter_put_get() {
        let temp = tempfile::tempdir().expect("tempdir");
        let base = temp.path().to_str().expect("path");

        let commands = StoreBackendCommands {
            get: format!(
                "cat {}/\"$STORE_NAME\"-\"$KEY\".json 2>/dev/null || true",
                base
            ),
            put: format!("echo \"$VALUE\" > {}/\"$STORE_NAME\"-\"$KEY\".json", base),
            delete: format!("rm -f {}/\"$STORE_NAME\"-\"$KEY\".json", base),
            list: "echo '[]'".to_string(),
            prune: None,
        };

        let adapter = CommandAdapter;

        // Put
        let result = adapter
            .execute(
                &commands,
                StoreOp::Put {
                    store_name: "test".to_string(),
                    project_id: "".to_string(),
                    key: "k1".to_string(),
                    value: r#"{"hello": "world"}"#.to_string(),
                    task_id: "t1".to_string(),
                },
            )
            .await
            .expect("put");
        assert!(matches!(result, StoreOpResult::Ok));

        // Get
        let result = adapter
            .execute(
                &commands,
                StoreOp::Get {
                    store_name: "test".to_string(),
                    project_id: "".to_string(),
                    key: "k1".to_string(),
                },
            )
            .await
            .expect("get");
        match result {
            StoreOpResult::Value(Some(v)) => assert_eq!(v["hello"], "world"),
            other => panic!("expected Value(Some), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn command_adapter_get_missing_returns_none() {
        let commands = StoreBackendCommands {
            get: "echo ''".to_string(),
            put: "true".to_string(),
            delete: "true".to_string(),
            list: "echo '[]'".to_string(),
            prune: None,
        };

        let adapter = CommandAdapter;
        let result = adapter
            .execute(
                &commands,
                StoreOp::Get {
                    store_name: "s".to_string(),
                    project_id: "".to_string(),
                    key: "missing".to_string(),
                },
            )
            .await
            .expect("get");
        assert!(matches!(result, StoreOpResult::Value(None)));
    }

    #[tokio::test]
    async fn command_adapter_prune_unsupported() {
        let commands = StoreBackendCommands {
            get: "true".to_string(),
            put: "true".to_string(),
            delete: "true".to_string(),
            list: "echo '[]'".to_string(),
            prune: None,
        };

        let adapter = CommandAdapter;
        let result = adapter
            .execute(
                &commands,
                StoreOp::Prune {
                    store_name: "s".to_string(),
                    project_id: "".to_string(),
                    max_entries: None,
                    ttl_days: None,
                },
            )
            .await;
        assert!(result.is_err());
    }
}
