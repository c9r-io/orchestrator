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
./scripts/run-cli.sh init
./scripts/run-cli.sh apply -f manifest.yaml
./scripts/run-cli.sh apply -f manifest.yaml --dry-run
./scripts/run-cli.sh apply -f manifest.yaml --project my-project
cat manifest.yaml | ./scripts/run-cli.sh apply -f -
```

## Resource Queries

```bash
./scripts/run-cli.sh get workspaces
./scripts/run-cli.sh get agents -o json
./scripts/run-cli.sh get workflows -o yaml
./scripts/run-cli.sh get workspaces -l env=dev
./scripts/run-cli.sh describe workspace default
./scripts/run-cli.sh delete agent old-agent
./scripts/run-cli.sh manifest export
./scripts/run-cli.sh edit workspace default
./scripts/run-cli.sh check
```

## Task Lifecycle

```bash
# Create
./scripts/run-cli.sh task create \
  --name "task-name" --goal "description" \
  --workflow self-bootstrap --project my-project \
  --target-file docs/qa/01.md   # repeatable
./scripts/run-cli.sh task create --name X --goal Y --no-start
./scripts/run-cli.sh task create --name X --goal Y --detach

# Control
./scripts/run-cli.sh task start <id>
./scripts/run-cli.sh task start <id> --detach
./scripts/run-cli.sh task pause <id>
./scripts/run-cli.sh task resume <id>
./scripts/run-cli.sh task retry <id> --item <item_id> --force

# Inspect
./scripts/run-cli.sh task list -o json
./scripts/run-cli.sh task info <id> -o yaml
./scripts/run-cli.sh task logs <id>
./scripts/run-cli.sh task watch <id>
./scripts/run-cli.sh task trace <id>

# Other
./scripts/run-cli.sh task delete <id>
./scripts/run-cli.sh task edit --help
./scripts/run-cli.sh task worker start
./scripts/run-cli.sh task session list
./scripts/run-cli.sh exec -it <task_id> <step_id>
```

## Persistent Store

```bash
./scripts/run-cli.sh store put <store> <key> <value>
./scripts/run-cli.sh store get <store> <key>
./scripts/run-cli.sh store list <store>
./scripts/run-cli.sh store delete <store> <key>
./scripts/run-cli.sh store prune <store>
```

## QA & Database

```bash
# Project-scoped reset (safe, isolated)
./scripts/run-cli.sh qa project reset <project> --keep-config --force
./scripts/run-cli.sh qa project create <project> --force
./scripts/run-cli.sh qa doctor

# Database reset (DESTRUCTIVE)
./scripts/run-cli.sh db reset --force
./scripts/run-cli.sh db reset --force --include-config
```

## Other Commands

```bash
./scripts/run-cli.sh debug
./scripts/run-cli.sh verify
./scripts/run-cli.sh version
./scripts/run-cli.sh config heal-log
./scripts/run-cli.sh config backfill-events --force
./scripts/run-cli.sh completion bash > ~/.bash_completion.d/orchestrator
```
