---
self_referential_safe: true
---

# QA 133: Daemon Configuration Hot Reload

## FR Reference

FR-086

## Prerequisites

Read-only code inspection. No daemon interaction required.

**Build check:** `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` — all tests pass, no clippy warnings.

## Verification Scenarios

### Scenario 1: ArcSwap update in persist path

**Steps:**
1. `rg 'set_config_runtime_snapshot' core/src/config_load/persist.rs`

**Expected:** `persist_config_and_reload()` calls `set_config_runtime_snapshot()` to atomically update the in-memory config snapshot.

### Scenario 2: Apply path calls persist_config_and_reload

**Steps:**
1. `rg 'persist_config_and_reload' core/src/service/resource.rs`

**Expected:** `apply_manifests()` calls `persist_config_and_reload()` before returning the gRPC response.

### Scenario 3: Trigger reload notification after apply

**Steps:**
1. `rg 'notify_trigger_reload' core/src/service/resource.rs`

**Expected:** `notify_trigger_reload()` is called after `persist_config_and_reload()` in the apply, delete, and trigger-update paths.

### Scenario 4: Webhook handler reads from ArcSwap

**Steps:**
1. `rg 'read_active_config|fire_trigger' crates/daemon/src/webhook.rs`

**Expected:** Webhook handler calls `read_active_config()` and `fire_trigger()`, both of which read from the ArcSwap config snapshot.

### Scenario 5: fire_trigger reads from config_runtime

**Steps:**
1. `rg 'read_active_config' core/src/service/resource.rs | grep fire_trigger -A 5` or inspect `fire_trigger()` function

**Expected:** `fire_trigger()` calls `read_active_config(state)` which loads from `config_runtime` ArcSwap.

## Checklist

- [x] S1: ArcSwap update in persist path — **PASS**
- [x] S2: Apply path calls persist_config_and_reload — **PASS**
- [x] S3: Trigger reload notification after apply — **PASS**
- [x] S4: Webhook handler reads from ArcSwap — **PASS**
- [x] S5: fire_trigger reads from config_runtime — **PASS**
- [x] Build: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` — **PASS**
