---
name: orchestrator-guide
description: >-
  Guide for working with the Agent Orchestrator — a CLI tool for AI-native SDLC
  automation. Use when writing or editing YAML manifests (Workspace, Agent,
  Workflow, StepTemplate, CRD), running orchestrator CLI commands, designing
  workflow step pipelines, writing CEL prehook/finalize expressions, or
  configuring self-bootstrap workflows. Triggers: any mention of orchestrator
  config, YAML manifests with "orchestrator.dev/v2", workflow steps, prehooks,
  finalize rules, task create/start/pause, orchestrator run, step filtering,
  direct assembly, agent capabilities, or StepTemplate prompts.
---

# Agent Orchestrator Guide

## Architecture (Client/Server)

The orchestrator uses a **client/server** model over gRPC:

- **`orchestratord`** — daemon binary (gRPC server + embedded worker pool). Listens on UDS (`data/orchestrator.sock`) by default, or TCP with `--bind`.
- **`orchestrator`** — thin CLI client binary that forwards all commands to the daemon via gRPC. No core library dependency.

Binary locations after `cargo build --release -p orchestratord -p orchestrator-cli`:
- `target/release/orchestratord` — daemon
- `target/release/orchestrator` — CLI client

Start the daemon first, then use the CLI:

```bash
# Start daemon (separate binary, not a CLI subcommand)
orchestratord --foreground --workers 2           # foreground (recommended for monitoring)
nohup orchestratord --foreground --workers 2 &   # background via nohup
orchestratord --bind 0.0.0.0:9090 --workers 4   # TCP instead of UDS

# Monitor daemon
ps aux | grep orchestratord | grep -v grep       # check process
orchestrator debug                                # verify CLI-to-daemon connectivity

# Stop daemon
kill <pid>                                        # graceful SIGTERM
```

## Core Workflow

1. Start the daemon: `orchestratord --foreground --workers 2`
2. `orchestrator init` — create SQLite schema
3. `orchestrator apply -f manifest.yaml` — load resources (daemon hot-reloads config via RwLock, no restart needed)
4. `orchestrator task create --name X --goal Y --workflow Z` — create and run (auto-enqueues to worker)
5. `orchestrator task info <id>` / `task trace <id>` / `task logs <id>` — inspect results

### Lightweight Execution (`orchestrator run`)

```bash
# Synchronous: run specific steps, follow logs, exit with status code
orchestrator run --workflow sdlc --step fix --set ticket_paths=docs/ticket/T-0042.md

# Background (equivalent to task create)
orchestrator run --workflow sdlc --step fix --detach

# Direct assembly: execute a StepTemplate without a workflow
orchestrator run --template fix-ticket --agent-capability fix --set ticket_paths=docs/ticket/T-0042.md
```

`task create` also supports `--step` (repeatable) and `--set key=value` (repeatable) for step filtering and pipeline variable injection.

Use `--project <id>` on `apply`, `get`, `describe`, `delete`, `task create/list`, and `store` to scope operations to a project. Use `orchestrator delete project/<id> --force` to clean up a project's task data and config.

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
