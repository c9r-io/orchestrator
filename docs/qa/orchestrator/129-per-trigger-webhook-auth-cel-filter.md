---
self_referential_safe: true
---

# QA 129: Per-Trigger Webhook Auth & CEL Payload Filter

## FR Reference

FR-081

## Prerequisites

Daemon must already be running (webhook server enabled by default on `0.0.0.0:19090`).

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
1. Apply SecretStore with signing key
2. Apply Trigger with `webhook.secret.fromRef: <store>`
3. `curl -X POST http://127.0.0.1:19090/webhook/<trigger> -d '{}' -H 'X-Webhook-Signature: sha256=<valid>'`

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

- [x] Scenario 1: Webhook config types compile
- [x] Scenario 2: Webhook source with webhook config accepted
- [x] Scenario 3: Per-trigger secret from SecretStore
- [x] Scenario 4: Invalid per-trigger signature rejected
- [x] Scenario 5: Multi-key rotation

## Notes

- **Daemon rebuild required**: The running daemon binary must be rebuilt with `cargo build --release -p orchestratord` before testing, as the webhook auth code was updated after the last daemon build.
- **Default signature header**: The default header is `x-webhook-signature` (lowercase); HTTP header matching is case-insensitive so `X-Webhook-Signature` also works.
- **Signature format**: HMAC-SHA256 hex-encoded, with optional `sha256=` prefix.
