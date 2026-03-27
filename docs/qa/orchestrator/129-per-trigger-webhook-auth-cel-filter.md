---
self_referential_safe: true
---

# QA 129: Per-Trigger Webhook Auth & CEL Payload Filter

## FR Reference

FR-081

## Scenario 1: Webhook config types compile

**Steps:**
1. `cargo check --workspace`

**Expected:** No errors. TriggerWebhookConfig, TriggerSecretRef types compile.

## Scenario 2: Webhook source with webhook config accepted

**Steps:**
1. Validate a manifest with `event.source: webhook` and `webhook.secret.fromRef`:
   ```bash
   orchestrator manifest validate -f <manifest>
   ```

**Expected:** Validation passes (no "event.source" error).

## Scenario 3: Per-trigger secret from SecretStore

**Steps:**
1. Start daemon with `--webhook-bind 127.0.0.1:19091`
2. Apply SecretStore with signing key
3. Apply Trigger with `webhook.secret.fromRef: <store>`
4. `curl` with valid HMAC signature

**Expected:** Returns 200 or 404 (trigger fired / trigger not found for default workspace).

## Scenario 4: Invalid per-trigger signature rejected

**Steps:**
1. Same setup as Scenario 3
2. `curl` with wrong signature

**Expected:** Returns 401 "invalid signature (no matching key)".

## Scenario 5: Multi-key rotation

**Steps:**
1. Apply SecretStore with `key1: secret-old` and `key2: secret-new`
2. Sign with `secret-old` → should pass
3. Sign with `secret-new` → should pass
4. Sign with `wrong-secret` → should fail

**Expected:** Both old and new keys accepted, wrong key rejected.

## Checklist

- [ ] Scenario 1: Webhook config types compile
- [ ] Scenario 2: Webhook source with webhook config accepted
- [ ] Scenario 3: Per-trigger secret from SecretStore
- [ ] Scenario 4: Invalid per-trigger signature rejected
- [ ] Scenario 5: Multi-key rotation
