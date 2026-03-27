---
self_referential_safe: true
---

# QA 129b: Per-Trigger Webhook Auth & CEL Payload Filter (Advanced)

Continuation of [QA 129](129-per-trigger-webhook-auth-cel-filter.md).

## FR Reference

FR-081

## Prerequisites

Daemon must already be running with `--webhook-secret global-key` (or `ORCHESTRATOR_WEBHOOK_SECRET=global-key`).

## Scenario 6: Global secret fallback

**Steps:**
1. Apply trigger WITHOUT `webhook.secret`
2. `curl -X POST http://127.0.0.1:19090/webhook/<trigger> -d '{}' -H 'X-Webhook-Signature: sha256=<hmac_of_global_key>'`

**Expected:** Returns 200/404 (global fallback used).

## Scenario 7: CEL filter — unit test

**Steps:**
1. `cargo test --lib -p agent-orchestrator -- prehook::cel::tests`

**Expected:** evaluate_webhook_filter tests pass.

## Scenario 8: All tests and clippy

**Steps:**
1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`

**Expected:** All pass, no warnings.

## Checklist

- [ ] Scenario 6: Global secret fallback
- [ ] Scenario 7: CEL filter — unit test
- [ ] Scenario 8: All tests and clippy
