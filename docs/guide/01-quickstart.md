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

This produces the supported runtime binaries:

| Binary | Path | Purpose |
|--------|------|---------|
| `orchestratord` | `target/release/orchestratord` | Daemon (gRPC server + embedded workers) |
| `orchestrator` | `target/release/orchestrator` | CLI client (connects to daemon via gRPC) |

Use `orchestratord` + `orchestrator` as the only supported runtime model.

## Step 2: Start the Daemon

```bash
./target/release/orchestratord --foreground --workers 2
```

The daemon owns the SQLite database, task queue, and worker pool. Keep it running in one terminal and use the CLI client from another terminal.

## Step 3: Initialize the Database

```bash
./target/release/orchestrator init
```

This creates the SQLite schema at `data/agent_orchestrator.db`. It does **not** load any configuration; that comes next.

## Step 4: Write a Manifest

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

## Step 5: Apply the Manifest

```bash
./target/release/orchestrator apply -f my-first-workflow.yaml
```

This loads all resources (Workspace, Agent, Workflow) into the database. You can verify:

```bash
./target/release/orchestrator get workspaces
./target/release/orchestrator get agents
./target/release/orchestrator get workflows
```

## Step 6: Create and Run a Task

```bash
./target/release/orchestrator task create \
  --name "my-first-task" \
  --goal "Verify QA docs pass" \
  --workflow simple_qa
```

This creates a task, binds it to the `default` workspace and `simple_qa` workflow, and starts execution immediately.

To create without starting:

```bash
./target/release/orchestrator task create \
  --name "my-first-task" \
  --goal "Verify QA docs pass" \
  --workflow simple_qa \
  --no-start
```

Then start it manually:

```bash
./target/release/orchestrator task start <task_id>
```

## Step 7: Inspect Results

```bash
# List all tasks
./target/release/orchestrator task list

# Task details (table, JSON, or YAML)
./target/release/orchestrator task info <task_id>
./target/release/orchestrator task info <task_id> -o json

# View execution logs
./target/release/orchestrator task logs <task_id>
```

## What Just Happened?

1. `orchestratord` started the control plane, SQLite-backed runtime, and embedded workers
2. `init` created the SQLite schema
3. `apply` loaded three resources into the database through the daemon
4. `task create` bound a workspace + workflow, discovered QA target files as task items, and enqueued work for the daemon workers
5. The `echo_agent` was selected (it has the `qa` capability) and its command was executed for each item
6. Results (exit code, stdout, stderr) were captured in the database

## Next Steps

- [02 - Resource Model](02-resource-model.md) — understand the four resource kinds
- [03 - Workflow Configuration](03-workflow-configuration.md) — design multi-step workflows
