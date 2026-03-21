---
self_referential_safe: true
---

# Self-Bootstrap - Binary Snapshot Verification & Integration Test

**Module**: self-bootstrap
**Scope**: Verify new binary snapshot verification function and end-to-end integration test for snapshot → modify → restore → verify workflow
**Scenarios**: 5
**Priority**: High

---

## Background

The binary snapshot mechanism provides a safety layer for self-referential workspaces. This document tests the new verification function that ensures binary integrity after snapshot/restore cycles.

### New Functions Being Tested

- `verify_binary_snapshot(workspace_root: &Path) -> Result<BinaryVerificationResult>` - Verifies that the release binary matches the `.stable` snapshot
- `BinaryVerificationResult` struct - Contains verification results (verified, checksums, paths)

Module: `core/src/scheduler/safety.rs`

---

## Scenario 1: verify_binary_snapshot Returns Match When Binaries Are Identical

### Preconditions
- Rust toolchain available

### Goal
Verify that `verify_binary_snapshot` returns a successful result indicating binaries match when no changes have been made.

### Steps
1. Run the unit test:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_verify_binary_snapshot_matches 2>&1 | tail -5
   ```
2. Code review — verify test creates temp workspace, writes binary, snapshots, then verifies:
   ```bash
   rg -n "test_verify_binary_snapshot_matches" crates/orchestrator-scheduler/src/scheduler/safety/tests.rs
   ```

### Expected
- Unit test passes
- `result.verified` is `true`
- `result.original_checksum` equals `result.current_checksum`

---

## Scenario 2: verify_binary_snapshot Detects Modified Binary

### Preconditions
- Rust toolchain available

### Goal
Verify that `verify_binary_snapshot` correctly detects when the release binary differs from the `.stable` snapshot.

### Steps
1. Run the unit test:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_verify_binary_snapshot_mismatch 2>&1 | tail -5
   ```

### Expected
- Unit test passes
- `result.verified` is `false`
- `result.original_checksum` differs from `result.current_checksum`

---

## Scenario 3: Integration Test - Full Snapshot → Modify → Restore → Verify Cycle

### Preconditions
- Rust toolchain available

### Goal
End-to-end verification that the entire snapshot/restore workflow maintains binary integrity, and the verification function correctly reports the result.

### Steps
1. Run the snapshot + restore round-trip unit tests:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_snapshot_binary_success test_restore_binary_snapshot_success 2>&1 | tail -10
   ```
2. Code review — verify the snapshot → modify → restore → verify cycle logic:
   ```bash
   rg -n "test_snapshot_binary_success|test_restore_binary_snapshot_success" crates/orchestrator-scheduler/src/scheduler/safety/tests.rs
   ```

### Expected
- `test_snapshot_binary_success` passes: snapshot creates `.stable` with correct content
- `test_restore_binary_snapshot_success` passes: restore replaces binary from `.stable`
- Both tests use isolated temp directories (no side effects)

---

## Scenario 4: verify_binary_snapshot Errors When Stable Missing

### Preconditions
- Rust toolchain available

### Goal
Verify that `verify_binary_snapshot` returns an error when the `.stable` snapshot file is missing.

### Steps
1. Run the unit test:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_verify_binary_snapshot_missing_stable 2>&1 | tail -5
   ```

### Expected
- Unit test passes
- Returns error with message containing "no .stable binary snapshot found"

---

## Scenario 5: verify_binary_snapshot Errors When Binary Missing

### Preconditions
- Rust toolchain available

### Goal
Verify that `verify_binary_snapshot` returns an error when the release binary is missing.

### Steps
1. Run the unit test:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_verify_binary_snapshot_missing_binary 2>&1 | tail -5
   ```

### Expected
- Unit test passes
- Returns error with message containing "release binary not found"

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | verify_binary_snapshot Returns Match When Binaries Are Identical | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_matches |
| 2 | verify_binary_snapshot Detects Modified Binary | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_mismatch |
| 3 | Integration Test - Full Snapshot → Modify → Restore → Verify Cycle | ✅ | 2026-03-04 | claude-sonnet-4-6 | binary_snapshot_smoke_verify_integration (integration_test.rs) |
| 4 | verify_binary_snapshot Errors When Stable Missing | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_missing_stable |
| 5 | verify_binary_snapshot Errors When Binary Missing | ✅ | 2026-03-04 | claude-sonnet-4-6 | test_verify_binary_snapshot_missing_binary |
