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
    let size_before = db_path.metadata().map(|m| m.len()).unwrap_or(0);

    let conn = rusqlite::Connection::open(db_path)?;
    conn.execute_batch("VACUUM")?;
    drop(conn);

    let size_after = db_path.metadata().map(|m| m.len()).unwrap_or(0);

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── vacuum_database ──────────────────────────────────────────────

    #[test]
    fn vacuum_database_returns_ok_with_positive_sizes() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");

        // Create a database and insert some data so the file has non-zero size.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, data TEXT);
             INSERT INTO t (data) VALUES ('hello');
             INSERT INTO t (data) VALUES ('world');",
        )
        .unwrap();
        drop(conn);

        let result = vacuum_database(&db_path).unwrap();
        assert!(result.size_before > 0, "size_before should be > 0");
        assert!(result.size_after > 0, "size_after should be > 0");
    }

    #[test]
    fn vacuum_database_on_empty_db() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("empty.db");

        // Force SQLite to write the header by creating a table.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE _empty (id INTEGER)").unwrap();
        drop(conn);

        let result = vacuum_database(&db_path).unwrap();
        assert!(result.size_before > 0, "SQLite file should have non-zero size after schema creation");
    }

    #[test]
    fn vacuum_database_nonexistent_path_creates_db() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("new.db");

        // The file does not exist yet; vacuum_database opens it (SQLite creates it).
        // size_before will be 0 because metadata fails on a missing file.
        let result = vacuum_database(&db_path).unwrap();
        assert_eq!(result.size_before, 0);
        assert!(result.size_after > 0);
    }

    // ── database_size_info ───────────────────────────────────────────

    #[test]
    fn database_size_info_without_archive() {
        let tmp = TempDir::new().unwrap();

        // Create a small database file.
        let db_path = tmp.path().join("app.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY);")
            .unwrap();
        drop(conn);

        // Create a logs directory with one file.
        let logs_dir = tmp.path().join("logs");
        fs::create_dir(&logs_dir).unwrap();
        fs::write(logs_dir.join("app.log"), "log line\n").unwrap();

        let info = database_size_info(&db_path, &logs_dir, None).unwrap();

        assert!(info.db_size > 0, "db_size should include the database file");
        assert_eq!(info.logs_size, 9); // "log line\n" is 9 bytes
        assert_eq!(info.archive_size, 0, "no archive dir supplied");
    }

    #[test]
    fn database_size_info_with_archive() {
        let tmp = TempDir::new().unwrap();

        let db_path = tmp.path().join("app.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE _t (id INTEGER)").unwrap();
        drop(conn);

        let logs_dir = tmp.path().join("logs");
        fs::create_dir(&logs_dir).unwrap();

        let archive_dir = tmp.path().join("archive");
        fs::create_dir(&archive_dir).unwrap();
        fs::write(archive_dir.join("event1.json"), "{}").unwrap();
        fs::write(archive_dir.join("event2.json"), "{}").unwrap();

        let info =
            database_size_info(&db_path, &logs_dir, Some(&archive_dir)).unwrap();

        assert!(info.db_size > 0);
        assert_eq!(info.logs_size, 0, "empty logs dir");
        assert_eq!(info.archive_size, 4, "two files of 2 bytes each");
    }

    #[test]
    fn database_size_info_includes_wal_and_shm() {
        let tmp = TempDir::new().unwrap();

        let db_path = tmp.path().join("app.db");
        fs::write(&db_path, "fake-db-content").unwrap();

        // Simulate WAL and SHM sidecar files.
        let wal_path = tmp.path().join("app.db-wal");
        let shm_path = tmp.path().join("app.db-shm");
        fs::write(&wal_path, "wal-data").unwrap();
        fs::write(&shm_path, "shm").unwrap();

        let logs_dir = tmp.path().join("logs");
        fs::create_dir(&logs_dir).unwrap();

        let info = database_size_info(&db_path, &logs_dir, None).unwrap();

        let expected = "fake-db-content".len() as u64
            + "wal-data".len() as u64
            + "shm".len() as u64;
        assert_eq!(info.db_size, expected);
    }

    // ── dir_size_recursive ───────────────────────────────────────────

    #[test]
    fn dir_size_recursive_empty_dir() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(dir_size_recursive(tmp.path()), 0);
    }

    #[test]
    fn dir_size_recursive_with_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.txt"), "aaaa").unwrap(); // 4 bytes
        fs::write(tmp.path().join("b.txt"), "bb").unwrap(); // 2 bytes
        assert_eq!(dir_size_recursive(tmp.path()), 6);
    }

    #[test]
    fn dir_size_recursive_nested_dirs() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("c.txt"), "ccc").unwrap(); // 3 bytes

        let deep = sub.join("deep");
        fs::create_dir(&deep).unwrap();
        fs::write(deep.join("d.txt"), "d").unwrap(); // 1 byte

        fs::write(tmp.path().join("root.txt"), "rr").unwrap(); // 2 bytes

        assert_eq!(dir_size_recursive(tmp.path()), 6);
    }

    #[test]
    fn dir_size_recursive_nonexistent_path() {
        let path = Path::new("/tmp/does_not_exist_at_all_12345");
        assert_eq!(dir_size_recursive(path), 0);
    }
}
