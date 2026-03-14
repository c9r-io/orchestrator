use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(super) const RELEASE_BINARY_REL: &str = "target/release/orchestratord";
pub(super) const STABLE_FILE: &str = ".stable";
pub(super) const STABLE_MANIFEST: &str = ".stable.json";
pub(super) const STABLE_TMP: &str = ".stable.tmp";

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Sidecar metadata describing a captured release-binary snapshot.
pub struct SnapshotManifest {
    /// Manifest schema version.
    pub version: u32,
    /// SHA-256 checksum of the snapshot file.
    pub sha256: String,
    /// Timestamp when the snapshot was created.
    pub created_at: String,
    /// Task identifier that produced the snapshot.
    pub task_id: String,
    /// Workflow cycle number that produced the snapshot.
    pub cycle: u32,
    /// Relative source path of the binary that was snapshotted.
    pub source_path: String,
    /// Snapshot file size in bytes.
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Result of comparing the current release binary to the saved stable snapshot.
pub struct BinaryVerificationResult {
    /// Whether the current binary matches the saved stable snapshot checksum.
    pub verified: bool,
    /// Checksum computed from the saved stable snapshot.
    pub original_checksum: String,
    /// Checksum computed from the current release binary.
    pub current_checksum: String,
    /// Path to the stable snapshot file.
    pub stable_path: PathBuf,
    /// Path to the release binary being checked.
    pub binary_path: PathBuf,
    /// Parsed manifest sidecar when present.
    pub manifest: Option<SnapshotManifest>,
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// Verifies that the saved stable snapshot matches the current release binary.
pub async fn verify_binary_snapshot(workspace_root: &Path) -> Result<BinaryVerificationResult> {
    let stable_path = workspace_root.join(STABLE_FILE);
    let binary_path = workspace_root.join(RELEASE_BINARY_REL);

    if !stable_path.exists() {
        anyhow::bail!(
            "no .stable binary snapshot found at {}",
            stable_path.display()
        );
    }

    if !binary_path.exists() {
        anyhow::bail!("release binary not found at {}", binary_path.display());
    }

    let stable_content = tokio::fs::read(&stable_path)
        .await
        .with_context(|| format!("failed to read stable binary at {}", stable_path.display()))?;
    let binary_content = tokio::fs::read(&binary_path)
        .await
        .with_context(|| format!("failed to read binary at {}", binary_path.display()))?;

    let stable_checksum = sha256_hex(&stable_content);
    let binary_checksum = sha256_hex(&binary_content);

    let verified = stable_checksum == binary_checksum;

    let manifest_path = workspace_root.join(STABLE_MANIFEST);
    let manifest = if manifest_path.exists() {
        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .ok()
            .and_then(|s| serde_json::from_str::<SnapshotManifest>(&s).ok());
        manifest_content
    } else {
        None
    };

    Ok(BinaryVerificationResult {
        verified,
        original_checksum: stable_checksum,
        current_checksum: binary_checksum,
        stable_path,
        binary_path,
        manifest,
    })
}

/// Captures the current release binary into the stable snapshot location.
pub async fn snapshot_binary(workspace_root: &Path, task_id: &str, cycle: u32) -> Result<PathBuf> {
    let binary_path = workspace_root.join(RELEASE_BINARY_REL);
    let stable_path = workspace_root.join(STABLE_FILE);
    let tmp_path = workspace_root.join(STABLE_TMP);
    let manifest_path = workspace_root.join(STABLE_MANIFEST);

    if !binary_path.exists() {
        anyhow::bail!(
            "release binary not found at {}; skipping binary snapshot",
            binary_path.display()
        );
    }

    // Atomic write: copy to .tmp first
    tokio::fs::copy(&binary_path, &tmp_path)
        .await
        .with_context(|| {
            format!(
                "failed to copy binary from {} to {}",
                binary_path.display(),
                tmp_path.display()
            )
        })?;

    // Compute SHA-256 of the tmp file
    let tmp_content = tokio::fs::read(&tmp_path)
        .await
        .with_context(|| format!("failed to read tmp file at {}", tmp_path.display()))?;
    let checksum = sha256_hex(&tmp_content);
    let size_bytes = tmp_content.len() as u64;

    // Atomic rename: .tmp -> .stable
    tokio::fs::rename(&tmp_path, &stable_path)
        .await
        .with_context(|| {
            format!(
                "failed to rename {} to {}",
                tmp_path.display(),
                stable_path.display()
            )
        })?;

    // Post-snapshot verification: re-read first 4096 bytes and confirm SHA-256 prefix
    let verification_content = tokio::fs::read(&stable_path).await.with_context(|| {
        format!(
            "failed to read stable file for verification at {}",
            stable_path.display()
        )
    })?;
    let prefix_len = std::cmp::min(4096, verification_content.len());
    let expected_prefix = &tmp_content[..prefix_len];
    let actual_prefix = &verification_content[..prefix_len];
    if expected_prefix != actual_prefix {
        anyhow::bail!("post-snapshot verification failed: stable file content mismatch");
    }

    // Write manifest sidecar
    let manifest = SnapshotManifest {
        version: 2,
        sha256: checksum,
        created_at: Utc::now().to_rfc3339(),
        task_id: task_id.to_string(),
        cycle,
        source_path: RELEASE_BINARY_REL.to_string(),
        size_bytes,
    };
    let manifest_json =
        serde_json::to_string_pretty(&manifest).context("failed to serialize snapshot manifest")?;
    tokio::fs::write(&manifest_path, manifest_json)
        .await
        .with_context(|| format!("failed to write manifest at {}", manifest_path.display()))?;

    Ok(stable_path)
}

/// Restores the saved stable snapshot over the current release binary.
pub async fn restore_binary_snapshot(workspace_root: &Path) -> Result<()> {
    let stable_path = workspace_root.join(STABLE_FILE);
    let binary_path = workspace_root.join(RELEASE_BINARY_REL);
    let manifest_path = workspace_root.join(STABLE_MANIFEST);

    if !stable_path.exists() {
        anyhow::bail!(
            "no .stable binary snapshot found at {}",
            stable_path.display()
        );
    }

    // Pre-restore integrity check if manifest exists
    if manifest_path.exists() {
        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .with_context(|| format!("failed to read manifest at {}", manifest_path.display()))?;
        let manifest: SnapshotManifest = serde_json::from_str(&manifest_content)
            .with_context(|| "failed to parse snapshot manifest")?;

        let stable_content = tokio::fs::read(&stable_path).await.with_context(|| {
            format!("failed to read stable binary at {}", stable_path.display())
        })?;
        let actual_checksum = sha256_hex(&stable_content);

        if actual_checksum != manifest.sha256 {
            anyhow::bail!(
                "pre-restore integrity check failed: .stable checksum {} does not match manifest {}",
                actual_checksum,
                manifest.sha256
            );
        }
    }
    // If no manifest exists, proceed anyway (v1 backward compat)

    tokio::fs::copy(&stable_path, &binary_path)
        .await
        .with_context(|| {
            format!(
                "failed to restore binary snapshot from {} to {}",
                stable_path.display(),
                binary_path.display()
            )
        })?;

    Ok(())
}
