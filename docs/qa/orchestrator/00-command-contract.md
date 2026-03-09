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

Entry point: `orchestrator <command>` (auto-builds + calls CLI client) or `./target/release/orchestrator <command>`.

Daemon lifecycle:
- Start: `./target/release/orchestratord --foreground --workers 2` (foreground, recommended)
- Background: `nohup ./target/release/orchestratord --foreground --workers 2 &`
- Stop: `kill $(cat data/daemon.pid)` (graceful SIGTERM)

---

## Scenario 1: Valid Top-Level Command Surface

### Preconditions

- CLI binary is available.

### Steps

1. Show help:
   ```bash
   orchestrator --help
   ```

2. Confirm top-level commands exist:
   - `init`
   - `apply`
   - `get`
   - `describe`
   - `delete`
   - `task`
   - `store`
   - `debug`
   - `check`
   - `manifest`
   - `version`

3. Confirm `task` subcommands:
   - `list`
   - `create`
   - `info`
   - `start`
   - `pause`
   - `resume`
   - `logs`
   - `delete`
   - `retry`
   - `watch`
   - `trace`

### Expected Result

- Help output includes all commands above.
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
   orchestrator init
   orchestrator apply -f <manifest.yaml>
   ```

2. (Recommended for isolated QA reruns) Reset only the scenario project:
   ```bash
   orchestrator delete "project/<qa-project-id>" --force
   ```

3. Validate workspace describe argument:
   ```bash
   orchestrator describe workspace default
   ```

4. Validate output format flags:
   ```bash
   orchestrator task list -o json
   orchestrator task info {task_id} -o yaml
   orchestrator get workspaces -o yaml
   ```

5. Validate task create does not depend on `--format`:
   ```bash
   orchestrator task create --project <qa-project-id> --name "contract-check" --goal "check" --no-start
   ```

6. Validate new scheduling flags:
   ```bash
   orchestrator task create --help | rg -- "--detach"
   orchestrator task start --help | rg -- "--detach"
   ```

### Expected Result

- `describe workspace` accepts positional workspace name.
- Output format flags work for commands that support `-o`.
- `task create --format ...` is never required in QA docs.
- `--detach` flags are part of the CLI contract.

---

## Scenario 3: kubectl-Style Surface Contract

### Preconditions

- Database initialized.

### Steps

1. Validate list-style get:
   ```bash
   orchestrator get workspaces
   orchestrator get agents
   orchestrator get workflows
   ```

2. Validate label selector syntax:
   ```bash
   orchestrator get workspaces -l env=dev
   ```

3. Validate stdin apply contract:
   ```bash
   cat fixtures/manifests/bundles/output-formats.yaml | orchestrator apply -f -
   ```

### Expected Result

- `get <resource-type>` syntax works.
- `-l key=value[,k=v]` is accepted on list get commands.
- `apply -f -` reads from stdin.

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
   - `orchestrator workspace create`
   - `orchestrator agent create`
   - `orchestrator workflow create`
   - `orchestrator workspace info`

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
