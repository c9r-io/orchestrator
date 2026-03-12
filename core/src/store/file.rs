//! File store backend — filesystem-based persistent store.

use crate::store::{StoreEntry, StoreOp, StoreOpResult};
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Filesystem-backed workflow store implementation.
pub struct FileStoreBackend {
    app_root: PathBuf,
}

impl FileStoreBackend {
    /// Creates a file-store backend rooted at the orchestrator app directory.
    pub fn new(app_root: PathBuf) -> Self {
        Self { app_root }
    }

    /// Executes a store operation against JSON files on disk.
    pub async fn execute(&self, op: StoreOp) -> Result<StoreOpResult> {
        match op {
            StoreOp::Get {
                store_name,
                project_id,
                key,
            } => self.get(&store_name, &project_id, &key),
            StoreOp::Put {
                store_name,
                project_id,
                key,
                value,
                ..
            } => self.put(&store_name, &project_id, &key, &value),
            StoreOp::Delete {
                store_name,
                project_id,
                key,
            } => self.delete(&store_name, &project_id, &key),
            StoreOp::List {
                store_name,
                project_id,
                limit,
                offset,
            } => self.list(&store_name, &project_id, limit, offset),
            StoreOp::Prune {
                store_name,
                project_id,
                max_entries,
                ..
            } => self.prune(&store_name, &project_id, max_entries),
        }
    }

    fn store_dir(&self, store_name: &str, project_id: &str) -> PathBuf {
        let pid = if project_id.trim().is_empty() {
            crate::config::DEFAULT_PROJECT_ID
        } else {
            project_id
        };
        self.app_root
            .join("data")
            .join("stores")
            .join(store_name)
            .join(pid)
    }

    fn entry_path(&self, store_name: &str, project_id: &str, key: &str) -> PathBuf {
        self.store_dir(store_name, project_id)
            .join(format!("{}.json", key))
    }

    fn get(&self, store_name: &str, project_id: &str, key: &str) -> Result<StoreOpResult> {
        let path = self.entry_path(store_name, project_id, key);
        if !path.exists() {
            return Ok(StoreOpResult::Value(None));
        }
        let content = std::fs::read_to_string(&path).context("failed to read store entry file")?;
        let value: serde_json::Value =
            serde_json::from_str(&content).context("failed to parse store entry JSON")?;
        Ok(StoreOpResult::Value(Some(value)))
    }

    fn put(
        &self,
        store_name: &str,
        project_id: &str,
        key: &str,
        value: &str,
    ) -> Result<StoreOpResult> {
        let path = self.entry_path(store_name, project_id, key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("failed to create store directory")?;
        }
        std::fs::write(&path, value).context("failed to write store entry file")?;
        Ok(StoreOpResult::Ok)
    }

    fn delete(&self, store_name: &str, project_id: &str, key: &str) -> Result<StoreOpResult> {
        let path = self.entry_path(store_name, project_id, key);
        if path.exists() {
            std::fs::remove_file(&path).context("failed to delete store entry file")?;
        }
        Ok(StoreOpResult::Ok)
    }

    fn list(
        &self,
        store_name: &str,
        project_id: &str,
        limit: u64,
        offset: u64,
    ) -> Result<StoreOpResult> {
        let dir = self.store_dir(store_name, project_id);
        if !dir.exists() {
            return Ok(StoreOpResult::Entries(vec![]));
        }

        let mut entries: Vec<StoreEntry> = Vec::new();
        let read_dir = std::fs::read_dir(&dir).context("failed to read store directory")?;

        let mut files: Vec<_> = read_dir
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();

        // Sort by modification time (newest first)
        files.sort_by(|a, b| {
            let t_a = a.metadata().and_then(|m| m.modified()).ok();
            let t_b = b.metadata().and_then(|m| m.modified()).ok();
            t_b.cmp(&t_a)
        });

        for entry in files.into_iter().skip(offset as usize).take(limit as usize) {
            let path = entry.path();
            let key = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let updated_at = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| {
                    let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                    Some(
                        chrono::DateTime::from_timestamp(duration.as_secs() as i64, 0)?
                            .to_rfc3339(),
                    )
                })
                .unwrap_or_default();

            entries.push(StoreEntry {
                key,
                value,
                updated_at,
            });
        }

        Ok(StoreOpResult::Entries(entries))
    }

    fn prune(
        &self,
        store_name: &str,
        project_id: &str,
        max_entries: Option<u64>,
    ) -> Result<StoreOpResult> {
        let dir = self.store_dir(store_name, project_id);
        if !dir.exists() {
            return Ok(StoreOpResult::Ok);
        }

        if let Some(max) = max_entries {
            let read_dir = std::fs::read_dir(&dir).context("failed to read store directory")?;
            let mut files: Vec<_> = read_dir
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .collect();

            files.sort_by(|a, b| {
                let t_a = a.metadata().and_then(|m| m.modified()).ok();
                let t_b = b.metadata().and_then(|m| m.modified()).ok();
                t_b.cmp(&t_a)
            });

            // Remove entries beyond max
            for entry in files.into_iter().skip(max as usize) {
                let _ = std::fs::remove_file(entry.path());
            }
        }

        Ok(StoreOpResult::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_put_get_delete() {
        let temp = tempfile::tempdir().expect("tempdir");
        let backend = FileStoreBackend::new(temp.path().to_path_buf());

        // Put
        let result = backend
            .execute(StoreOp::Put {
                store_name: "metrics".to_string(),
                project_id: "".to_string(),
                key: "k1".to_string(),
                value: r#"{"count": 10}"#.to_string(),
                task_id: "t1".to_string(),
            })
            .await
            .expect("put");
        assert!(matches!(result, StoreOpResult::Ok));

        // Get
        let result = backend
            .execute(StoreOp::Get {
                store_name: "metrics".to_string(),
                project_id: "".to_string(),
                key: "k1".to_string(),
            })
            .await
            .expect("get");
        match result {
            StoreOpResult::Value(Some(v)) => assert_eq!(v["count"], 10),
            other => panic!("expected Value(Some), got {:?}", other),
        }

        // Delete
        let result = backend
            .execute(StoreOp::Delete {
                store_name: "metrics".to_string(),
                project_id: "".to_string(),
                key: "k1".to_string(),
            })
            .await
            .expect("delete");
        assert!(matches!(result, StoreOpResult::Ok));

        // Get after delete
        let result = backend
            .execute(StoreOp::Get {
                store_name: "metrics".to_string(),
                project_id: "".to_string(),
                key: "k1".to_string(),
            })
            .await
            .expect("get after delete");
        assert!(matches!(result, StoreOpResult::Value(None)));
    }

    #[tokio::test]
    async fn file_list_entries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let backend = FileStoreBackend::new(temp.path().to_path_buf());

        backend
            .execute(StoreOp::Put {
                store_name: "s".to_string(),
                project_id: "p".to_string(),
                key: "a".to_string(),
                value: r#"{"v": 1}"#.to_string(),
                task_id: "".to_string(),
            })
            .await
            .expect("put a");
        backend
            .execute(StoreOp::Put {
                store_name: "s".to_string(),
                project_id: "p".to_string(),
                key: "b".to_string(),
                value: r#"{"v": 2}"#.to_string(),
                task_id: "".to_string(),
            })
            .await
            .expect("put b");

        let result = backend
            .execute(StoreOp::List {
                store_name: "s".to_string(),
                project_id: "p".to_string(),
                limit: 100,
                offset: 0,
            })
            .await
            .expect("list");
        match result {
            StoreOpResult::Entries(entries) => assert_eq!(entries.len(), 2),
            other => panic!("expected Entries, got {:?}", other),
        }
    }
}
