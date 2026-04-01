# Secret key rotation interrupted by SIGTERM leaves daemon unable to start

- **Observed during**: full-qa-execution.md, during qa_testing phase, cycle 1
- **Severity**: critical
- **Symptom**: Daemon cannot start after SIGTERM because SecretStore data remains encrypted with a revoked key (`k-20260401024224-157a`). The key rotation to `k-20260401030019-2ed1` was interrupted mid-way.
- **Expected**: Either (a) the rotation should be atomic/resumable on next startup, or (b) the daemon should detect the incomplete rotation and auto-resume before validating SecretStore entries.
- **Evidence**:
  - Daemon log: `received SIGTERM, shutting down sender_pid=45264` at 2026-04-01T03:00:27Z
  - Key `k-20260401024224-157a` is `revoked` in DB but data is still encrypted with it
  - Key `k-20260401030019-2ed1` is `active` (created at 03:00:19Z — during shutdown window)
  - Key `k-20260331073755-aa77` is `decrypt_only` (from earlier incomplete rotation)
  - Startup error: `no decryption key available for key_id 'k-20260401024224-157a'`
  - DB query: `SELECT key_id, state FROM secret_keys` shows the inconsistent state
- **Root cause**: A QA agent likely executed `secret key rotate` during the full-qa regression. The rotation started at ~02:42Z, creating key `157a`, then another rotation at ~03:00Z created `2ed1`. The daemon received SIGTERM at 03:00:27Z before the rotation could re-encrypt all SecretStore entries from `157a` to `2ed1`.
- **Workaround**: Manually update the key state in the DB to allow decryption:
  ```sql
  UPDATE secret_keys SET state = 'decrypt_only' WHERE key_id = 'k-20260401024224-157a';
  ```
  Then restart the daemon and run `secret key rotate --resume`.
- **SIGTERM 来源**: PID 45264 为 full-qa 任务的 QA agent 子进程，正在执行 `64-secretstore-key-lifecycle.md` S2 (`orchestrator secret key rotate`)。
  该 agent 进程触发密钥轮换后，shell timeout 或 long-lived-command-guard 机制可能向 daemon 发送了 SIGTERM。
- **根本原因**: QA 文档 `64-secretstore-key-lifecycle.md` 错误标记为 `self_referential_safe: true`，
  导致全量 QA 回归中 S2 场景在自引用模式下执行了真实密钥轮换操作。
- **修复方案**:
  1. 代码修复: `build_keyring_from_records()` 自动恢复 revoked-but-referenced 密钥（`secret_key_lifecycle.rs`）
  2. 文档修复: 将 QA-64 和 QA-135 标记为 `self_referential_safe: false` 并指定安全场景
- **Status**: fixing
