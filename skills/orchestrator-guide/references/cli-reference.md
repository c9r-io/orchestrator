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
| workspace | ws |
| manifest | m |
| edit | e |
| config | cfg |
| check | ck |
| daemon | d |

## Daemon Lifecycle

```bash
# Start daemon (background by default)
orchestrator daemon start
orchestrator daemon start --foreground        # with restart loop
orchestrator daemon start --bind 0.0.0.0:9090 # TCP instead of UDS
orchestrator daemon start --workers 4         # worker pool size

# Manage
orchestrator daemon status                    # PID, version, uptime
orchestrator daemon stop                      # graceful SIGTERM
orchestrator daemon restart                   # stop + start
```

Connection: CLI connects via UDS (`data/orchestrator.sock`) by default, or `$ORCHESTRATOR_SOCKET` env.

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
orchestrator get workspaces -l env=dev
orchestrator describe workspace default
orchestrator delete agent old-agent
orchestrator manifest export
orchestrator edit workspace default
orchestrator check
```

## Task Lifecycle

```bash
# Create
orchestrator task create \
  --name "task-name" --goal "description" \
  --workflow self-bootstrap --project my-project \
  --target-file docs/qa/01.md   # repeatable
orchestrator task create --name X --goal Y --no-start
orchestrator task create --name X --goal Y --detach

# Control
orchestrator task start <id>
orchestrator task start <id> --detach
orchestrator task pause <id>
orchestrator task resume <id>
orchestrator task retry <id> --item <item_id> --force

# Inspect
orchestrator task list -o json
orchestrator task info <id> -o yaml
orchestrator task logs <id>
orchestrator task watch <id>
orchestrator task trace <id>

# Other
orchestrator task delete <id>
orchestrator task edit --help
orchestrator task worker start
orchestrator task session list
orchestrator exec -it <task_id> <step_id>
```

## Persistent Store

```bash
orchestrator store put <store> <key> <value>
orchestrator store get <store> <key>
orchestrator store list <store>
orchestrator store delete <store> <key>
orchestrator store prune <store>
```

## QA & Database

```bash
# Project-scoped reset (safe, isolated)
orchestrator project reset <project> --force
orchestrator qa doctor

# Database reset (DESTRUCTIVE)
orchestrator db reset --force
orchestrator db reset --force --include-config
```

## Other Commands

```bash
orchestrator debug
orchestrator verify
orchestrator version
orchestrator config heal-log
orchestrator config backfill-events --force
orchestrator completion bash > ~/.bash_completion.d/orchestrator
```
