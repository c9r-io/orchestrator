# Scheduled Scan Template

> **Purpose**: Cron-triggered security audit — demonstrates agent-driven security analysis, static checks, and the Trigger resource.

## Use Cases

- Periodic security audits: agent-driven threat modeling + static tool scanning
- Compliance checks: regular architecture security and dependency health reviews
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
agent_audit (scan-agent) → static_check (scan-agent)
```

1. **agent_audit** — Agent-driven security analysis: identify trust boundaries, review auth logic, check injection vectors, assess secrets handling and error exposure
2. **static_check** — Static tool scanning: dependency audit (cargo audit / npm audit), secret scanning, known vulnerability pattern checks

### Why Agent Analysis First?

Traditional static scanning only catches known patterns (CVEs, regex matches). AI agents can:
- Understand security implications in business logic (permission bypasses, TOCTOU races)
- Perform threat modeling (identify trust boundaries and attack surfaces)
- Provide context-aware remediation advice

Static scanning complements by covering mechanical checks the agent might miss (dependency CVEs, hardcoded credential regex, etc.).

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

### Replace with a Real Agent

Swap the echo command for a real agent:

```yaml
command: claude -p "{prompt}" --verbose --output-format stream-json
```

The agent will then perform actual threat modeling and run static scan commands.

### Customize StepTemplate Prompts

Adjust the static_check prompt for your tech stack:

```yaml
prompt: >-
  Run static security checks:
  - `cargo audit` for Rust dependency CVEs
  - `rg -n 'password|secret|api_key' --type rust` for hardcoded secrets
  - Check for `unsafe` blocks without safety comments
```

### Add a Webhook Trigger

Trigger scans via webhook events (e.g., after CI push):

```yaml
kind: Trigger
metadata:
  name: on-push-scan
spec:
  event:
    source: webhook
    filter: "payload.ref == 'refs/heads/main'"
  action:
    workflow: scheduled_scan
    workspace: default
    goal: "Post-push security scan"
    start: true
```

## Further Reading

- [FR Watch Template](/en/showcases/fr-watch) — Webhook Trigger example (file system monitoring)
- [Secret Rotation Workflow](/en/showcases/secret-rotation-workflow) — Production cron trigger example
- [Advanced Features](/en/guide/advanced-features) — Trigger resource details
