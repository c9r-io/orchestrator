# CLI Command Reference

## Table of Contents
- [Global Options](#global-options)
- [Aliases](#aliases)
- [Daemon Lifecycle](#daemon-lifecycle)
- [Init & Apply](#init--apply)
- [Manifest Operations](#manifest-operations)
- [Resource Queries](#resource-queries)
- [Task Lifecycle](#task-lifecycle)
- [Persistent Store](#persistent-store)
- [QA & Database](#qa--database)
- [Other Commands](#other-commands)

## Global Options

| Flag | Description |
|------|-------------|
| `-v, --verbose` | Verbose output |
| `--log-level <LEVEL>` | error/warn/info/debug/trace |
| `--log-format <FORMAT>` | pretty/json |
| `--unsafe` | Bypass --force gates |

## Aliases

| Full | Short |
|------|-------|
| apply | ap |
| get | g |
| describe | desc |
| delete | rm |
| task | t |
| task list | task ls |
| task create | task new |
| task info | task get |
| task logs | task log |
| task delete | task rm |
| project | proj |
| check | ck |
| debug | dbg |
| store list | store ls |

## Daemon Lifecycle

The daemon is a **standalone binary** (`orchestratord`), not a CLI subcommand.

```bash
# Start daemon
orchestratord --foreground --workers 2           # foreground (recommended)
nohup orchestratord --foreground --workers 2 &   # background via nohup
orchestratord --bind 0.0.0.0:9090 --workers 4   # TCP instead of UDS

# Monitor
ps aux | grep orchestratord | grep -v grep       # check process
orchestrator task worker status                   # worker queue state

# Stop
kill <pid>                                        # graceful SIGTERM
```

Connection: CLI connects via UDS (`data/orchestrator.sock`) by default, or `$ORCHESTRATOR_SOCKET` env.

> Config changes from `apply` are hot-reloaded into the daemon via `RwLock<ActiveConfig>` — no restart needed.

## Init & Apply

```bash
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator apply -f manifest.yaml --dry-run
orchestrator apply -f manifest.yaml --project my-project
cat manifest.yaml | orchestrator apply -f -
```

## Manifest Operations

```bash
orchestrator manifest validate -f manifest.yaml
cat manifest.yaml | orchestrator manifest validate -f -
```

## Resource Queries

```bash
orchestrator get workspaces
orchestrator get agents -o json
orchestrator get workflows -o yaml
orchestrator get agents --project my-project   # project-scoped
orchestrator describe workspace/default
orchestrator describe agent/coder --project my-project
orchestrator delete agent/old-agent --force
orchestrator delete agent/old --force --project my-project
orchestrator check
```

> **Note**: `orchestrator get` requires valid global defaults config.
> In project-only deployments (no global workspaces), `get` will fail.
> Use sqlite queries to verify project-scoped resources:
> ```bash
> sqlite3 data/agent_orchestrator.db \
>   "SELECT json_extract(config_json, '$.projects.\"<project>\".workspaces') \
>    FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1;"
> ```

## Task Lifecycle

```bash
# Create (defaults to --detach: auto-enqueues to daemon worker, returns immediately)
orchestrator task create \
  --name "task-name" --goal "description" \
  --workflow self-bootstrap --project my-project \
  --target-file docs/qa/01.md   # repeatable; -t shorthand

# Create with blocking wait (foreground execution)
orchestrator task create --name X --goal Y --attach

# Control
orchestrator task pause <id>
orchestrator task resume <id>
orchestrator task retry <item_id> --force

# Inspect
orchestrator task list -o json
orchestrator task list --project my-project    # filter by project
orchestrator task info <id> -o yaml
orchestrator task logs <id>
orchestrator task watch <id>              # real-time auto-refreshing panel
orchestrator task trace <id>              # execution timeline with anomaly detection

# Worker management
orchestrator task worker status           # queue state: pending tasks, stop signal
orchestrator task worker start            # start standalone worker loop (non-daemon mode)
orchestrator task worker stop             # signal worker to stop

# Delete
orchestrator task delete <id> --force
```

> **Note**: In C/S mode, `task create` defaults to `--detach` (enqueue to daemon worker).
> Tasks start executing immediately when a worker picks them up.
> Use `--attach` for blocking inline execution.

## Persistent Store

```bash
orchestrator store put <store> <key> <value>
orchestrator store get <store> <key>
orchestrator store list <store>
orchestrator store delete <store> <key>
orchestrator store prune <store>

# Project-scoped store
orchestrator store get <store> <key> --project my-project
orchestrator store put <store> <key> <value> --project my-project
```

## Project & Database

```bash
# Project-scoped reset (safe, isolated)
orchestrator project reset <project> --force
orchestrator project reset <project> --force --include-config

# Database reset (DESTRUCTIVE)
orchestrator db reset --force
orchestrator db reset --force --include-config
```

## System Commands

```bash
orchestrator debug
orchestrator debug --component config
orchestrator version
orchestrator check
orchestrator check -o json
orchestrator manifest validate -f manifest.yaml
```
