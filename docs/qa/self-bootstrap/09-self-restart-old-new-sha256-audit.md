---
self_referential_safe: true
---

# Self-Bootstrap - Self-Restart Old/New Binary SHA256 Audit Chain

**Module**: self-bootstrap
**Scope**: Enhanced `self_restart_ready` and `binary_verification` event payloads with `old_binary_sha256`, renamed `new_binary_sha256`, and `binary_changed` flag for full old-to-new audit trail
**Scenarios**: 4
**Priority**: Medium

---

## Background

The self-restart step builds a new binary, then records a `self_restart_ready` event before the process restarts. After restart, `verify_post_restart_binary` compares the running binary against the expected hash from that event.

This enhancement adds the **pre-build binary hash** (`old_binary_sha256`) alongside the post-build hash (renamed from `binary_sha256` to `new_binary_sha256`), plus a `binary_changed` boolean. After restart, the `binary_verification` event also carries `old_binary_sha256`, completing the `old -> expected -> actual` audit chain.

Key file:
- `crates/orchestrator-scheduler/src/scheduler/safety/restart.rs` — `execute_self_restart_step`, `verify_post_restart_binary`
- `crates/orchestrator-scheduler/src/scheduler/safety/tests.rs` — unit tests

---

## Scenario 1: self_restart_ready Event Contains Old and New SHA256

### Preconditions
- Unit test environment available (`cargo test --lib`)

### Goal
Verify that when a self-restart step succeeds, the `self_restart_ready` event payload contains `old_binary_sha256`, `new_binary_sha256`, and `binary_changed` fields.

### Steps
1. Confirm the payload fields exist in the source:
   ```bash
   rg -n 'old_binary_sha256|new_binary_sha256|binary_changed' crates/orchestrator-scheduler/src/scheduler/safety/restart.rs
   ```
2. Run the unit test that validates the payload structure:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_execute_self_restart_step_records_old_binary_sha256 2>&1 | tail -5
   ```

### Expected
- Source contains `"old_binary_sha256"`, `"new_binary_sha256"`, and `"binary_changed"` in the `self_restart_ready` event payload
- `binary_sha256` (legacy) is still accepted alongside `new_binary_sha256` for backward compat
- Unit test passes

---

## Scenario 2: binary_changed Flag Is True When Hashes Differ

### Preconditions
- Unit test environment available

### Goal
Verify that `binary_changed` is `true` when `old_binary_sha256 != new_binary_sha256` and neither is `"unknown"`.

### Steps
1. Run the unit test that validates the payload structure (includes `binary_changed` field presence):
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_execute_self_restart_step_records_old_binary_sha256 2>&1 | tail -5
   ```
2. Confirm the `binary_changed` logic in source — `false` when either hash is `"unknown"`:
   ```bash
   rg -n 'binary_changed' crates/orchestrator-scheduler/src/scheduler/safety/restart.rs
   ```

### Expected
- Unit test passes: `self_restart_ready` event contains `old_binary_sha256`, `new_binary_sha256`, and `binary_changed`
- Source confirms `binary_changed` is `true` only when both hashes are valid and differ; `false` when either is `"unknown"`

---

## Scenario 3: binary_verification Event Includes old_binary_sha256

### Preconditions
- Unit test environment available

### Goal
Verify that after restart, `verify_post_restart_binary` propagates `old_binary_sha256` from the stored `self_restart_ready` event into the `binary_verification` event, completing the `old -> expected -> actual` chain.

### Steps
1. Run the unit test for propagation:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_verify_post_restart_binary_includes_old_sha256 2>&1 | tail -5
   ```
2. Confirm the verification event payload structure in source:
   ```bash
   rg -n 'old_binary_sha256|expected_sha256|actual_sha256|verified' crates/orchestrator-scheduler/src/scheduler/safety/restart.rs | head -20
   ```

### Expected
- Unit test passes
- Both `binary_verification` event emissions (match and mismatch branches) include `old_binary_sha256`, `expected_sha256`, `actual_sha256`, and `verified`

---

## Scenario 4: Backward Compatibility — Legacy Events Without old_binary_sha256

### Preconditions
- Unit test environment available

### Goal
Verify that `verify_post_restart_binary` handles legacy `self_restart_ready` events (created before this enhancement) that lack the `old_binary_sha256` field — it should default to `"unknown"` and still function correctly.

### Steps
1. Run the backward-compat unit test:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- test_verify_post_restart_binary_unknown_hash_skips 2>&1 | tail -5
   ```
2. Confirm the fallback logic in source:
   ```bash
   rg -n 'old_binary_sha256.*unwrap_or|unknown' crates/orchestrator-scheduler/src/scheduler/safety/restart.rs | head -5
   ```

### Expected
- Unit test passes: legacy events without `old_binary_sha256` produce a `binary_verification` event with `old_binary_sha256: "unknown"`
- Verification still returns the correct `true`/`false` result based on `new_binary_sha256` match

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | self_restart_ready Event Contains Old and New SHA256 | PASS | 2026-03-28 | qa-testing skill | All fields present; backward compat with legacy binary_sha256 confirmed |
| 2 | binary_changed Flag Is True When Hashes Differ | PASS | 2026-03-28 | qa-testing skill | Logic confirmed: true only when both hashes valid and differ; false when either unknown |
| 3 | binary_verification Event Includes old_binary_sha256 | PASS | 2026-03-28 | qa-testing skill | Both match/mismatch branches include old_binary_sha256, expected_sha256, actual_sha256, verified |
| 4 | Backward Compatibility — Legacy Events Without old_binary_sha256 | PASS | 2026-03-28 | qa-testing skill | Legacy events default to "unknown"; verification still correct |
