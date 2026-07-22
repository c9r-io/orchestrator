//! Daemon-level host file-sharing policy.
//!
//! The policy is intentionally outside project manifests: it is the operator-owned
//! ceiling that project Workspace and ExecutionProfile paths may never exceed.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// One read-only global Skill directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalSkillPath {
    /// Host path containing one or more Skills.
    pub path: String,
}

/// Effective daemon file-sharing policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct FileSharingPolicy {
    /// Directories exposed read-only to sandboxed tasks.
    pub global_skills: Vec<GlobalSkillPath>,
    /// Operator-owned ceiling for all user-declared host paths.
    pub shareable_roots: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct FileSharingEnvelope {
    file_sharing: FileSharingPolicy,
}

/// Load `{data_dir}/file-sharing.yaml`. A missing file is a deny-all policy.
pub fn load_file_sharing_policy(data_dir: &Path) -> anyhow::Result<FileSharingPolicy> {
    let path = data_dir.join("file-sharing.yaml");
    if !path.exists() {
        return Ok(FileSharingPolicy::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| anyhow::anyhow!("failed to read {}: {error}", path.display()))?;
    let envelope: FileSharingEnvelope = serde_yaml::from_str(&raw)
        .map_err(|error| anyhow::anyhow!("failed to parse {}: {error}", path.display()))?;
    Ok(envelope.file_sharing)
}

/// Expand a leading `~` using the daemon user's HOME.
pub fn expand_home(path: &str) -> anyhow::Result<PathBuf> {
    if path == "~" || path.starts_with("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("HOME is not set while expanding '{path}'"))?;
        return Ok(if path == "~" {
            home
        } else {
            home.join(path.trim_start_matches("~/"))
        });
    }
    Ok(PathBuf::from(path))
}

/// Canonicalize an authorization path without permitting lexical traversal.
/// Missing leaf components are accepted only beneath a canonical existing ancestor.
pub fn canonicalize_policy_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("file-sharing path cannot be empty");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        anyhow::bail!("file-sharing path cannot contain '..': {}", path.display());
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            anyhow::anyhow!(
                "file-sharing path has no existing ancestor: {}",
                absolute.display()
            )
        })?;
        missing.push(name.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            anyhow::anyhow!("file-sharing path has no parent: {}", absolute.display())
        })?;
    }
    let mut resolved = ancestor.canonicalize().map_err(|error| {
        anyhow::anyhow!("failed to canonicalize {}: {error}", ancestor.display())
    })?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

impl FileSharingPolicy {
    /// Resolve and validate the configured ceiling roots.
    pub fn resolved_shareable_roots(&self) -> anyhow::Result<Vec<PathBuf>> {
        self.shareable_roots
            .iter()
            .map(|path| canonicalize_policy_path(&expand_home(path)?))
            .collect()
    }

    /// Resolve global Skill paths and verify each is inside the ceiling.
    pub fn resolved_global_skills(&self) -> anyhow::Result<Vec<PathBuf>> {
        self.global_skills
            .iter()
            .map(|entry| {
                let path = canonicalize_policy_path(&expand_home(&entry.path)?)?;
                self.ensure_shareable(&path, "fileSharing.globalSkills")?;
                if !path.is_dir() {
                    anyhow::bail!("global Skill path must be a directory: {}", path.display());
                }
                Ok(path)
            })
            .collect()
    }

    /// Fail closed unless `path` is contained by one configured ceiling root.
    pub fn ensure_shareable(&self, path: &Path, field: &str) -> anyhow::Result<PathBuf> {
        let candidate = canonicalize_policy_path(path)?;
        let roots = self.resolved_shareable_roots()?;
        if roots.iter().any(|root| candidate.starts_with(root)) {
            return Ok(candidate);
        }
        anyhow::bail!(
            "[FILE_SHARING_PATH_OUTSIDE_CEILING] {field} path '{}' is outside fileSharing.shareableRoots",
            candidate.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_policy_is_deny_all() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = load_file_sharing_policy(dir.path()).expect("load policy");
        assert!(policy.shareable_roots.is_empty());
        assert!(policy.global_skills.is_empty());
    }

    #[test]
    fn ceiling_is_subset_not_union_or_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("shared");
        std::fs::create_dir_all(&root).expect("shared root");
        let sibling = dir.path().join("shared-escape");
        std::fs::create_dir_all(&sibling).expect("sibling");
        let policy = FileSharingPolicy {
            global_skills: Vec::new(),
            shareable_roots: vec![root.to_string_lossy().into_owned()],
        };
        assert!(
            policy
                .ensure_shareable(&root.join("nested"), "test")
                .is_ok()
        );
        assert!(policy.ensure_shareable(&sibling, "test").is_err());
    }

    #[test]
    fn parent_traversal_is_rejected_before_normalization() {
        let error = canonicalize_policy_path(Path::new("safe/../escape"))
            .expect_err("parent traversal must fail");
        assert!(error.to_string().contains("cannot contain '..'"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_checked_after_canonicalization() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("shared");
        let outside = dir.path().join("private");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&outside).expect("outside");
        symlink(&outside, root.join("link")).expect("symlink");
        let policy = FileSharingPolicy {
            global_skills: Vec::new(),
            shareable_roots: vec![root.to_string_lossy().into_owned()],
        };
        assert!(policy.ensure_shareable(&root.join("link"), "test").is_err());
    }
}
