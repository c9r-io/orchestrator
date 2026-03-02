use crate::state::InnerState;
use anyhow::{Context, Result};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryVerificationResult {
    pub verified: bool,
    pub original_checksum: String,
    pub current_checksum: String,
    pub stable_path: PathBuf,
    pub binary_path: PathBuf,
}

pub async fn verify_binary_snapshot(workspace_root: &Path) -> Result<BinaryVerificationResult> {
    let stable_path = workspace_root.join(".stable");
    let binary_path = workspace_root.join("core/target/release/agent-orchestrator");

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

    let stable_checksum = format!("{:x}", Md5::digest(&stable_content));
    let binary_checksum = format!("{:x}", Md5::digest(&binary_content));

    let verified = stable_checksum == binary_checksum;

    Ok(BinaryVerificationResult {
        verified,
        original_checksum: stable_checksum,
        current_checksum: binary_checksum,
        stable_path,
        binary_path,
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

pub async fn snapshot_binary(workspace_root: &Path) -> Result<PathBuf> {
    let binary_path = workspace_root.join("core/target/release/agent-orchestrator");
    let stable_path = workspace_root.join(".stable");

    if !binary_path.exists() {
        anyhow::bail!(
            "release binary not found at {}; skipping binary snapshot",
            binary_path.display()
        );
    }

    tokio::fs::copy(&binary_path, &stable_path)
        .await
        .with_context(|| {
            format!(
                "failed to snapshot binary from {} to {}",
                binary_path.display(),
                stable_path.display()
            )
        })?;

    Ok(stable_path)
}

pub async fn restore_binary_snapshot(workspace_root: &Path) -> Result<()> {
    let stable_path = workspace_root.join(".stable");
    let binary_path = workspace_root.join("core/target/release/agent-orchestrator");

    if !stable_path.exists() {
        anyhow::bail!(
            "no .stable binary snapshot found at {}",
            stable_path.display()
        );
    }

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
        eprintln!("[self_test] cargo check failed:\n{}", stderr);
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
        eprintln!("[self_test] cargo test --lib failed:\n{}", stderr);
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
            eprintln!("[self_test] manifest validate failed:\n{}", stderr);
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

        let binary_path = temp_dir.join("core/target/release/agent-orchestrator");
        let test_content = b"mock binary content for testing";
        create_mock_binary(&binary_path, test_content).expect("create mock binary");

        let result = snapshot_binary(&temp_dir).await;

        assert!(result.is_ok());
        let stable_path = result.expect("snapshot should succeed");
        assert!(stable_path.exists());
        let restored_content = std::fs::read(&stable_path).expect("read stable snapshot");
        assert_eq!(restored_content, test_content);

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

        let result = snapshot_binary(&temp_dir).await;

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

        let stable_path = temp_dir.join(".stable");
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

        let binary_path = temp_dir.join("core/target/release/agent-orchestrator");
        let original_content = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
        create_mock_binary(&binary_path, &original_content).expect("create original binary");

        snapshot_binary(&temp_dir).await.expect("snapshot binary");
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

        let binary_path = temp_dir.join("core/target/release/agent-orchestrator");
        let stable_path = temp_dir.join(".stable");
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

        let binary_path = temp_dir.join("core/target/release/agent-orchestrator");
        let stable_path = temp_dir.join(".stable");
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

        let binary_path = temp_dir.join("core/target/release/agent-orchestrator");
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

        let stable_path = temp_dir.join(".stable");
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
}
