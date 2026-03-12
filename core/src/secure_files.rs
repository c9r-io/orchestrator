use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Creates a directory and enforces the requested permissions when supported.
pub fn ensure_dir(path: &Path, mode: u32) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    }
    Ok(())
}

/// Atomically writes a file and applies the requested permissions when supported.
pub fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent, 0o700)?;
    }

    let tmp_path = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!("{ext}."))
            .unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .with_context(|| format!("failed to create temporary file {}", tmp_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("failed to set permissions on {}", tmp_path.display()))?;
    }
    file.write_all(contents)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to fsync {}", tmp_path.display()))?;
    drop(file);
    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename {} -> {}",
            tmp_path.display(),
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_dir_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("sub").join("deep");
        assert!(!dir.exists());
        ensure_dir(&dir, 0o700).expect("ensure_dir should succeed");
        assert!(dir.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("perms");
        ensure_dir(&dir, 0o750).expect("ensure_dir");
        let perms = std::fs::metadata(&dir).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o750);
    }

    #[test]
    fn write_atomic_creates_file_with_content() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.txt");
        write_atomic(&file, b"hello world", 0o600).expect("write_atomic");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello world");
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_sets_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("perm.txt");
        write_atomic(&file, b"data", 0o640).expect("write_atomic");
        let perms = std::fs::metadata(&file).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o640);
    }

    #[test]
    fn write_atomic_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("new_parent").join("deep").join("file.dat");
        write_atomic(&file, b"nested", 0o600).expect("write_atomic");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "nested");
    }

    #[test]
    fn write_atomic_no_temp_file_left_behind() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("clean.txt");
        write_atomic(&file, b"ok", 0o600).expect("write_atomic");
        // Only the target file should exist in the dir
        let entries: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["clean.txt"]);
    }

    #[test]
    fn write_atomic_file_with_extension_computes_tmp_path() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("config.yaml");
        write_atomic(&file, b"key: val", 0o600).expect("write_atomic");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "key: val");
    }

    #[test]
    fn write_atomic_file_without_extension() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("noext");
        write_atomic(&file, b"data", 0o600).expect("write_atomic");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "data");
    }

    #[test]
    fn write_atomic_fails_if_temp_already_exists() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("race.txt");
        // Pre-create the tmp file that write_atomic would use
        // write_atomic computes: path.with_extension("txt.tmp") => "race.txt.tmp"
        let tmp_path = file.with_extension("txt.tmp");
        std::fs::write(&tmp_path, b"blocker").unwrap();
        let result = write_atomic(&file, b"data", 0o600);
        assert!(result.is_err());
    }
}
