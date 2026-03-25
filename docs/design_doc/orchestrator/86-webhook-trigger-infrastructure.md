# Design Doc 86: Webhook Trigger Infrastructure

## FR Reference

FR-080: Webhook Trigger 基础设施 — HTTP 事件入口与通用事件源扩展

## Design Decisions

### HTTP Server: axum alongside gRPC

The daemon spawns a lightweight axum HTTP server on a separate port when `--webhook-bind` is specified. Default disabled — zero overhead if not configured. Shares `InnerState` with the gRPC server.

Routes:
- `POST /webhook/{trigger_name}` — fire trigger in default project
- `POST /webhook/{project}/{trigger_name}` — fire trigger in specified project
- `GET /health` — readiness check

### Trigger Event Source Extension

Added `"webhook"` as a valid trigger event source alongside `"task_completed"` and `"task_failed"`. Webhook triggers match incoming webhook events and can apply CEL filter conditions on the payload (CEL evaluation deferred to future FR).

### TriggerEventPayload Extension

Added `payload: Option<serde_json::Value>` to `TriggerEventPayload`. This carries the webhook JSON body through the broadcast channel to the trigger engine for event matching and goal construction.

### Goal Construction

When a webhook payload is present, the task goal includes a truncated JSON summary:
```
Triggered by webhook 'my-trigger': {"event":"push","ref":"main"}
```

### HMAC-SHA256 Signature Verification

`--webhook-secret` enables signature verification via the `X-Webhook-Signature` header. Supports both `sha256=<hex>` and raw hex formats. Without the header, requests return 401.

### Direct Fire + Event Broadcast

Webhook requests do both:
1. Direct fire: immediately create a task from the named trigger
2. Event broadcast: send the payload to the trigger engine for event-based matching

This dual path ensures both named triggers and event-filtered triggers work.

### CLI `trigger fire --payload`

The CLI `trigger fire` command now accepts `--payload <JSON>` to simulate webhook payloads for testing. The payload is broadcast as a webhook event to the trigger engine.

## Files Created

| File | Purpose |
|------|---------|
| `crates/daemon/src/webhook.rs` | axum HTTP webhook server |

## Files Modified

| File | Change |
|------|--------|
| `core/src/trigger_engine.rs` | Extended TriggerEventPayload with payload field, webhook matching, goal construction |
| `core/src/state.rs` | Added payload: None to task event broadcasts |
| `core/src/resource/trigger.rs` | Accept "webhook" as valid event source |
| `crates/daemon/src/main.rs` | Added --webhook-bind/--webhook-secret args, HTTP server spawn |
| `crates/daemon/Cargo.toml` | Added axum, hmac, hex dependencies |
| `crates/proto/orchestrator.proto` | Added payload_json to TriggerFireRequest |
| `crates/cli/src/cli.rs` | Added --payload to trigger fire |
| `crates/cli/src/commands/trigger.rs` | Pass payload to RPC |
| `crates/daemon/src/server/trigger.rs` | Broadcast payload as webhook event |
| `crates/gui/src/commands/trigger.rs` | Added payload_json: None |
