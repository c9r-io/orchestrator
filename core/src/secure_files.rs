use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

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
