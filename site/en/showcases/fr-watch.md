# FR Watch Template

> **Harness Engineering template**: this showcase demonstrates one concrete capability slice of orchestrator as a control plane for agent-first software delivery.
>
> **Purpose**: Monitor FR document creation via native filesystem trigger, automating FR governance — demonstrates filesystem Trigger and CEL payload filtering.

## Use Cases

- Automatically triage and plan new Feature Requests when they land in `docs/feature_request/`
- Any event-driven "file change → automatic task" scenario
- No external tools needed (fswatch / curl) — the daemon monitors the filesystem natively

## Prerequisites

- `orchestratord` is running
- Database initialized (`orchestrator init`)
- Project has a `docs/feature_request/` directory

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/fr-watch.yaml --project fr-watch
```

### 2. Trigger: Create an FR File

```bash
echo "# FR-099: New Feature" > docs/feature_request/FR-099-new-feature.md
```

The daemon detects the file creation and automatically creates and starts a task.

### 3. Inspect Results

```bash
orchestrator task list --project fr-watch
orchestrator task logs <task_id>
```

## Workflow Steps

```
fr_triage (fr-governance-agent) → fr_plan (fr-governance-agent)
```

1. **fr_triage** — Triage the new FR: assess priority, complexity, and dependencies
2. **fr_plan** — Draft an implementation plan: break into tasks, identify modules, define acceptance criteria

### Key Feature: Filesystem Trigger

```yaml
kind: Trigger
metadata:
  name: fr-file-created
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - docs/feature_request/
      events:
        - create
      debounce_ms: 500
    filter:
      condition: "payload_filename.endsWith('.md') && payload_filename.startsWith('FR-')"
  action:
    workflow: fr_governance
    workspace: default
    start: true
  concurrencyPolicy: Forbid
```

- `source: filesystem` — native daemon file monitoring (macOS FSEvents / Linux inotify)
- `filesystem.paths` — directories to watch (relative to Workspace root_path)
- `filesystem.events` — only listen for `create`, ignore modify/delete
- `debounce_ms: 500` — 500ms debounce window to merge duplicate events
- `filter.condition` — CEL filter: only `FR-*.md` files trigger the workflow

### CEL Variables (auto-injected)

| Variable | Description |
|----------|-------------|
| `payload_path` | Full path of the changed file |
| `payload_filename` | Filename (e.g. `FR-099-new-feature.md`) |
| `payload_dir` | Parent directory |
| `payload_event_type` | `create` / `modify` / `delete` |
| `payload_timestamp` | Event timestamp |

### Trigger Type Comparison

| Aspect | Filesystem | Cron | Webhook |
|--------|-----------|------|---------|
| Trigger | File changes | Time-based | HTTP POST |
| Latency | Real-time (~500ms debounce) | Depends on schedule | Real-time |
| External Dependency | None | None | Requires event source |
| Typical Use | File monitoring | Periodic tasks | CI/CD integration |

## Customization Guide

### Modify CEL Filter

```yaml
# Watch all markdown files
condition: "payload_filename.endsWith('.md')"

# Only specific prefix
condition: "payload_filename.startsWith('ISSUE-')"

# Watch both create and modify
events:
  - create
  - modify
```

### Replace with a Real Agent

See [Hello World Customization Guide](/en/showcases/hello-world#replace-with-a-real-agent). With a real agent, the fr_triage step will actually read the FR document and produce a triage report.

## Further Reading

- [Scheduled Scan Template](/en/showcases/scheduled-scan) — Cron Trigger example
- [Secret Rotation Workflow](/en/showcases/secret-rotation-workflow) — Webhook Trigger example
- [Advanced Features](/en/guide/advanced-features) — Trigger resource details
