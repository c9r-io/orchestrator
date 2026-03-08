---
name: orchestrator-guide
description: >-
  Guide for working with the Agent Orchestrator — a CLI tool for AI-native SDLC
  automation. Use when writing or editing YAML manifests (Workspace, Agent,
  Workflow, StepTemplate, CRD), running orchestrator CLI commands, designing
  workflow step pipelines, writing CEL prehook/finalize expressions, or
  configuring self-bootstrap workflows. Triggers: any mention of orchestrator
  config, YAML manifests with "orchestrator.dev/v2", workflow steps, prehooks,
  finalize rules, task create/start/pause, agent capabilities, or StepTemplate
  prompts.
---

# Agent Orchestrator Guide

## Architecture (Client/Server)

The orchestrator uses a **client/server** model over gRPC:

- **`orchestratord`** — daemon process (gRPC server + worker pool). Listens on UDS (`data/orchestrator.sock`) by default, or TCP with `--bind`.
- **`orchestrator`** — thin CLI client that forwards all commands to the daemon via gRPC.

Start the daemon first, then use the CLI:

```bash
orchestrator daemon start            # background (default)
orchestrator daemon start --foreground  # foreground with restart loop
orchestrator daemon status           # check daemon health
orchestrator daemon stop             # graceful shutdown
orchestrator daemon restart          # stop + start
```

## Core Workflow

1. `orchestrator daemon start` — start the daemon
2. `orchestrator init` — create SQLite schema
3. `orchestrator apply -f manifest.yaml` — load resources
4. `orchestrator task create --name X --goal Y --workflow Z` — create and run a task
5. `orchestrator task info <id>` / `task logs <id>` — inspect results

## Resource Kinds

All resources use `apiVersion: orchestrator.dev/v2` with `metadata.name` and `spec`.

| Kind | Purpose |
|------|---------|
| Workspace | File system context: root_path, qa_targets, ticket_dir, self_referential |
| Agent | Execution unit: capabilities list + command template with `{prompt}` |
| StepTemplate | Prompt content with pipeline variables (`{goal}`, `{diff}`, `{source_tree}`, etc.) |
| Workflow | Step pipeline + loop policy + finalize rules + safety config |
| WorkflowStore | Cross-task persistent key-value store (WP01) |
| CustomResourceDefinition | Extensible resource types with JSON Schema + CEL validation |

## Minimal Manifest Example

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: my_agent
spec:
  capabilities: [qa]
  command: "echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: simple
spec:
  steps:
    - id: qa
      enabled: true
  loop:
    mode: once
```

## Step Definition Quick Reference

```yaml
- id: plan              # required, unique
  scope: task           # task (once/cycle) | item (per QA file)
  enabled: true         # required
  template: plan        # StepTemplate name (agent steps)
  builtin: self_test    # OR builtin handler name
  command: "cargo check" # OR direct shell command
  prehook:              # conditional gate (CEL)
    engine: cel
    when: "is_last_cycle"
  behavior:
    on_failure: { action: continue }
    post_actions:
      - type: store_put
        store: context
        key: result
        from_var: plan_output
```

Auto-inferred from `id`: builtin IDs → builtin mode; agent IDs → capability mode. See references for full lists.

## Agent Structured Output

Agents must produce JSON on stdout:

```json
{"confidence": 0.95, "quality_score": 0.9, "artifacts": [{"kind": "analysis", "findings": [{"title": "X", "description": "Y", "severity": "info"}]}]}
```

## Reference Files

Load these as needed for detailed specifications:

- **Resource model, pipeline variables, step fields**: Read [references/resource-and-steps.md](references/resource-and-steps.md)
- **CEL prehook/finalize variables and patterns**: Read [references/cel-expressions.md](references/cel-expressions.md)
- **CLI command reference**: Read [references/cli-reference.md](references/cli-reference.md)
- **Advanced features (CRD, Store, Spawn, Invariants, Self-Bootstrap)**: Read [references/advanced.md](references/advanced.md)
