# FR Watch Template

> **Purpose**: Monitor FR document creation via webhook, triggering automated FR governance — demonstrates webhook Trigger and CEL payload filtering.

## Use Cases

- Automatically triage and plan new Feature Requests when they land in `docs/feature_request/`
- Connect file system events (fswatch / inotifywait / GitHub Actions) to orchestrator automation
- Any event-driven "file change → webhook → task" pipeline

## Prerequisites

- `orchestratord` is running with `--webhook-bind 127.0.0.1:9090`
- Database initialized (`orchestrator init`)

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/fr-watch.yaml --project fr-watch
```

### 2. Simulate a New FR Event

```bash
# Option A: CLI trigger fire
orchestrator trigger fire fr-file-created --project fr-watch \
  --payload '{"file":"docs/feature_request/FR-099-new-feature.md"}'

# Option B: curl webhook
curl -X POST http://127.0.0.1:9090/webhook/fr-file-created \
  -H "Content-Type: application/json" \
  -d '{"file":"docs/feature_request/FR-099-new-feature.md"}'
```

### 3. Inspect Results

```bash
orchestrator task list --project fr-watch
orchestrator task logs <task_id>
```

### 4. Connect a Real File Watcher (Optional)

```bash
# macOS: fswatch
fswatch -0 docs/feature_request/ | while read -d '' file; do
  [[ "$file" == *FR-*.md ]] && \
  curl -X POST http://127.0.0.1:9090/webhook/fr-file-created \
    -H "Content-Type: application/json" \
    -d "{\"file\":\"$file\"}"
done

# Linux: inotifywait
inotifywait -m docs/feature_request/ -e create --format '%w%f' | while read file; do
  [[ "$file" == *FR-*.md ]] && \
  curl -X POST http://127.0.0.1:9090/webhook/fr-file-created \
    -H "Content-Type: application/json" \
    -d "{\"file\":\"$file\"}"
done
```

## Workflow Steps

```
fr_triage (fr-governance-agent) → fr_plan (fr-governance-agent)
```

1. **fr_triage** — Triage the new FR: assess priority, complexity, and dependencies
2. **fr_plan** — Draft an implementation plan: break into tasks, identify modules, define acceptance criteria

### Key Feature: Webhook Trigger + CEL Filter

```yaml
kind: Trigger
metadata:
  name: fr-file-created
spec:
  event:
    source: webhook
    filter: "has(payload.file) && payload.file.startsWith('docs/feature_request/FR-')"
  action:
    workflow: fr_governance
    workspace: default
    goal: "Triage and plan newly created feature request"
    start: true
  concurrency_policy: Forbid
```

- `event.source: webhook` — accepts HTTP POST events
- `filter` — CEL expression filters the payload; only FR file paths trigger the workflow
- `concurrency_policy: Forbid` — prevents conflicts when multiple FRs arrive simultaneously

### Cron vs Webhook Triggers

| Aspect | Cron (scheduled-scan) | Webhook (fr-watch) |
|--------|----------------------|-------------------|
| Trigger | Time-based | Event-driven |
| Use Case | Periodic tasks | Responsive tasks |
| Latency | Depends on schedule interval | Real-time |
| External Dependency | None | Requires event source (fswatch/CI/API) |

## Customization Guide

### Modify CEL Filter

```yaml
# Only monitor P0-priority FRs (requires priority field in payload)
filter: "has(payload.priority) && payload.priority == 'P0'"

# Monitor any markdown file creation
filter: "has(payload.file) && payload.file.endsWith('.md')"
```

### Replace with a Real Agent

See [Hello World Customization Guide](/en/showcases/hello-world#replace-with-a-real-agent). With a real agent, the fr_triage step will actually read the FR document and produce a triage report.

### Add HMAC Signature Verification

Recommended for production:

```yaml
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: webhook-signing-keys
      signatureHeader: X-Webhook-Signature
    filter: "has(payload.file) && payload.file.startsWith('docs/feature_request/FR-')"
```

## Further Reading

- [Secret Rotation Workflow](/en/showcases/secret-rotation-workflow) — Another trigger-driven workflow
- [Scheduled Scan Template](/en/showcases/scheduled-scan) — Cron Trigger example
- [Advanced Features](/en/guide/advanced-features) — Trigger resource details
