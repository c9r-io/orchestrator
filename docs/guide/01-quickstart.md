# 01 - Quick Start

Run your first workflow in 5 minutes.

## Prerequisites

- Rust toolchain (for building from source)
- SQLite3
- Bash shell

## Step 1: Build

```bash
cd core && cargo build --release && cd ..
```

The binary is at `./core/target/release/agent-orchestrator`. The wrapper script `./scripts/orchestrator.sh` is the recommended entry point.

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

## Next Steps

- [02 - Resource Model](02-resource-model.md) — understand the four resource kinds
- [03 - Workflow Configuration](03-workflow-configuration.md) — design multi-step workflows
