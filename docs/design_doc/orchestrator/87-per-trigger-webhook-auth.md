# Design Doc 87: Per-Trigger Webhook Auth & CEL Payload Filter

## FR Reference

FR-081

## Design Decisions

### Per-Trigger Secret from SecretStore

Each webhook trigger can reference a SecretStore via `webhook.secret.fromRef`. At request time, the webhook handler reads the active config, resolves the store, and tries **all values** in the store for HMAC verification. This supports key rotation without downtime — add a new key, apply, remove the old key later.

```yaml
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: my-signing-keys   # SecretStore with multiple keys
      signatureHeader: X-Custom-Sig  # Custom header name
```

### Fallback Chain

1. Per-trigger `webhook.secret.fromRef` → resolve from SecretStore, try all keys
2. Global `--webhook-secret` → single shared secret
3. Neither → no verification (request allowed)

### CEL Payload Filtering

`filter.condition` is evaluated via `cel-interpreter` against the webhook JSON body. Top-level fields are injected as `payload_{field_name}` variables (string, number, bool) for direct CEL access.

```yaml
filter:
  condition: "payload_type == 'message'"
```

### Config Types

Added `TriggerWebhookConfig` + `TriggerSecretRef` to stored config, and `TriggerWebhookSpec` + `WebhookSecretRef` to CLI/YAML types. Conversion in `to_config()`/`from_config()` handles both directions.

## Files Modified

| File | Change |
|------|--------|
| `crates/orchestrator-config/src/config/trigger.rs` | Added TriggerWebhookConfig, TriggerSecretRef |
| `crates/orchestrator-config/src/cli_types.rs` | Added TriggerWebhookSpec, WebhookSecretRef |
| `core/src/resource/trigger.rs` | Updated to_config/from_config, accept "webhook" source |
| `core/src/prehook/cel.rs` | Added evaluate_webhook_filter() + 6 unit tests |
| `core/src/prehook/mod.rs` | Re-exported evaluate_webhook_filter |
| `core/src/trigger_engine.rs` | Implemented CEL condition evaluation in handle_event_trigger |
| `crates/daemon/src/webhook.rs` | Per-trigger secret resolution with multi-key rotation + fallback |
