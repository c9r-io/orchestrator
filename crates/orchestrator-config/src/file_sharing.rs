//! Daemon-level host file-sharing policy.
//!
//! The policy is intentionally outside project manifests: it is the operator-owned
//! ceiling that project Workspace and ExecutionProfile paths may never exceed.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

const GLOBAL_SKILL_UNTRUSTED: &str = "FILE_SHARING_GLOBAL_SKILL_UNTRUSTED";

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

/// One host path that a task execution can modify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskWritablePath {
    /// Canonical or canonicalizable host path.
    pub path: PathBuf,
    /// Operator-facing configuration source for diagnostics.
    pub source: String,
}

impl TaskWritablePath {
    /// Creates a task-writable path with its configuration source.
    pub fn new(path: impl Into<PathBuf>, source: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
        }
    }
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

    /// Resolve global Skill paths and verify their provenance and isolation.
    pub fn resolved_global_skills(
        &self,
        task_writable_paths: &[TaskWritablePath],
    ) -> anyhow::Result<Vec<PathBuf>> {
        self.global_skills
            .iter()
            .map(|entry| {
                let path = canonicalize_policy_path(&expand_home(&entry.path)?)?;
                self.ensure_shareable(&path, "fileSharing.globalSkills")?;
                if !path.is_dir() {
                    anyhow::bail!("global Skill path must be a directory: {}", path.display());
                }
                validate_global_skill_metadata(&path)?;
                for writable in task_writable_paths {
                    let writable_path = canonicalize_policy_path(&writable.path)?;
                    if path.starts_with(&writable_path) || writable_path.starts_with(&path) {
                        return Err(untrusted_global_skill(
                            &path,
                            format!(
                                "it overlaps task-writable path '{}' from {}",
                                writable_path.display(),
                                writable.source
                            ),
                            "move the global Skill outside every task work_dir and writable ExecutionProfile path, or remove that write authorization",
                        ));
                    }
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

fn untrusted_global_skill(
    path: &Path,
    reason: impl std::fmt::Display,
    suggested_fix: &str,
) -> anyhow::Error {
    anyhow::anyhow!(
        "[{GLOBAL_SKILL_UNTRUSTED}] global Skill directory '{}' is untrusted: {reason}\n  category: authorization\n  suggested_fix: {suggested_fix}",
        path.display()
    )
}

#[cfg(unix)]
fn validate_global_skill_metadata(path: &Path) -> anyhow::Result<()> {
    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    let daemon_uid = unsafe { libc::geteuid() };
    validate_global_skill_metadata_for_uid(path, daemon_uid)
}

#[cfg(unix)]
fn validate_global_skill_metadata_for_uid(path: &Path, daemon_uid: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::metadata(path).map_err(|error| {
        untrusted_global_skill(
            path,
            format!("its metadata cannot be read: {error}"),
            "restore the directory and make it readable by the daemon user",
        )
    })?;
    if metadata.uid() != daemon_uid {
        return Err(untrusted_global_skill(
            path,
            format!(
                "owner uid {} does not match daemon uid {daemon_uid}",
                metadata.uid()
            ),
            "change the directory owner to the daemon user and restart orchestratord",
        ));
    }
    let mode = metadata.mode() & 0o7777;
    if mode & 0o022 != 0 {
        return Err(untrusted_global_skill(
            path,
            format!("mode {mode:#06o} permits group or world writes"),
            "remove group/world write permission (for example, chmod go-w) and restart orchestratord",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_global_skill_metadata(path: &Path) -> anyhow::Result<()> {
    Err(untrusted_global_skill(
        path,
        "this platform cannot verify Unix owner and permission bits",
        "disable fileSharing.globalSkills or run orchestratord on a supported Unix platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_for(root: &Path, global_skill: &Path) -> FileSharingPolicy {
        FileSharingPolicy {
            global_skills: vec![GlobalSkillPath {
                path: global_skill.to_string_lossy().into_owned(),
            }],
            shareable_roots: vec![root.to_string_lossy().into_owned()],
        }
    }

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

    #[cfg(unix)]
    #[test]
    fn global_skill_owned_by_another_uid_is_rejected() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let skill = dir.path().join("skills");
        std::fs::create_dir(&skill).expect("skill directory");
        let actual_uid = std::fs::metadata(&skill).expect("metadata").uid();
        let error = validate_global_skill_metadata_for_uid(&skill, actual_uid.wrapping_add(1))
            .expect_err("different owner must fail");
        assert!(error.to_string().contains(GLOBAL_SKILL_UNTRUSTED));
        assert!(error.to_string().contains("owner uid"));
        assert!(error.to_string().contains("suggested_fix"));
    }

    #[cfg(unix)]
    #[test]
    fn group_writable_global_skill_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let skill = dir.path().join("skills");
        std::fs::create_dir(&skill).expect("skill directory");
        std::fs::set_permissions(&skill, std::fs::Permissions::from_mode(0o770))
            .expect("set permissions");
        let error = policy_for(dir.path(), &skill)
            .resolved_global_skills(&[])
            .expect_err("group-writable directory must fail");
        assert!(error.to_string().contains(GLOBAL_SKILL_UNTRUSTED));
        assert!(error.to_string().contains("group or world writes"));
    }

    #[cfg(unix)]
    #[test]
    fn world_writable_global_skill_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let skill = dir.path().join("skills");
        std::fs::create_dir(&skill).expect("skill directory");
        std::fs::set_permissions(&skill, std::fs::Permissions::from_mode(0o707))
            .expect("set permissions");
        let error = policy_for(dir.path(), &skill)
            .resolved_global_skills(&[])
            .expect_err("world-writable directory must fail");
        assert!(error.to_string().contains(GLOBAL_SKILL_UNTRUSTED));
    }

    #[cfg(unix)]
    #[test]
    fn global_skill_overlapping_task_writable_path_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let skill = dir.path().join("skills");
        let writable_child = skill.join("generated");
        std::fs::create_dir_all(&writable_child).expect("writable child");
        std::fs::set_permissions(&skill, std::fs::Permissions::from_mode(0o700))
            .expect("set permissions");
        let error = policy_for(dir.path(), &skill)
            .resolved_global_skills(&[TaskWritablePath::new(
                &writable_child,
                "executionProfile.writable_paths",
            )])
            .expect_err("writable descendant must fail");
        assert!(error.to_string().contains(GLOBAL_SKILL_UNTRUSTED));
        assert!(
            error
                .to_string()
                .contains("executionProfile.writable_paths")
        );
    }

    #[cfg(unix)]
    #[test]
    fn trusted_isolated_global_skill_is_accepted() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let skill = dir.path().join("skills");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&skill).expect("skill directory");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::set_permissions(&skill, std::fs::Permissions::from_mode(0o755))
            .expect("set permissions");
        let resolved = policy_for(dir.path(), &skill)
            .resolved_global_skills(&[TaskWritablePath::new(&workspace, "workspace.work_dir")])
            .expect("trusted directory");
        assert_eq!(
            resolved,
            vec![skill.canonicalize().expect("canonical skill path")]
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_global_skill_is_rejected_when_provenance_cannot_be_verified() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill = dir.path().join("skills");
        std::fs::create_dir(&skill).expect("skill directory");
        let error = policy_for(dir.path(), &skill)
            .resolved_global_skills(&[])
            .expect_err("unsupported provenance check must fail closed");
        assert!(error.to_string().contains(GLOBAL_SKILL_UNTRUSTED));
        assert!(error.to_string().contains("supported Unix platform"));
    }
}
