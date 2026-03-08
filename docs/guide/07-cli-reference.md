# 07 - CLI Reference

Quick-reference for all Agent Orchestrator CLI commands.

## Entry Points

| Mode | Command | Description |
|------|---------|-------------|
| Standalone | `orchestrator <command>` | Legacy monolithic CLI |
| C/S Daemon | `./target/release/orchestratord [flags]` | gRPC server + embedded workers |
| C/S Client | `./target/release/orchestrator <command>` | Lightweight gRPC client |

**Standalone mode** runs everything in-process. **C/S mode** separates the daemon (state, DB, workers) from the CLI client (thin gRPC calls over Unix socket).

## Global Options

| Flag | Description |
|------|-------------|
| `-v, --verbose` | Enable verbose output |
| `--log-level <LEVEL>` | Override log level: `error`, `warn`, `info`, `debug`, `trace` |
| `--log-format <FORMAT>` | Console log format: `pretty`, `json` |
| `--unsafe` | Bypass all `--force` gates and override runner policy to Unsafe |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

## Command Aliases

Several commands have short aliases for convenience:

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
| `workspace` | `ws` |
| `manifest` | `m` |
| `edit` | `e` |
| `completion` | `comp` |
| `config` | `cfg` |
| `check` | `ck` |
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

# Label selector
orchestrator get workspaces -l env=dev,team=platform
```

### describe

Detailed view of a single resource.

```bash
orchestrator describe workspace default
orchestrator describe agent coder
orchestrator describe workflow self-bootstrap
```

### delete

Delete a resource by kind/name.

```bash
orchestrator delete workspace my-ws
orchestrator delete agent old-agent
```

## Workspace

```bash
orchestrator workspace info default          # positional arg
orchestrator workspace create --help
```

## Agent

```bash
orchestrator agent create --help
```

## Workflow

```bash
orchestrator workflow create --help
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
| `--detach` | Enqueue for background worker |

### task list / info

```bash
orchestrator task list
orchestrator task list -o json

orchestrator task info <task_id>
orchestrator task info <task_id> -o yaml
```

### task start / pause / resume

```bash
orchestrator task start <task_id>
orchestrator task start <task_id> --detach

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
orchestrator task retry <task_id> --item <item_id> --force
```

### task edit

Insert a step into a running task's execution plan.

```bash
orchestrator task edit --help
```

### task delete

```bash
orchestrator task delete <task_id>
```

### task worker (standalone mode)

Background worker for processing detached tasks (standalone mode only).

```bash
orchestrator task worker start
orchestrator task worker start --poll-ms 500 --workers 3
orchestrator task worker stop
orchestrator task worker status
```

> **C/S mode**: Workers are embedded in the daemon. Use `orchestratord --workers N` instead. No separate worker command is needed.

### task session

Session management for attached task execution.

```bash
orchestrator task session list
orchestrator task session info <session_id>
orchestrator task session close <session_id>
```

## Exec

Execute a command in a task step context.

```bash
orchestrator exec --help

# Interactive mode
orchestrator exec -it <task_id> <step_id>
```

## Manifest & Edit

```bash
# Export all config as YAML
orchestrator manifest export

# Edit a resource interactively (opens $EDITOR)
orchestrator edit workspace default
orchestrator edit workflow self-bootstrap
```

## Database

```bash
# Reset database (destructive — requires --force)
orchestrator db reset --force
orchestrator db reset --force --include-config
```

**Warning**: `db reset` is destructive. Use `qa project reset` for isolated cleanup.

## QA Project Management

```bash
# Reset a project (isolated — does not affect other projects)
orchestrator qa project reset <project> --keep-config --force

# Create a fresh project scaffold
orchestrator qa project create <project> --force

# QA doctor — validate concurrency guardrails
orchestrator qa doctor
```

## Persistent Store

```bash
orchestrator store get <store_name> <key>
orchestrator store put <store_name> <key> <value>
orchestrator store delete <store_name> <key>
orchestrator store list <store_name>
orchestrator store prune <store_name>
```

## Config Lifecycle

```bash
# Show self-heal audit log
orchestrator config heal-log

# Backfill missing step_scope in legacy events
orchestrator config backfill-events --force
```

## Debug & Verify

```bash
orchestrator debug           # inspect internal state
orchestrator verify          # run verification checks
orchestrator version         # build version + git hash
```

## Shell Completion

```bash
# Generate completions (bash/zsh/fish)
orchestrator completion bash > ~/.bash_completion.d/orchestrator
orchestrator completion zsh > ~/.zfunc/_orchestrator
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

Files created:
- PID: `data/daemon.pid`
- Socket: `data/orchestrator.sock`

### daemon management (via CLI client)

```bash
./target/release/orchestrator daemon start              # start daemon in background
./target/release/orchestrator daemon start --foreground  # foreground mode
./target/release/orchestrator daemon status              # check if running
./target/release/orchestrator daemon stop                # graceful shutdown
./target/release/orchestrator daemon restart             # stop + start
```

### C/S CLI command surface

All commands below connect to the daemon via Unix socket:

```bash
# Resource management
./target/release/orchestrator apply -f manifest.yaml
./target/release/orchestrator apply -f - < manifest.yaml
./target/release/orchestrator apply -f manifest.yaml --dry-run
./target/release/orchestrator get workspaces -o json
./target/release/orchestrator describe workspace/default -o yaml
./target/release/orchestrator delete workspace/old --force

# Task lifecycle
./target/release/orchestrator task create --name "test" --goal "goal" --detach
./target/release/orchestrator task list -o json
./target/release/orchestrator task info <task_id>
./target/release/orchestrator task start <task_id> --detach
./target/release/orchestrator task pause <task_id>
./target/release/orchestrator task resume <task_id>
./target/release/orchestrator task logs <task_id> --tail 50
./target/release/orchestrator task logs <task_id> --follow
./target/release/orchestrator task delete <task_id> --force
./target/release/orchestrator task retry <item_id> --force

# Store
./target/release/orchestrator store put <store> <key> <value>
./target/release/orchestrator store get <store> <key>
./target/release/orchestrator store list <store> -o json
./target/release/orchestrator store delete <store> <key>
./target/release/orchestrator store prune <store>

# System
./target/release/orchestrator version
./target/release/orchestrator debug --component config
./target/release/orchestrator check -o json
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
