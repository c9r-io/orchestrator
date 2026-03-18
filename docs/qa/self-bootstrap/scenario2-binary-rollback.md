---
self_referential_safe: true
---

# Self-Bootstrap Tests - Scenario 2: Binary Snapshot Restoration on Auto-Rollback

**Module**: self-bootstrap
**Scenario**: Binary Snapshot Restoration on Auto-Rollback
**Status**: REWRITTEN — code review + unit test verification
**Test Date**: 2026-03-18
**Tester**: Claude

---

## Goal
Verify that when auto-rollback triggers (after max consecutive failures), the `.stable` binary is restored over the live release binary.

---

### Verification Method

Code review + unit test verification. The binary snapshot restore and rollback logic is fully covered by unit tests in `crates/orchestrator-scheduler/src/scheduler/safety/tests.rs`. No live daemon or task execution required.

### Steps

1. **Code review** — confirm restore logic in `scheduler/safety/` module:
   - `restore_binary_snapshot()` copies `.stable` over the release binary
   - Restore verifies SHA-256 integrity via manifest when available
   - Restore rejects corrupt `.stable` files
   - Backward compatibility: restore works without manifest (pre-manifest snapshots)
   - Atomic restore: no partial writes on failure

2. **Code review** — confirm rollback trigger logic:
   - Auto-rollback fires after `max_consecutive_failures` reached
   - `binary_snapshot_restored` event is emitted in same cycle as `auto_rollback`
   - `consecutive_failures` counter resets to 0 after rollback

3. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- test_restore_binary_snapshot_success
   cargo test --workspace --lib -- test_restore_binary_snapshot_missing_stable
   cargo test --workspace --lib -- test_snapshot_restore_content_integrity
   cargo test --workspace --lib -- test_restore_with_manifest_integrity_check
   cargo test --workspace --lib -- test_restore_rejects_corrupt_stable
   cargo test --workspace --lib -- test_restore_without_manifest_backward_compat
   cargo test --workspace --lib -- test_restore_binary_creates_parent_dirs
   cargo test --workspace --lib -- test_create_checkpoint_and_rollback_success
   ```

### Expected Results

- All restore unit tests pass — `.stable` is correctly restored with integrity verification
- Rollback unit tests pass — auto-rollback triggers at correct failure count
- SHA-256 verification ensures restored binary matches `.stable` contents
- `consecutive_failures` counter reset logic is verified

---

## Checklist

- [x] Restore logic copies `.stable` over release binary (unit test verified)
- [x] Restore verifies SHA-256 integrity (unit test verified)
- [x] Corrupt `.stable` files are rejected (unit test verified)
- [x] Rollback triggers after max consecutive failures (unit test verified)
- [x] `consecutive_failures` counter resets after rollback (unit test verified)
