//! Database maintenance utilities: VACUUM and size reporting.

use anyhow::Result;
use std::path::Path;

/// Result of a VACUUM operation.
pub struct VacuumResult {
    /// Database file size before VACUUM (bytes).
    pub size_before: u64,
    /// Database file size after VACUUM (bytes).
    pub size_after: u64,
}

/// Size information for the data directory.
pub struct SizeInfo {
    /// SQLite database file size (bytes).
    pub db_size: u64,
    /// Total size of log files (bytes).
    pub logs_size: u64,
    /// Total size of event archive files (bytes).
    pub archive_size: u64,
}

/// Execute `VACUUM` on the database to reclaim disk space.
///
/// Note: VACUUM temporarily requires up to 2x the database size in free
/// disk space because SQLite creates a temporary copy.
pub fn vacuum_database(db_path: &Path) -> Result<VacuumResult> {
    let size_before = db_path
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    let conn = rusqlite::Connection::open(db_path)?;
    conn.execute_batch("VACUUM")?;
    drop(conn);

    let size_after = db_path
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(VacuumResult {
        size_before,
        size_after,
    })
}

/// Compute size information for the data directory components.
pub fn database_size_info(
    db_path: &Path,
    logs_dir: &Path,
    archive_dir: Option<&Path>,
) -> Result<SizeInfo> {
    let db_size = db_path.metadata().map(|m| m.len()).unwrap_or(0);
    // Include WAL and SHM files.
    let wal = db_path.with_extension("db-wal");
    let shm = db_path.with_extension("db-shm");
    let db_size = db_size
        + wal.metadata().map(|m| m.len()).unwrap_or(0)
        + shm.metadata().map(|m| m.len()).unwrap_or(0);

    let logs_size = dir_size_recursive(logs_dir);
    let archive_size = archive_dir.map(dir_size_recursive).unwrap_or(0);

    Ok(SizeInfo {
        db_size,
        logs_size,
        archive_size,
    })
}

/// Recursively compute the total size of a directory.
fn dir_size_recursive(path: &Path) -> u64 {
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_file() {
                total += meta.len();
            } else if meta.is_dir() {
                total += dir_size_recursive(&entry.path());
            }
        }
    }
    total
}
