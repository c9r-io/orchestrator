# CLI Command Reference

> **Tip:** Run `orchestrator guide` to get this reference dynamically from the CLI itself.
> Use `orchestrator guide <command>` to filter by command name or `--category <cat>` to filter by category.

## Table of Contents
- [Global Options](#global-options)
- [Aliases](#aliases)
- [Daemon Lifecycle](#daemon-lifecycle)
- [Init & Apply](#init--apply)
- [Manifest Operations](#manifest-operations)
- [Resource Queries](#resource-queries)
- [Task Lifecycle](#task-lifecycle)
- [Agent Management](#agent-management)
- [Secret Management](#secret-management)
- [Persistent Store](#persistent-store)
- [Trigger Management](#trigger-management)
- [Event Management](#event-management)
- [QA & Database](#qa--database)
- [Other Commands](#other-commands)

## Global Options

| Flag | Description |
|------|-------------|
| `-v, --verbose` | Verbose output |
| `--control-plane-config <PATH>` | Override control-plane client config file |

## Aliases

| Full | Short |
|------|-------|
| apply | ap |
| get | g |
| describe | desc |
| delete | rm |
| task | t |
| task list | ls |
| debug | dbg |
| check | ck |
| agent | ag |
| event | ev |
| trigger | tg |

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

Connection: CLI connects via UDS (`~/.orchestratord/orchestrator.sock`) by default, or `$ORCHESTRATOR_SOCKET` env.

> Config changes from `apply` are hot-reloaded into the daemon via `RwLock<ActiveConfig>` — no restart needed.

## Init & Apply

```bash
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator apply -f manifest.yaml --dry-run
orchestrator apply -f manifest.yaml --prune       # delete resources not in manifest
orchestrator apply -f manifest.yaml --project my-project
cat manifest.yaml | orchestrator apply -f -
```

> **Important**: Production workflows (self-bootstrap, self-evolution) must ALWAYS use `--project` to isolate resources. Apply execution profiles BEFORE workflows that reference them.

Recommended apply order for multi-resource setups:
```bash
orchestrator apply -f execution-profiles.yaml --project my-project
orchestrator apply -f secrets.yaml --project my-project
orchestrator apply -f workflow.yaml --project my-project
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
orchestrator get executionprofiles
orchestrator get workspaces -l env=dev
orchestrator describe workspace default
orchestrator describe executionprofile sandbox_write
orchestrator delete agent old-agent
orchestrator delete agent old-agent --force       # skip confirmation
orchestrator delete agent old-agent --dry-run     # preview without deleting
orchestrator manifest export
orchestrator edit workspace default
orchestrator check
```

> **Note**: `orchestrator get` requires a valid global defaults config.
> In project-only deployments (no global workspaces), `get` will fail.
> Use sqlite queries to verify project-scoped resources:
> ```bash
> sqlite3 ~/.orchestratord/agent_orchestrator.db \
>   "SELECT json_extract(config_json, '$.projects.\"<project>\".workspaces') \
>    FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1;"
> ```

## Task Lifecycle

```bash
# Create (auto-enqueues to daemon worker, returns immediately)
orchestrator task create \
  --name "task-name" --goal "description" \
  --workflow my-workflow --project my-project \
  --target-file docs/qa/01.md   # repeatable; -t shorthand

# Step filtering: only run specific steps from the workflow
orchestrator task create \
  --workflow sdlc --project my-project \
  --step fix --set ticket_paths=docs/ticket/T-0042.md

# Multiple steps (executed in workflow order)
orchestrator task create --workflow sdlc --step plan --step implement

# Create without auto-starting
orchestrator task create --name X --goal Y --no-start

# Control
orchestrator task start <id>              # start a created task
orchestrator task start --latest          # start the most recent task
orchestrator task pause <id>
orchestrator task resume <id>
orchestrator task retry <id> --item <item_id> --force
orchestrator task recover <id>            # recover orphaned running items

# Inspect
orchestrator task list -o json
orchestrator task info <id> -o yaml
orchestrator task logs <id>
orchestrator task logs --tail 100 <id>
orchestrator task watch <id>              # real-time auto-refreshing panel
orchestrator task trace <id>              # execution timeline with anomaly detection

# Other
orchestrator task delete <id>
```

> **Note**: `task create` auto-enqueues to the daemon worker by default.
> Tasks start executing immediately when a worker picks them up.
> Use `--no-start` to create without auto-starting.
> `--step` filters to specific step IDs; `--set key=value` injects pipeline variables.

## Lightweight Execution (`run`)

```bash
# Synchronous: follow logs until task completes, exit with status code
orchestrator run \
  --workflow sdlc --step fix \
  --set ticket_paths=docs/ticket/T-0042.md

# Background (equivalent to task create)
orchestrator run --workflow sdlc --step fix --detach

# Direct assembly: StepTemplate + agent capability, no workflow needed
orchestrator run \
  --template fix-ticket --agent-capability fix \
  --set ticket_paths=docs/ticket/T-0042.md

# With execution profile override
orchestrator run \
  --template fix-ticket --agent-capability fix \
  --profile host-unrestricted
```

| Flag | Description |
|------|-------------|
| `-W, --workflow` | Workflow ID (required unless `--template`) |
| `-S, --step` | Step IDs to execute (repeatable) |
| `--set` | Pipeline variable `key=value` (repeatable) |
| `-p, --project` | Project ID |
| `-w, --workspace` | Workspace ID |
| `-t, --target-file` | Target files (repeatable) |
| `--detach` | Run in background (print task ID and return) |
| `--template` | StepTemplate name (direct assembly mode) |
| `--agent-capability` | Agent capability (direct assembly mode) |
| `--profile` | ExecutionProfile override (direct assembly mode) |

## Agent Management

```bash
orchestrator agent list                   # list agents with state and capabilities
orchestrator agent cordon <agent_name>    # mark unschedulable (no new work)
orchestrator agent uncordon <agent_name>  # mark schedulable again
orchestrator agent drain <agent_name>     # cordon + wait for in-flight work to finish
```

## Secret Management

```bash
# Key lifecycle
orchestrator secret key status            # show active encryption key
orchestrator secret key list              # list all keys with state (active/retired/revoked)
orchestrator secret key rotate            # rotate to new key (requires active key)
orchestrator secret key revoke <key_id>   # revoke a specific key
orchestrator secret key history           # show key audit trail

# If all keys are retired/revoked, SecretStore writes are blocked.
# To recover: delete DB and re-init to create a fresh primary key.
```

## Persistent Store

```bash
orchestrator store put <store> <key> <value>
orchestrator store get <store> <key>
orchestrator store list <store>
orchestrator store delete <store> <key>
orchestrator store prune <store>
```

## Trigger Management

```bash
orchestrator get triggers                # list all triggers
orchestrator get trigger/<name> -o yaml  # get single trigger
orchestrator trigger suspend <name>      # pause trigger (no auto-fire)
orchestrator trigger resume <name>       # unpause trigger
orchestrator trigger fire <name>         # manually fire (create task now)
orchestrator delete trigger/<name>       # remove trigger
```

## Event Management

```bash
orchestrator event cleanup               # remove old events (per retention config)
orchestrator event stats                  # show event table statistics
```

## Database

```bash
orchestrator db status             # show database info
orchestrator db migrations list    # list applied migrations
```

## Other Commands

```bash
orchestrator debug                           # system debug info
orchestrator check                           # preflight check
orchestrator check --workflow my-wf          # check specific workflow
orchestrator check --project my-proj -o json # check with project filter
orchestrator version                         # show version
orchestrator daemon stop                     # stop the daemon
orchestrator daemon status                   # check daemon status
orchestrator daemon maintenance --enable     # block new task creation
orchestrator daemon maintenance --disable    # resume normal operation
```
