use crate::state::InnerState;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::error;

const RELEASE_BINARY_REL: &str = "core/target/release/agent-orchestrator";
const STABLE_FILE: &str = ".stable";
const STABLE_MANIFEST: &str = ".stable.json";
const STABLE_TMP: &str = ".stable.tmp";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub version: u32,
    pub sha256: String,
    pub created_at: String,
    pub task_id: String,
    pub cycle: u32,
    pub source_path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryVerificationResult {
    pub verified: bool,
    pub original_checksum: String,
    pub current_checksum: String,
    pub stable_path: PathBuf,
    pub binary_path: PathBuf,
    pub manifest: Option<SnapshotManifest>,
}

fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

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

pub async fn create_checkpoint(workspace_root: &Path, task_id: &str, cycle: u32) -> Result<String> {
    let tag_name = format!("checkpoint/{}/{}", task_id, cycle);
    let output = tokio::process::Command::new("git")
        .args(["tag", "-f", &tag_name])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to create git checkpoint tag")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git tag failed: {}", stderr);
    }
    Ok(tag_name)
}

pub async fn rollback_to_checkpoint(workspace_root: &Path, tag_name: &str) -> Result<()> {
    let output = tokio::process::Command::new("git")
        .args(["reset", "--hard", tag_name])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to rollback to checkpoint")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {}", stderr);
    }
    Ok(())
}

pub async fn snapshot_binary(
    workspace_root: &Path,
    task_id: &str,
    cycle: u32,
) -> Result<PathBuf> {
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
    let verification_content = tokio::fs::read(&stable_path)
        .await
        .with_context(|| {
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
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .context("failed to serialize snapshot manifest")?;
    tokio::fs::write(&manifest_path, manifest_json)
        .await
        .with_context(|| {
            format!(
                "failed to write manifest at {}",
                manifest_path.display()
            )
        })?;

    Ok(stable_path)
}

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
            .with_context(|| {
                format!(
                    "failed to read manifest at {}",
                    manifest_path.display()
                )
            })?;
        let manifest: SnapshotManifest = serde_json::from_str(&manifest_content)
            .with_context(|| "failed to parse snapshot manifest")?;

        let stable_content = tokio::fs::read(&stable_path)
            .await
            .with_context(|| {
                format!(
                    "failed to read stable binary at {}",
                    stable_path.display()
                )
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

pub async fn execute_self_test_step(
    workspace_root: &Path,
    state: &InnerState,
    task_id: &str,
    item_id: &str,
) -> Result<i64> {
    let core_dir = workspace_root.join("core");
    let cargo_bin = std::env::var("ORCH_SELF_TEST_CARGO").unwrap_or_else(|_| "cargo".to_string());

    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_check"}),
    );
    let check_output = tokio::process::Command::new(&cargo_bin)
        .args(["check", "--message-format=short"])
        .current_dir(&core_dir)
        .output()
        .await
        .context("failed to run cargo check")?;

    if !check_output.status.success() {
        let stderr = String::from_utf8_lossy(&check_output.stderr);
        error!(phase = "cargo_check", stderr = %stderr.trim(), "self-test phase failed");
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "cargo_check", "passed": false}),
        );
        return Ok(check_output.status.code().unwrap_or(1) as i64);
    }
    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_check", "passed": true}),
    );

    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_test_lib"}),
    );
    let test_output = tokio::process::Command::new(&cargo_bin)
        .args([
            "test",
            "--lib",
            "--",
            "--skip",
            "self_test_survives_smoke_test",
        ])
        .current_dir(&core_dir)
        .output()
        .await
        .context("failed to run cargo test --lib")?;

    if !test_output.status.success() {
        let stderr = String::from_utf8_lossy(&test_output.stderr);
        error!(
            phase = "cargo_test_lib",
            stderr = %stderr.trim(),
            "self-test phase failed"
        );
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "cargo_test_lib", "passed": false}),
        );
        return Ok(test_output.status.code().unwrap_or(1) as i64);
    }
    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_test_lib", "passed": true}),
    );

    let script_path = workspace_root.join("scripts/orchestrator.sh");
    if script_path.exists() {
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "manifest_validate"}),
        );
        let validate_output = tokio::process::Command::new(&script_path)
            .args([
                "manifest",
                "validate",
                "-f",
                "docs/workflow/self-bootstrap.yaml",
            ])
            .current_dir(workspace_root)
            .output()
            .await
            .context("failed to run manifest validate")?;

        if !validate_output.status.success() {
            let stderr = String::from_utf8_lossy(&validate_output.stderr);
            error!(
                phase = "manifest_validate",
                stderr = %stderr.trim(),
                "self-test phase failed"
            );
            state.emit_event(
                task_id,
                Some(item_id),
                "self_test_phase",
                json!({"phase": "manifest_validate", "passed": false}),
            );
            return Ok(validate_output.status.code().unwrap_or(1) as i64);
        }
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "manifest_validate", "passed": true}),
        );
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;
    use std::io::Write;
    use tempfile::TempDir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn create_mock_binary(path: &Path, content: &[u8]) -> std::io::Result<()> {
        let parent = path.parent().expect("mock binary path should have parent");
        std::fs::create_dir_all(parent)?;
        let mut file = std::fs::File::create(path)?;
        file.write_all(content)?;
        Ok(())
    }

    fn make_temp_dir(label: &str) -> TempDir {
        tempfile::Builder::new()
            .prefix(label)
            .tempdir()
            .expect("create temp dir")
    }

    fn write_executable(path: &Path, content: &str) {
        let parent = path.parent().expect("script parent");
        std::fs::create_dir_all(parent).expect("create script dir");
        std::fs::write(path, content).expect("write script");
        #[cfg(unix)]
        {
            let mut perms = std::fs::metadata(path)
                .expect("script metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).expect("set executable permissions");
        }
    }

    fn run_git(args: &[&str], cwd: &Path) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("run git");
        assert!(status.success(), "git {:?} should succeed", args);
    }

    #[tokio::test]
    async fn test_snapshot_binary_success() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-snapshot-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let test_content = b"mock binary content for testing";
        create_mock_binary(&binary_path, test_content).expect("create mock binary");

        let result = snapshot_binary(&temp_dir, "test-task", 1).await;

        assert!(result.is_ok());
        let stable_path = result.expect("snapshot should succeed");
        assert!(stable_path.exists());
        let restored_content = std::fs::read(&stable_path).expect("read stable snapshot");
        assert_eq!(restored_content, test_content);

        // Verify .stable.tmp does not exist
        assert!(!temp_dir.join(STABLE_TMP).exists());

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_snapshot_binary_missing_release() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let result = snapshot_binary(&temp_dir, "test-task", 1).await;

        assert!(result.is_err());
        let err_msg = result.expect_err("operation should fail").to_string();
        assert!(err_msg.contains("release binary not found"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_restore_binary_snapshot_success() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-restore-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let stable_path = temp_dir.join(STABLE_FILE);
        let binary_path = temp_dir.join("core/target/release");
        std::fs::create_dir_all(&binary_path).expect("create binary dir");

        let test_content = b"stable binary snapshot content";
        create_mock_binary(&stable_path, test_content).expect("create stable snapshot");

        let result = restore_binary_snapshot(&temp_dir).await;

        assert!(result.is_ok());
        let restored_binary_path = binary_path.join("agent-orchestrator");
        assert!(restored_binary_path.exists());
        let restored_content = std::fs::read(&restored_binary_path).expect("read restored binary");
        assert_eq!(restored_content, test_content);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_restore_binary_snapshot_missing_stable() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-restore-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let result = restore_binary_snapshot(&temp_dir).await;

        assert!(result.is_err());
        let err_msg = result.expect_err("operation should fail").to_string();
        assert!(err_msg.contains("no .stable binary snapshot found"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_snapshot_restore_content_integrity() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-integrity-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let original_content = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
        create_mock_binary(&binary_path, &original_content).expect("create original binary");

        snapshot_binary(&temp_dir, "test-task", 1)
            .await
            .expect("snapshot binary");
        restore_binary_snapshot(&temp_dir)
            .await
            .expect("restore binary snapshot");

        let final_content = std::fs::read(&binary_path).expect("read final binary");
        assert_eq!(final_content, original_content);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_verify_binary_snapshot_matches() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-verify-match-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let stable_path = temp_dir.join(STABLE_FILE);
        let test_content = b"binary content for verification test";
        create_mock_binary(&binary_path, test_content).expect("create current binary");
        create_mock_binary(&stable_path, test_content).expect("create stable binary");

        let result = verify_binary_snapshot(&temp_dir).await;

        assert!(result.is_ok());
        let verification = result.expect("verification should succeed");
        assert!(verification.verified);
        assert_eq!(
            verification.original_checksum,
            verification.current_checksum
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_verify_binary_snapshot_mismatch() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-verify-mismatch-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let stable_path = temp_dir.join(STABLE_FILE);
        let original_content = b"original binary content";
        let modified_content = b"modified binary content";
        create_mock_binary(&binary_path, modified_content).expect("create modified binary");
        create_mock_binary(&stable_path, original_content).expect("create original stable binary");

        let result = verify_binary_snapshot(&temp_dir).await;

        assert!(result.is_ok());
        let verification = result.expect("verification should succeed");
        assert!(!verification.verified);
        assert_ne!(
            verification.original_checksum,
            verification.current_checksum
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_verify_binary_snapshot_missing_stable() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-verify-no-stable-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        create_mock_binary(&binary_path, b"test").expect("create binary without stable");

        let result = verify_binary_snapshot(&temp_dir).await;

        assert!(result.is_err());
        let err_msg = result.expect_err("operation should fail").to_string();
        assert!(err_msg.contains("no .stable binary snapshot found"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_verify_binary_snapshot_missing_binary() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-verify-no-binary-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let stable_path = temp_dir.join(STABLE_FILE);
        create_mock_binary(&stable_path, b"test").expect("create stable snapshot");

        let result = verify_binary_snapshot(&temp_dir).await;

        assert!(result.is_err());
        let err_msg = result.expect_err("operation should fail").to_string();
        assert!(err_msg.contains("release binary not found"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_create_checkpoint_and_rollback_success() {
        let temp = make_temp_dir("safety-test-git");
        let repo = temp.path();

        run_git(&["init"], repo);
        run_git(&["config", "user.email", "test@example.com"], repo);
        run_git(&["config", "user.name", "Coverage Test"], repo);

        let tracked = repo.join("tracked.txt");
        std::fs::write(&tracked, "base\n").expect("write base file");
        run_git(&["add", "tracked.txt"], repo);
        run_git(&["commit", "-m", "base"], repo);

        std::fs::write(&tracked, "checkpoint\n").expect("write checkpoint content");
        run_git(&["commit", "-am", "checkpoint"], repo);

        let tag = create_checkpoint(repo, "task-1", 2)
            .await
            .expect("create checkpoint");
        assert_eq!(tag, "checkpoint/task-1/2");

        std::fs::write(&tracked, "after\n").expect("write after content");
        run_git(&["commit", "-am", "after"], repo);

        rollback_to_checkpoint(repo, &tag)
            .await
            .expect("rollback to checkpoint");

        assert_eq!(
            std::fs::read_to_string(&tracked).expect("read tracked file"),
            "checkpoint\n"
        );
    }

    #[tokio::test]
    async fn test_create_checkpoint_fails_outside_git_repo() {
        let temp = make_temp_dir("safety-test-no-git");
        let err = create_checkpoint(temp.path(), "task-1", 1)
            .await
            .expect_err("checkpoint should fail without git repo");
        assert!(err.to_string().contains("git tag failed"));
    }

    #[tokio::test]
    async fn test_rollback_to_checkpoint_fails_for_missing_tag() {
        let temp = make_temp_dir("safety-test-missing-tag");
        let repo = temp.path();

        run_git(&["init"], repo);
        run_git(&["config", "user.email", "test@example.com"], repo);
        run_git(&["config", "user.name", "Coverage Test"], repo);
        let tracked = repo.join("tracked.txt");
        std::fs::write(&tracked, "base\n").expect("write base file");
        run_git(&["add", "tracked.txt"], repo);
        run_git(&["commit", "-m", "base"], repo);

        let err = rollback_to_checkpoint(repo, "checkpoint/missing/1")
            .await
            .expect_err("rollback should fail for missing tag");
        assert!(err.to_string().contains("git reset failed"));
    }

    #[tokio::test]
    async fn test_execute_self_test_step_returns_nonzero_when_cargo_check_fails() {
        let _env_guard = ENV_LOCK.lock().await;
        let mut fixture = TestState::new();
        let state = fixture.build();
        let workspace_root = state.app_root.clone();

        let fake_bin = workspace_root.join("fake-bin");
        let cargo_log = workspace_root.join("fake-cargo.log");
        write_executable(
            &fake_bin.join("cargo"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_CARGO_LOG\"\nexit 9\n",
        );
        std::fs::create_dir_all(workspace_root.join("core")).expect("create fake core dir");

        let fake_cargo = fake_bin.join("cargo");
        std::env::set_var("FAKE_CARGO_LOG", &cargo_log);
        std::env::set_var("ORCH_SELF_TEST_CARGO", &fake_cargo);

        let result = execute_self_test_step(&workspace_root, &state, "task-1", "item-1")
            .await
            .expect("self test should return exit code");

        std::env::remove_var("FAKE_CARGO_LOG");
        std::env::remove_var("ORCH_SELF_TEST_CARGO");

        assert_eq!(result, 9);
        let log = std::fs::read_to_string(&cargo_log).expect("read cargo log");
        assert!(log.contains("check --message-format=short"));
        assert!(!log.contains("test --lib"));
    }

    #[tokio::test]
    async fn test_execute_self_test_step_success_with_manifest_validate() {
        let _env_guard = ENV_LOCK.lock().await;
        let mut fixture = TestState::new();
        let state = fixture.build();
        let workspace_root = state.app_root.clone();

        let fake_bin = workspace_root.join("fake-bin");
        let cargo_log = workspace_root.join("fake-cargo.log");
        let manifest_log = workspace_root.join("manifest.log");
        write_executable(
            &fake_bin.join("cargo"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_CARGO_LOG\"\nexit 0\n",
        );
        write_executable(
            &workspace_root.join("scripts/orchestrator.sh"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_MANIFEST_LOG\"\nexit 0\n",
        );
        std::fs::create_dir_all(workspace_root.join("core")).expect("create fake core dir");

        let fake_cargo = fake_bin.join("cargo");
        std::env::set_var("FAKE_CARGO_LOG", &cargo_log);
        std::env::set_var("FAKE_MANIFEST_LOG", &manifest_log);
        std::env::set_var("ORCH_SELF_TEST_CARGO", &fake_cargo);

        let result = execute_self_test_step(&workspace_root, &state, "task-1", "item-1")
            .await
            .expect("self test should succeed");

        std::env::remove_var("FAKE_CARGO_LOG");
        std::env::remove_var("FAKE_MANIFEST_LOG");
        std::env::remove_var("ORCH_SELF_TEST_CARGO");

        assert_eq!(result, 0);
        let cargo_calls = std::fs::read_to_string(&cargo_log).expect("read cargo log");
        let manifest_calls = std::fs::read_to_string(&manifest_log).expect("read manifest log");
        assert!(cargo_calls.contains("check --message-format=short"));
        assert!(cargo_calls.contains("test --lib -- --skip self_test_survives_smoke_test"));
        assert!(manifest_calls.contains("manifest validate -f docs/workflow/self-bootstrap.yaml"));
    }

    // --- New v2 tests ---

    #[tokio::test]
    async fn test_snapshot_creates_manifest() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-manifest-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let test_content = b"manifest test binary content";
        create_mock_binary(&binary_path, test_content).expect("create mock binary");

        snapshot_binary(&temp_dir, "task-abc", 3)
            .await
            .expect("snapshot should succeed");

        let manifest_path = temp_dir.join(STABLE_MANIFEST);
        assert!(manifest_path.exists(), ".stable.json should exist");

        let manifest_str =
            std::fs::read_to_string(&manifest_path).expect("read manifest");
        let manifest: SnapshotManifest =
            serde_json::from_str(&manifest_str).expect("parse manifest JSON");

        assert_eq!(manifest.version, 2);
        assert_eq!(manifest.task_id, "task-abc");
        assert_eq!(manifest.cycle, 3);
        assert_eq!(manifest.source_path, RELEASE_BINARY_REL);
        assert_eq!(manifest.size_bytes, test_content.len() as u64);

        let expected_sha = sha256_hex(test_content);
        assert_eq!(manifest.sha256, expected_sha);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_snapshot_manifest_round_trip() {
        let manifest = SnapshotManifest {
            version: 2,
            sha256: "abc123".to_string(),
            created_at: "2026-03-04T00:00:00Z".to_string(),
            task_id: "task-rt".to_string(),
            cycle: 5,
            source_path: RELEASE_BINARY_REL.to_string(),
            size_bytes: 999,
        };
        let json = serde_json::to_string_pretty(&manifest).expect("serialize");
        let deserialized: SnapshotManifest =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.version, manifest.version);
        assert_eq!(deserialized.sha256, manifest.sha256);
        assert_eq!(deserialized.task_id, manifest.task_id);
        assert_eq!(deserialized.cycle, manifest.cycle);
        assert_eq!(deserialized.size_bytes, manifest.size_bytes);
    }

    #[tokio::test]
    async fn test_snapshot_atomic_leaves_no_tmp() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-no-tmp-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        create_mock_binary(&binary_path, b"no tmp test").expect("create mock binary");

        snapshot_binary(&temp_dir, "task-tmp", 1)
            .await
            .expect("snapshot should succeed");

        let tmp_path = temp_dir.join(STABLE_TMP);
        assert!(
            !tmp_path.exists(),
            ".stable.tmp should not exist after successful snapshot"
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_snapshot_verify_sha256() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-sha256-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let test_content = b"sha256 verification content";
        create_mock_binary(&binary_path, test_content).expect("create mock binary");

        snapshot_binary(&temp_dir, "task-sha", 1)
            .await
            .expect("snapshot should succeed");

        let stable_content =
            std::fs::read(temp_dir.join(STABLE_FILE)).expect("read .stable");
        let actual_sha = sha256_hex(&stable_content);

        let manifest_str =
            std::fs::read_to_string(temp_dir.join(STABLE_MANIFEST)).expect("read manifest");
        let manifest: SnapshotManifest =
            serde_json::from_str(&manifest_str).expect("parse manifest");

        assert_eq!(
            actual_sha, manifest.sha256,
            "SHA-256 in manifest should match actual .stable content"
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_restore_with_manifest_integrity_check() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-restore-integrity-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let test_content = b"integrity check restore content";
        create_mock_binary(&binary_path, test_content).expect("create mock binary");

        snapshot_binary(&temp_dir, "task-integrity", 2)
            .await
            .expect("snapshot should succeed");

        // Restore should succeed because manifest matches
        restore_binary_snapshot(&temp_dir)
            .await
            .expect("restore should succeed with valid manifest");

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_restore_rejects_corrupt_stable() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-restore-corrupt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let test_content = b"original content for corruption test";
        create_mock_binary(&binary_path, test_content).expect("create mock binary");

        snapshot_binary(&temp_dir, "task-corrupt", 1)
            .await
            .expect("snapshot should succeed");

        // Corrupt the .stable file
        std::fs::write(temp_dir.join(STABLE_FILE), b"corrupted content")
            .expect("corrupt stable file");

        let result = restore_binary_snapshot(&temp_dir).await;
        assert!(result.is_err(), "restore should fail with corrupt .stable");
        let err_msg = result.expect_err("should be error").to_string();
        assert!(
            err_msg.contains("pre-restore integrity check failed"),
            "error should mention integrity check: {}",
            err_msg
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_restore_without_manifest_backward_compat() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-restore-v1-compat-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let stable_path = temp_dir.join(STABLE_FILE);
        let binary_dir = temp_dir.join("core/target/release");
        std::fs::create_dir_all(&binary_dir).expect("create binary dir");

        let test_content = b"v1 stable without manifest";
        create_mock_binary(&stable_path, test_content).expect("create stable");

        // No .stable.json — v1 backward compat
        assert!(!temp_dir.join(STABLE_MANIFEST).exists());

        let result = restore_binary_snapshot(&temp_dir).await;
        assert!(
            result.is_ok(),
            "restore should succeed without manifest (v1 compat)"
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_verify_includes_manifest_metadata() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-verify-manifest-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let test_content = b"verify manifest metadata content";
        create_mock_binary(&binary_path, test_content).expect("create mock binary");

        snapshot_binary(&temp_dir, "task-meta", 7)
            .await
            .expect("snapshot should succeed");

        let result = verify_binary_snapshot(&temp_dir)
            .await
            .expect("verify should succeed");

        assert!(result.verified);
        assert!(result.manifest.is_some(), "manifest should be present");
        let m = result.manifest.unwrap();
        assert_eq!(m.task_id, "task-meta");
        assert_eq!(m.cycle, 7);
        assert_eq!(m.version, 2);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_verify_without_manifest() {
        let temp_dir = std::env::temp_dir().join(format!(
            "safety-test-verify-no-manifest-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        let binary_path = temp_dir.join(RELEASE_BINARY_REL);
        let stable_path = temp_dir.join(STABLE_FILE);
        let test_content = b"no manifest verify content";
        create_mock_binary(&binary_path, test_content).expect("create binary");
        create_mock_binary(&stable_path, test_content).expect("create stable");
        // No .stable.json

        let result = verify_binary_snapshot(&temp_dir)
            .await
            .expect("verify should succeed");

        assert!(result.verified);
        assert!(
            result.manifest.is_none(),
            "manifest should be None when .stable.json absent"
        );

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
