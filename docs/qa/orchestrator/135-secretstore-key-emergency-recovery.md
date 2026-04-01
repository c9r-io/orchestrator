---
self_referential_safe: false
self_referential_safe_scenarios: [S2, S4, S5, S6, S7, S8]
---

# QA 135: SecretStore Key Emergency Recovery

## FR Reference

FR-089

## Prerequisites

Build check: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` — all tests pass, no clippy warnings.

## Verification Scenarios

### Scenario 1: `bootstrap_key()` creates active key when all keys are terminal

**Steps:**
1. `cargo test -p orchestrator-security bootstrap_creates_key_when_no_active -- --nocapture`

**Expected:** Test passes — after force-revoking the only active key, `bootstrap_key()` creates a new active key and records a `KeyBootstrapped` audit event.

### Scenario 2: `bootstrap_key()` fails when an active key already exists

**Steps:**
1. `cargo test -p orchestrator-security bootstrap_fails_when_active_key_exists -- --nocapture`

**Expected:** Test passes — `bootstrap_key()` returns error containing "active key already exists".

### Scenario 3: Revoking last active key without `--force` shows enhanced warning

**Steps:**
1. `cargo test -p orchestrator-security revoke_last_active_key_warns -- --nocapture`

**Expected:** Test passes — error message contains "last active key" and "bootstrap".

### Scenario 4: CLI `Bootstrap` subcommand is registered

**Steps:**
1. `rg 'Bootstrap' crates/cli/src/cli.rs`

**Expected:** `Bootstrap` variant exists in `SecretKeyCommands` enum with doc comment mentioning emergency recovery.

### Scenario 5: Proto definition for `SecretKeyBootstrap` RPC

**Steps:**
1. `rg 'SecretKeyBootstrap' crates/proto/orchestrator.proto`

**Expected:** RPC definition `rpc SecretKeyBootstrap(SecretKeyBootstrapRequest) returns (SecretKeyBootstrapResponse);` exists. Request message is empty, response has `message` and `key_id` fields.

### Scenario 6: `KeyBootstrapped` audit event kind exists

**Steps:**
1. `rg 'KeyBootstrapped' crates/orchestrator-security/src/secret_key_audit.rs`

**Expected:** `KeyBootstrapped` variant in `KeyAuditEventKind`, with `as_str()` returning `"key_bootstrapped"` and `from_str_value()` parsing it back.

### Scenario 7: Daemon handler wired up

**Steps:**
1. `rg 'secret_key_bootstrap' crates/daemon/src/server/mod.rs`

**Expected:** `secret_key_bootstrap` method delegates to `secret::secret_key_bootstrap`.

### Scenario 8: Revoke error message includes recovery guidance

**Steps:**
1. `rg "secret key bootstrap" crates/orchestrator-security/src/secret_key_lifecycle.rs`

**Expected:** The revoke error for the last active key mentions `secret key bootstrap` as the recovery path.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | `bootstrap_key()` creates active key when all keys are terminal | | | | |
| 2 | `bootstrap_key()` fails when an active key already exists | | | | |
| 3 | Revoking last active key without `--force` shows enhanced warning | | | | |
| 4 | CLI `Bootstrap` subcommand is registered | | | | |
| 5 | Proto definition for `SecretKeyBootstrap` RPC | | | | |
| 6 | `KeyBootstrapped` audit event kind exists | | | | |
| 7 | Daemon handler wired up | | | | |
| 8 | Revoke error message includes recovery guidance | | | | |
