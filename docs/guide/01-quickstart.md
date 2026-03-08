# 01 - Quick Start

Run your first workflow in 5 minutes.

## Prerequisites

- Rust toolchain (for building from source)
- SQLite3
- Bash shell

## Step 1: Build

```bash
cargo build --workspace --release
```

This produces three binaries:

| Binary | Path | Purpose |
|--------|------|---------|
| `agent-orchestrator` | `core/target/release/agent-orchestrator` | Standalone CLI (legacy) |
| `orchestratord` | `target/release/orchestratord` | Daemon (gRPC server + embedded workers) |
| `orchestrator` | `target/release/orchestrator` | CLI client (connects to daemon via gRPC) |

The wrapper script `./scripts/orchestrator.sh` runs the standalone binary. For C/S mode, use `orchestratord` + `orchestrator` directly.

## Step 2: Initialize the Database

```bash
./scripts/orchestrator.sh init
```

This creates the SQLite schema at `data/agent_orchestrator.db`. It does **not** load any configuration — that comes next.

## Step 3: Write a Manifest

Create a YAML file that defines a Workspace, an Agent, and a Workflow. Here is a minimal example:

```yaml
# my-first-workflow.yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: echo_agent
spec:
  capabilities:
    - qa
  command: >-
    echo '{"confidence":0.95,"quality_score":0.9,
    "artifacts":[{"kind":"analysis","findings":[
    {"title":"all-good","description":"no issues found","severity":"info"}
    ]}]}'
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: simple_qa
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
  loop:
    mode: once
```

## Step 4: Apply the Manifest

```bash
./scripts/orchestrator.sh apply -f my-first-workflow.yaml
```

This loads all resources (Workspace, Agent, Workflow) into the database. You can verify:

```bash
./scripts/orchestrator.sh get workspaces
./scripts/orchestrator.sh get agents
./scripts/orchestrator.sh get workflows
```

## Step 5: Create and Run a Task

```bash
./scripts/orchestrator.sh task create \
  --name "my-first-task" \
  --goal "Verify QA docs pass" \
  --workflow simple_qa
```

This creates a task, binds it to the `default` workspace and `simple_qa` workflow, and starts execution immediately.

To create without starting:

```bash
./scripts/orchestrator.sh task create \
  --name "my-first-task" \
  --goal "Verify QA docs pass" \
  --workflow simple_qa \
  --no-start
```

Then start it manually:

```bash
./scripts/orchestrator.sh task start <task_id>
```

## Step 6: Inspect Results

```bash
# List all tasks
./scripts/orchestrator.sh task list

# Task details (table, JSON, or YAML)
./scripts/orchestrator.sh task info <task_id>
./scripts/orchestrator.sh task info <task_id> -o json

# View execution logs
./scripts/orchestrator.sh task logs <task_id>
```

## What Just Happened?

1. `init` created the SQLite schema
2. `apply` loaded three resources into the database
3. `task create` bound a workspace + workflow, discovered QA target files as task items, and ran the `qa` step on each item
4. The `echo_agent` was selected (it has the `qa` capability) and its command was executed for each item
5. Results (exit code, stdout, stderr) were captured in the database

## Alternative: Client/Server Mode

Instead of running tasks inline, you can use the daemon for background execution:

```bash
# Start daemon with 2 background workers
./target/release/orchestratord --foreground --workers 2

# In another terminal — use the gRPC client
./target/release/orchestrator apply -f my-first-workflow.yaml
./target/release/orchestrator task create --name "my-task" --goal "QA" --workflow simple_qa --detach
./target/release/orchestrator task list
./target/release/orchestrator task logs <task_id>
```

The daemon holds all state, automatically picks up enqueued tasks, and the CLI client communicates over a Unix socket. See [07 - CLI Reference](07-cli-reference.md) for daemon commands.

## Next Steps

- [02 - Resource Model](02-resource-model.md) — understand the four resource kinds
- [03 - Workflow Configuration](03-workflow-configuration.md) — design multi-step workflows
