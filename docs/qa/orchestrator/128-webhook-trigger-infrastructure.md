# QA 128: Webhook Trigger Infrastructure

## FR Reference

FR-080

## Verification Scenarios

### Scenario 1: Webhook server starts with --webhook-bind

**Steps:**
1. `orchestratord --foreground --workers 1 --webhook-bind 127.0.0.1:9090`
2. `curl -s http://127.0.0.1:9090/health`

**Expected:** Server starts, health endpoint returns "ok".

### Scenario 2: Webhook fires a trigger

**Steps:**
1. Start daemon with `--webhook-bind 127.0.0.1:9090`
2. Apply a trigger manifest with `source: webhook`
3. `curl -X POST http://127.0.0.1:9090/webhook/<trigger_name> -d '{"key":"value"}'`

**Expected:** Returns 200 with `{"task_id":"...","trigger":"...","status":"fired"}`.

### Scenario 3: Webhook with project scope

**Steps:**
1. `curl -X POST http://127.0.0.1:9090/webhook/myproject/my-trigger -d '{}'`

**Expected:** Trigger fires in project "myproject".

### Scenario 4: HMAC signature verification

**Steps:**
1. Start daemon with `--webhook-bind 127.0.0.1:9090 --webhook-secret mysecret`
2. `curl -X POST http://127.0.0.1:9090/webhook/test -d '{}' -H 'X-Webhook-Signature: sha256=invalid'`

**Expected:** Returns 401 "invalid signature".

### Scenario 5: Missing signature rejected

**Steps:**
1. Start daemon with `--webhook-secret mysecret`
2. `curl -X POST http://127.0.0.1:9090/webhook/test -d '{}'`

**Expected:** Returns 401 "missing signature".

### Scenario 6: No webhook server without --webhook-bind

**Steps:**
1. `orchestratord --foreground --workers 1` (no --webhook-bind)
2. `curl http://127.0.0.1:9090/health`

**Expected:** Connection refused. No HTTP server running.

### Scenario 7: trigger fire --payload via CLI

**Steps:**
1. `orchestrator trigger fire my-trigger --payload '{"event":"test"}'`

**Expected:** Trigger fires, payload broadcast as webhook event.

### Scenario 8: Webhook source accepted in manifest

**Steps:**
1. Apply trigger with `event.source: webhook`

**Expected:** `orchestrator apply` succeeds (no validation error).

### Scenario 9: Compilation and tests

**Steps:**
1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`

**Expected:** All tests pass, no clippy warnings.

## Checklist

- [ ] S1: Webhook server starts with --webhook-bind
- [ ] S2: Webhook fires a trigger
- [ ] S3: Webhook with project scope
- [ ] S4: HMAC signature verification
- [ ] S5: Missing signature rejected
- [ ] S6: No webhook server without --webhook-bind
- [ ] S7: trigger fire --payload via CLI
- [ ] S8: Webhook source accepted in manifest
- [ ] S9: Compilation and tests
