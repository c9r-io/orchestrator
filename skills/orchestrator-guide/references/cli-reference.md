# CLI Command Reference

## Table of Contents
- [Global Options](#global-options)
- [Aliases](#aliases)
- [Init & Apply](#init--apply)
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

## Init & Apply

```bash
./scripts/orchestrator.sh init
./scripts/orchestrator.sh apply -f manifest.yaml
./scripts/orchestrator.sh apply -f manifest.yaml --dry-run
./scripts/orchestrator.sh apply -f manifest.yaml --project my-project
cat manifest.yaml | ./scripts/orchestrator.sh apply -f -
```

## Resource Queries

```bash
./scripts/orchestrator.sh get workspaces
./scripts/orchestrator.sh get agents -o json
./scripts/orchestrator.sh get workflows -o yaml
./scripts/orchestrator.sh get workspaces -l env=dev
./scripts/orchestrator.sh describe workspace default
./scripts/orchestrator.sh delete agent old-agent
./scripts/orchestrator.sh manifest export
./scripts/orchestrator.sh edit workspace default
./scripts/orchestrator.sh check
```

## Task Lifecycle

```bash
# Create
./scripts/orchestrator.sh task create \
  --name "task-name" --goal "description" \
  --workflow self-bootstrap --project my-project \
  --target-file docs/qa/01.md   # repeatable
./scripts/orchestrator.sh task create --name X --goal Y --no-start
./scripts/orchestrator.sh task create --name X --goal Y --detach

# Control
./scripts/orchestrator.sh task start <id>
./scripts/orchestrator.sh task start <id> --detach
./scripts/orchestrator.sh task pause <id>
./scripts/orchestrator.sh task resume <id>
./scripts/orchestrator.sh task retry <id> --item <item_id> --force

# Inspect
./scripts/orchestrator.sh task list -o json
./scripts/orchestrator.sh task info <id> -o yaml
./scripts/orchestrator.sh task logs <id>
./scripts/orchestrator.sh task watch <id>
./scripts/orchestrator.sh task trace <id>

# Other
./scripts/orchestrator.sh task delete <id>
./scripts/orchestrator.sh task edit --help
./scripts/orchestrator.sh task worker start
./scripts/orchestrator.sh task session list
./scripts/orchestrator.sh exec -it <task_id> <step_id>
```

## Persistent Store

```bash
./scripts/orchestrator.sh store put <store> <key> <value>
./scripts/orchestrator.sh store get <store> <key>
./scripts/orchestrator.sh store list <store>
./scripts/orchestrator.sh store delete <store> <key>
./scripts/orchestrator.sh store prune <store>
```

## QA & Database

```bash
# Project-scoped reset (safe, isolated)
./scripts/orchestrator.sh qa project reset <project> --keep-config --force
./scripts/orchestrator.sh qa project create <project> --force
./scripts/orchestrator.sh qa doctor

# Database reset (DESTRUCTIVE)
./scripts/orchestrator.sh db reset --force
./scripts/orchestrator.sh db reset --force --include-config
```

## Other Commands

```bash
./scripts/orchestrator.sh debug
./scripts/orchestrator.sh verify
./scripts/orchestrator.sh version
./scripts/orchestrator.sh config heal-log
./scripts/orchestrator.sh config backfill-events --force
./scripts/orchestrator.sh completion bash > ~/.bash_completion.d/orchestrator
```
