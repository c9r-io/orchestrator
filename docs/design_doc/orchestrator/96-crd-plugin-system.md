# Design Doc: CRD Plugin System

## Summary

Extends CRD from pure data/config definitions into runtime-capable definitions. Users defining a CRD can attach **plugins** — named, typed extensions that inject custom logic at well-defined phases in the daemon pipeline. This replaces hardcoded platform-specific handling with a generic, user-extensible model.

## Motivation

FR-081/082 solved per-trigger authentication and integration packaging. However, platform-specific logic (signature verification algorithms, payload normalization) was still external. FR-083 enables CRDs themselves to carry runtime behavior, making integrations fully self-contained.

## Design

### Plugin Schema

CRD definitions gain a `plugins` list. Each plugin has:

- `name`: unique identifier within the CRD
- `type`: `interceptor`, `transformer`, or `cron` (extensible)
- `phase`: when the plugin runs (e.g., `webhook.authenticate`, `webhook.transform`)
- `command`: shell command to execute
- `timeout`: seconds before kill (default 5)
- `schedule`/`timezone`: for cron-type plugins

### Plugin Types

| Type | Phase | Contract |
|------|-------|----------|
| interceptor | webhook.authenticate | env: WEBHOOK_BODY, WEBHOOK_HEADER_*; exit 0=accept |
| transformer | webhook.transform | stdin: JSON; stdout: transformed JSON |
| cron | (implicit) | env: CRD_KIND, PLUGIN_NAME; scheduled by daemon |

### CRD-Trigger Linkage

Triggers reference CRD plugins via `crdRef` on webhook config:

```yaml
spec:
  event:
    source: webhook
    webhook:
      crdRef: SlackIntegration
```

When `crdRef` is set, CRD interceptors replace built-in HMAC; transformers modify payload before trigger matching.

### Auto-Registration

CRD cron plugins are automatically included in the trigger engine's cron schedule via `build_cron_schedule()`. Apply/delete triggers config reload, which rebuilds the schedule.

### Built-in Tool Library

`orchestrator tool` subcommands callable from plugin scripts:
- `webhook-verify-hmac`: HMAC-SHA256 verification
- `payload-extract`: JSON path extraction from stdin
- `secret-rotate`: atomic SecretStore key update via gRPC

## Key Files

- `crates/orchestrator-config/src/crd_types.rs` — `CrdPlugin` struct
- `crates/orchestrator-config/src/config/trigger.rs` — `crd_ref` field
- `core/src/crd/plugins.rs` — execution engine
- `core/src/crd/validate.rs` — plugin validation
- `crates/daemon/src/webhook.rs` — webhook interceptor/transformer injection
- `core/src/trigger_engine.rs` — CRD cron scheduling
- `crates/cli/src/commands/tool.rs` — built-in tools

## Trade-offs

- **Synchronous script execution in hot path**: Interceptors/transformers run synchronously with a timeout. Suitable for lightweight scripts; long-running logic should use cron plugins instead.
- **Open type/phase strings**: New plugin types can be added without schema changes, but unknown types are rejected at apply time for safety.
- **Shell-based execution**: Uses `sh -c` for simplicity and universal compatibility; future work could add WASM or native plugin support.
