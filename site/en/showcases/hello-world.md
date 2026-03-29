# Hello World Template

> **Harness Engineering template**: this showcase demonstrates one concrete capability slice of orchestrator as a control plane for agent-first software delivery.
>
> **Purpose**: The simplest runnable workflow — one Workspace, one Agent, one Workflow. Zero API cost.

## Use Cases

- First contact with orchestrator — verify installation and basic flow
- Understand the Workspace → Agent → Workflow resource relationship
- Starting skeleton for custom workflows

## Prerequisites

- `orchestratord` is running (`orchestratord --foreground --workers 2`)
- Database initialized (`orchestrator init`)

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/hello-world.yaml --project hello-world
```

### 2. Verify Resources

```bash
orchestrator get workspaces --project hello-world
orchestrator get agents --project hello-world
orchestrator get workflows --project hello-world
```

### 3. Create and Run a Task

```bash
orchestrator task create \
  --name "hello" \
  --goal "Say hello" \
  --workflow hello \
  --project hello-world
```

### 4. Inspect Results

```bash
orchestrator task list --project hello-world
orchestrator task info <task_id>
orchestrator task logs <task_id>
```

## Expected Output

The echo agent returns a fixed JSON response:

```json
{
  "confidence": 0.95,
  "quality_score": 0.9,
  "artifacts": [{
    "kind": "analysis",
    "findings": [{
      "title": "hello-world",
      "description": "Workflow executed successfully.",
      "severity": "info"
    }]
  }]
}
```

The task completes within seconds with status `Completed`.

## Customization Guide

### Replace with a Real Agent

Swap the echo agent's `command` for a real AI agent:

```yaml
# Claude Code
command: claude -p "{prompt}" --verbose --output-format stream-json

# OpenCode
command: opencode -p "{prompt}"
```

You will need to configure the corresponding API key (via SecretStore or environment variables).

### Add More Steps

Add steps to the Workflow's `steps` list and ensure the Agent has the matching `capability`.

## Further Reading

- [Quick Start](/en/guide/quickstart) — Full 5-minute onboarding tutorial
- [Resource Model](/en/guide/resource-model) — Deep dive into resource kinds
- [QA Loop Template](/en/showcases/qa-loop) — Next step: multi-step workflows
