# 07 - CLI Reference

Quick-reference for all Agent Orchestrator CLI commands.

## Entry Points

| Binary | Description |
|--------|-------------|
| `orchestratord` | gRPC daemon — server + embedded workers |
| `orchestrator` | CLI client — lightweight gRPC calls over Unix socket |

The daemon holds all state (engine, DB, task queue). The CLI is a thin RPC client.

## Global Options

| Flag | Description |
|------|-------------|
| `-v, --verbose` | Enable verbose output |
| `-h, --help` | Print help |
| `-V, --version` | Print version |
| `--control-plane-config <path>` | Override control-plane client config (env: `ORCHESTRATOR_CONTROL_PLANE_CONFIG`) |

## Command Aliases

| Command | Alias |
|---------|-------|
| `apply` | `ap` |
| `get` | `g` |
| `describe` | `desc` |
| `delete` | `rm` |
| `task` | `t` |
| `task list` | `task ls` |
| `task create` | `task new` |
| `task info` | `task get` |
| `task logs` | `task log` |
| `task delete` | `task rm` |
| `project` | `proj` |
| `check` | `ck` |
| `debug` | `dbg` |
| `store list` | `store ls` |

## Initialization & Configuration

### init

Create runtime directories and SQLite schema.

```bash
orchestrator init
```

### apply

Load resources from a YAML manifest into the database.

```bash
# From file
orchestrator apply -f manifest.yaml

# From stdin
cat manifest.yaml | orchestrator apply -f -

# Dry-run (validate only)
orchestrator apply -f manifest.yaml --dry-run

# Project-scoped apply
orchestrator apply -f manifest.yaml --project my-project
```

### check

Preflight validation: cross-reference agents, workflows, and templates.

```bash
orchestrator check
```

## Resource Queries

### get

List resources (kubectl-style).

```bash
orchestrator get workspaces
orchestrator get agents
orchestrator get workflows

# Output format
orchestrator get agents -o json
orchestrator get agents -o yaml

# Project-scoped query
orchestrator get agents --project my-project
```

### describe

Detailed view of a single resource.

```bash
orchestrator describe workspace/default
orchestrator describe agent/coder

# Project-scoped
orchestrator describe agent/my-agent --project my-project
```

### delete

Delete a resource by kind/name.

```bash
orchestrator delete workspace/my-ws --force
orchestrator delete agent/old-agent --force

# Project-scoped
orchestrator delete agent/old --force --project my-project
```

## Task Lifecycle

### task create

```bash
orchestrator task create \
  --name "my-task" \
  --goal "Implement feature X" \
  --workflow self-bootstrap \
  --project my-project \
  --workspace default \
  --target-file docs/qa/01-test.md    # can specify multiple times
```

| Flag | Description |
|------|-------------|
| `-n, --name` | Task name |
| `-g, --goal` | Task goal/description |
| `-p, --project` | Project ID |
| `-w, --workspace` | Workspace ID |
| `-W, --workflow` | Workflow ID |
| `-t, --target-file` | Target files (repeatable) |
| `--no-start` | Create without auto-starting |

### task list / info

```bash
orchestrator task list
orchestrator task list -o json
orchestrator task list --project my-project    # filter by project

orchestrator task info <task_id>
orchestrator task info <task_id> -o yaml
```

### task start / pause / resume

```bash
orchestrator task start <task_id>

orchestrator task pause <task_id>
orchestrator task resume <task_id>
```

### task logs / watch / trace

```bash
# View execution logs
orchestrator task logs <task_id>

# Live watch (auto-refreshing status panel)
orchestrator task watch <task_id>

# Execution trace with anomaly detection
orchestrator task trace <task_id>
```

### task retry

Retry a failed task item.

```bash
orchestrator task retry <task_item_id> [--force]
```

### task delete

```bash
orchestrator task delete <task_id> --force
```

## Manifest

```bash
# Validate a manifest file
orchestrator manifest validate -f manifest.yaml

# Export all resources as manifest documents
orchestrator manifest export [-o yaml|json]
```

## Secret Key Management

```bash
orchestrator secret key status [-o json]
orchestrator secret key list [-o json]
orchestrator secret key rotate [--resume]
orchestrator secret key revoke <key_id> [--force]
orchestrator secret key history [-n <limit>] [--key-id <id>] [-o json]
```

## Database Operations

```bash
orchestrator db status [-o json]
orchestrator db migrations list [-o json]
```

## Project Cleanup

Use `orchestrator delete project/<id> --force` for project cleanup.

## Project Management

Project isolation is native — use `--project` on `apply`, `get`, `describe`, `delete`, `task create`, `task list`, and `store` commands.

```bash
# Apply resources to a project scope
orchestrator apply -f manifest.yaml --project my-project

# Explicitly prune resources omitted from the manifest
orchestrator apply -f manifest.yaml --project my-project --prune

# Query project-scoped resources
orchestrator get agents --project my-project

# Delete a project and all its data (tasks, items, runs, events, config)
orchestrator delete project/<project> --force
```

Default `apply` is merge-only: resources omitted from the manifest are preserved.
Use `--prune` only when you want omitted resources of the same applied kinds to be deleted
within the target project.

## Persistent Store

```bash
orchestrator store get <store_name> <key>
orchestrator store put <store_name> <key> <value>
orchestrator store delete <store_name> <key>
orchestrator store list <store_name>
orchestrator store prune <store_name>

# Project-scoped store
orchestrator store get <store_name> <key> --project my-project
orchestrator store put <store_name> <key> <value> --project my-project
```

## Debug & System

```bash
orchestrator debug                   # inspect internal state
orchestrator debug --component config  # show active config
orchestrator version                 # build version + git hash
orchestrator check                   # preflight validation
orchestrator check -o json           # structured check output
```

## Output Formats

Most `get` and `info` commands support `-o` for output format:

```bash
-o json    # JSON output
-o yaml    # YAML output
# (default) # table output
```

## Daemon (C/S Mode)

### orchestratord

The daemon binary that runs the gRPC server and embedded background workers.

```bash
# Start in foreground (recommended for development)
./target/release/orchestratord --foreground

# With multiple workers
./target/release/orchestratord --foreground --workers 3

# TCP bind (for remote access)
./target/release/orchestratord --foreground --bind 0.0.0.0:50051
```

| Flag | Description |
|------|-------------|
| `--foreground`, `-f` | Run in foreground (don't daemonize) |
| `--bind <addr>` | TCP bind address (default: Unix socket) |
| `--workers <N>` | Number of background workers (default: 1) |
| `--insecure-bind <addr>` | Insecure TCP bind for development (feature-gated: `dev-insecure`) |

### control-plane issue-client

Issue client TLS materials for connecting to the daemon's control plane:

```bash
orchestratord control-plane issue-client \
  --bind <addr> --subject <name> [--role <role>]
```

Files created:
- PID: `data/daemon.pid`
- Socket: `data/orchestrator.sock`

### daemon management

```bash
./target/release/orchestratord --foreground --workers 2   # foreground (recommended)
nohup ./target/release/orchestratord --foreground &       # background via nohup
kill $(cat data/daemon.pid)                               # graceful SIGTERM
```

### C/S CLI command surface

All commands connect to the daemon via Unix socket:

```bash
# Resource management (--project for project scope)
orchestrator apply -f manifest.yaml [--project <id>] [--dry-run]
orchestrator get <resource> [-o json|yaml] [--project <id>]
orchestrator describe <kind/name> [--project <id>]
orchestrator delete <kind/name> --force [--project <id>]

# Task lifecycle
orchestrator task create --name X --goal Y [--project <id>] [--workflow Z]
orchestrator task list [-o json] [--project <id>] [--status <s>]
orchestrator task info <id> [-o json]
orchestrator task start <id>
orchestrator task pause <id>
orchestrator task resume <id>
orchestrator task logs <id> [--tail N] [--follow]
orchestrator task watch <id>
orchestrator task trace <id> [--verbose]
orchestrator task retry <item_id> [--force]
orchestrator task delete <id> --force

# Project cleanup
orchestrator delete project/<id> --force

# Store (--project for project scope)
orchestrator store put <store> <key> <value> [--project <id>]
orchestrator store get <store> <key> [--project <id>]
orchestrator store list <store> [-o json] [--project <id>]
orchestrator store delete <store> <key> [--project <id>]
orchestrator store prune <store> [--project <id>]

# Manifest
orchestrator manifest validate -f <file>
orchestrator manifest export [-o yaml|json]

# Secret key management
orchestrator secret key status|list|rotate|revoke|history

# Database
orchestrator db status [-o json]
orchestrator db migrations list [-o json]

# System
orchestrator version
orchestrator debug [--component config]
orchestrator check [-o json] [--workflow <w>]
orchestrator init [<root>]
```

## Resource Metadata

All resources support `metadata.labels` (key-value pairs for categorization and label-selector queries) and `metadata.annotations` (arbitrary key-value metadata). Both are optional.

```yaml
metadata:
  name: my-resource
  labels:
    env: dev
    team: platform
  annotations:
    note: "created for sprint 12"
```

## Structured Agent Output

Agents must produce JSON on stdout conforming to this schema:

```json
{
  "confidence": 0.95,
  "quality_score": 0.9,
  "artifacts": [
    {
      "kind": "analysis",
      "findings": [
        {
          "title": "finding-name",
          "description": "details",
          "severity": "info"
        }
      ]
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `confidence` | `float` | Agent's confidence in the result (0.0–1.0) |
| `quality_score` | `float` | Quality assessment (0.0–1.0) |
| `artifacts` | `array` | Structured output artifacts |
| `artifacts[].kind` | `string` | `analysis`, `code_change`, etc. |
| `artifacts[].findings` | `array` | List of findings with title/description/severity |
| `artifacts[].files` | `array` | List of modified files (for code_change) |

This output is parsed into `AgentOutput` and used for prehook variable injection (`qa_confidence`, `qa_quality_score`) and finalize rule evaluation.
