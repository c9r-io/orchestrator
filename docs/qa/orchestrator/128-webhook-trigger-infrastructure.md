---
self_referential_safe: true
---

# QA 128: Webhook Trigger Infrastructure

## FR Reference

FR-080

## Prerequisites

Daemon must already be running (webhook server enabled by default on `127.0.0.1:19090`).

## Verification Scenarios

### Scenario 1: Webhook health endpoint

**Steps:**
1. `curl -s http://127.0.0.1:19090/health`

**Expected:** Returns "ok".

### Scenario 2: Webhook fires a trigger

**Steps:**
1. Apply a trigger manifest with `source: webhook`
2. `curl -X POST http://127.0.0.1:19090/webhook/<trigger_name> -d '{"key":"value"}'`

**Expected:** Returns 200 with `{"task_id":"...","trigger":"...","status":"fired"}`.

### Scenario 3: Webhook with project scope

**Steps:**
1. `curl -X POST http://127.0.0.1:19090/webhook/myproject/my-trigger -d '{}'`

**Expected:** Trigger fires in project "myproject".

### Scenario 4: HMAC signature verification

**Steps:**
1. Verify daemon was started with `--webhook-secret` (or `ORCHESTRATOR_WEBHOOK_SECRET` env var)
2. `curl -X POST http://127.0.0.1:19090/webhook/test -d '{}' -H 'X-Webhook-Signature: sha256=invalid'`

**Expected:** Returns 401 "invalid signature".

### Scenario 5: Missing signature rejected

**Steps:**
1. Verify daemon was started with `--webhook-secret`
2. `curl -X POST http://127.0.0.1:19090/webhook/test -d '{}'`

**Expected:** Returns 401 "missing signature".

### Scenario 6: Webhook disabled via --webhook-bind none

**Steps:**
1. Verify `orchestratord --help` shows `--webhook-bind` with default `127.0.0.1:19090` and accepts `none`

**Expected:** Help text documents the flag. (Actual disable test requires daemon restart — tested in `scripts/qa/test-webhook-trigger.sh`.)

### Scenario 7: Custom bind address override

**Steps:**
1. Verify `orchestratord --help` shows `--webhook-bind` accepts a custom address

**Expected:** Help text documents the override capability. (Actual bind test requires daemon restart — tested in `scripts/qa/test-webhook-trigger.sh`.)

### Scenario 8: Webhook source accepted in manifest

**Steps:**
1. Apply trigger with `event.source: webhook`

**Expected:** `orchestrator apply` succeeds (no validation error on source field).

### Scenario 9: Compilation and tests

**Steps:**
1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`

**Expected:** All tests pass, no clippy warnings.

## Checklist

- [x] S1: Webhook health endpoint — **PASS** (returns "ok")
- [x] S2: Webhook fires a trigger — **PASS** (config hot-reload via ArcSwap; see design doc `92-daemon-config-hot-reload.md`)
- [x] S3: Webhook with project scope — **PASS** (same mechanism as S2)
- [x] S4: HMAC signature verification — **PASS** (invalid signature rejected)
- [x] S5: Missing signature rejected — **PASS** (missing signature rejected)
- [x] S6: Webhook disabled via --webhook-bind none — **PASS** (help text correct)
- [x] S7: Custom bind address override — **PASS** (help text correct)
- [x] S8: Webhook source accepted in manifest — **PASS** (manifest validated successfully)
- [ ] S9: Compilation and tests — **SKIPPED** (unsafe in self-referential mode)
