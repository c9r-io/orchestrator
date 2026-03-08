# Orchestrator - QA Command Contract

**Module**: orchestrator  
**Scope**: Canonical CLI contract for all QA documents  
**Scenarios**: 4  
**Priority**: Critical

---

## Purpose

This document is the single source of truth for command syntax used by QA docs.
All files under `docs/qa/orchestrator/` must align with this contract.

Runtime state source of truth is SQLite. YAML is used as import/export/edit artifact when needed.

Entry point: `./scripts/run-cli.sh <command>` (auto-builds + calls CLI client) or `./target/release/orchestrator <command>`.

Daemon lifecycle:
- Start: `orchestrator daemon start -f` (foreground with restart loop) or `orchestrator daemon start` (background)
- Stop/status/restart: `orchestrator daemon stop|status|restart`
- Direct: `./target/release/orchestratord [--foreground] [--bind addr] [--workers N]`

---

## Scenario 1: Valid Top-Level Command Surface

### Preconditions

- CLI binary is available.

### Steps

1. Show help:
   ```bash
   ./scripts/run-cli.sh --help
   ```

2. Confirm top-level commands exist:
   - `init`
   - `apply`
   - `get`
   - `describe`
   - `task`
   - `workspace`
   - `agent`
   - `workflow`
   - `manifest`
   - `edit`
   - `db`
   - `qa`
   - `completion`
   - `debug`
   - `exec`
   - `store`

3. Confirm C/S CLI (`orchestrator`) top-level commands:
   - `daemon` (start/stop/status/restart)
   - `apply`
   - `get`
   - `describe`
   - `delete`
   - `task` (list/create/info/start/pause/resume/logs/delete/retry)
   - `store` (get/put/delete/list/prune)
   - `debug`
   - `check`
   - `version`

### Expected Result

- Help output includes all commands above.
- `task worker` subcommands are visible under `task --help` (standalone mode).
- C/S CLI includes `daemon` subcommand family for lifecycle management.
- Deprecated/removed command groups are not documented in QA steps.

---

## Scenario 2: Parameter Contract Check

### Preconditions

- Database initialized and config applied from a YAML file that defines a `default` workspace.
  The `init` command only creates the DB schema; it does **not** load config or create workspaces.
  You must run `apply -f <manifest.yaml>` so that
  the `default` workspace is present in SQLite before running any workspace/task commands.

### Steps

1. Apply manifest environment (if not already done):
   ```bash
   ./scripts/run-cli.sh init
   ./scripts/run-cli.sh apply -f <manifest.yaml>
   ```

2. (Recommended for isolated QA reruns) Reset only the scenario project:
   ```bash
   ./scripts/run-cli.sh qa project reset <qa-project-id> --keep-config --force
   ```

3. Validate workspace info positional argument:
   ```bash
   ./scripts/run-cli.sh workspace info default
   ```

4. Validate output format flags:
   ```bash
   ./scripts/run-cli.sh task list -o json
   ./scripts/run-cli.sh task info {task_id} -o yaml
   ./scripts/run-cli.sh get workspaces -o yaml
   ```

5. Validate task create does not depend on `--format`:
   ```bash
   ./scripts/run-cli.sh task create --project <qa-project-id> --name "contract-check" --goal "check" --no-start
   ```

6. Validate new scheduling flags and worker commands:
   ```bash
   ./scripts/run-cli.sh task create --help | rg -- "--detach"
   ./scripts/run-cli.sh task start --help | rg -- "--detach"
   ./scripts/run-cli.sh task worker --help
   ```
7. Validate task edit and exec command families:
   ```bash
   ./scripts/run-cli.sh task edit --help
   ./scripts/run-cli.sh exec --help
   ```

### Expected Result

- `workspace info` accepts positional workspace id.
- Output format flags work for commands that support `-o`.
- `task create --format ...` is never required in QA docs.
- `--detach` flags and `task worker` command family are part of the CLI contract.
- `task edit` and `exec` are part of the CLI contract.

---

## Scenario 3: kubectl-Style Surface Contract

### Preconditions

- Database initialized.

### Steps

1. Validate list-style get:
   ```bash
   ./scripts/run-cli.sh get workspaces
   ./scripts/run-cli.sh get agents
   ./scripts/run-cli.sh get workflows
   ```

2. Validate label selector syntax:
   ```bash
   ./scripts/run-cli.sh get workspaces -l env=dev
   ```

3. Validate stdin apply contract:
   ```bash
   cat fixtures/manifests/bundles/output-formats.yaml | ./scripts/run-cli.sh apply -f -
   ```

4. Validate create command surfaces:
   ```bash
   ./scripts/run-cli.sh workspace create --help
   ./scripts/run-cli.sh agent create --help
   ./scripts/run-cli.sh workflow create --help
   ```

### Expected Result

- `get <resource-type>` syntax works.
- `-l key=value[,k=v]` is accepted on list get commands.
- `apply -f -` reads from stdin.
- `workspace/agent/workflow create` subcommands are exposed.

---

## Scenario 4: Banned Patterns Guard

### Preconditions

- QA lint script is available.

### Steps

1. Run QA doc lint:
   ```bash
   ./scripts/qa-doc-lint.sh
   ```

2. Confirm no banned patterns in docs:
   - `cd orchestrator`
   - `--workspace-id`
   - `orchestrator agent health`
   - `orchestrator/config/default.yaml`
   - `config bootstrap --from`

### Expected Result

- Lint exits with code `0`.
- QA docs remain aligned with current repository structure and CLI surface.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Valid Top-Level Command Surface | ✅ | 2026-02-23 | opencode | |
| 2 | Parameter Contract Check | ✅ | 2026-02-23 | opencode | |
| 3 | kubectl-Style Surface Contract | ✅ | 2026-02-23 | opencode | |
| 4 | Banned Patterns Guard | ✅ | 2026-02-23 | opencode | |
