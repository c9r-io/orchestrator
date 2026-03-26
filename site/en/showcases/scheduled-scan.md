# Scheduled Scan Template

> **Purpose**: Cron-triggered security/compliance audit — demonstrates the Trigger resource and cron scheduling.

## Use Cases

- Periodic security scanning: dependency vulnerability detection, hardcoded credential checks
- Compliance audits: periodic configuration and security policy verification
- Any periodically recurring audit task

## Prerequisites

- `orchestratord` is running (cron triggers require `--webhook-bind`)
- Database initialized (`orchestrator init`)

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/scheduled-scan.yaml --project scan
```

### 2. Manual Run (Test)

```bash
orchestrator task create \
  --name "scan-1" \
  --goal "Run security audit" \
  --workflow scheduled_scan \
  --project scan
```

### 3. Inspect Results

```bash
orchestrator task list --project scan
orchestrator task logs <task_id>
```

### 4. Verify Trigger Registration

```bash
orchestrator get triggers --project scan
```

The `weekly-scan` cron trigger will automatically create new tasks every Monday at 3:00 AM UTC.

## Workflow Steps

```
audit (scan-agent) — single step, fixed loop with 1 cycle
```

### Key Feature: Trigger

```yaml
kind: Trigger
metadata:
  name: weekly-scan
spec:
  cron:
    schedule: "0 3 * * 1"    # Every Monday at 3:00 AM
    timezone: "UTC"
  action:
    workflow: scheduled_scan
    workspace: default
    goal: "Weekly automated security audit"
    start: true              # Auto-start after creation
  concurrency_policy: Forbid  # Prevent overlapping executions
```

- `cron.schedule` — Standard cron expression
- `cron.timezone` — Timezone control
- `action.start: true` — Task starts automatically after creation
- `concurrency_policy: Forbid` — Skips this trigger if the previous task is still running

## Customization Guide

### Adjust Frequency

```yaml
# Daily at 2:00 AM
schedule: "0 2 * * *"

# Every 6 hours
schedule: "0 */6 * * *"

# Monthly on the 1st
schedule: "0 0 1 * *"
```

### Replace with a Real Scan Agent

Swap the echo command for a real agent and customize the StepTemplate prompt:

```yaml
# Agent
command: claude -p "{prompt}" --verbose --output-format stream-json

# StepTemplate prompt — customize for your tech stack
prompt: >-
  Run `cargo audit` to check for known vulnerabilities.
  Run `rg -n 'password|secret|api_key' --type rust` to find hardcoded secrets.
  Report all findings with severity levels.
```

### Add a Webhook Trigger

In addition to cron, trigger scans via webhook events:

```yaml
kind: Trigger
metadata:
  name: on-push-scan
spec:
  source: webhook
  filter: "payload.ref == 'refs/heads/main'"
  action:
    workflow: scheduled_scan
    workspace: default
    goal: "Post-push security scan"
    start: true
```

## Further Reading

- [Secret Rotation Workflow](/en/showcases/secret-rotation-workflow) — Production cron trigger example (secret rotation)
- [Content Promotion](/en/showcases/promotion-execution) — Another cron-triggered workflow (content distribution)
- [Advanced Features](/en/guide/advanced-features) — Trigger resource details
