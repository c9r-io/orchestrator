# Orchestrator - QA Command Contract

**Module**: orchestrator  
**Scope**: Canonical CLI contract for all QA documents  
**Scenarios**: 3  
**Priority**: Critical

---

## Purpose

This document is the single source of truth for command syntax used by QA docs.
All files under `docs/qa/orchestrator/` must align with this contract.

Runtime state source of truth is SQLite. YAML is used as import/export/edit artifact when needed.

Entry point: `./scripts/orchestrator.sh <command>` (recommended) or `./core/target/release/agent-orchestrator <command>`.

---

## Scenario 1: Valid Top-Level Command Surface

### Preconditions

- CLI binary is available.

### Steps

1. Show help:
   ```bash
   ./scripts/orchestrator.sh --help
   ```

2. Confirm top-level commands exist:
   - `init`
   - `apply`
   - `get`
   - `describe`
   - `task`
   - `workspace`
   - `config`
   - `edit`
   - `db`
   - `completion`
   - `debug`

### Expected Result

- Help output includes all commands above.
- Deprecated/removed command groups (for example `daemon`, `agent`) are not documented in QA steps.

---

## Scenario 2: Parameter Contract Check

### Preconditions

- Database initialized and config bootstrapped from a YAML file that defines a `default` workspace.
  The `init` command only creates the DB schema; it does **not** load config or create workspaces.
  You must run `config bootstrap --from <file>` (or pass `--config <file>` at runtime) so that
  the `default` workspace is present in SQLite before running any workspace/task commands.

### Steps

1. Bootstrap environment (if not already done):
   ```bash
   rm -f data/agent_orchestrator.db
   ./scripts/orchestrator.sh init
   ./scripts/orchestrator.sh config bootstrap --from <config.yaml>
   ```

2. Validate workspace info positional argument:
   ```bash
   ./scripts/orchestrator.sh workspace info default
   ```

2. Validate output format flags:
   ```bash
   ./scripts/orchestrator.sh task list -o json
   ./scripts/orchestrator.sh task info {task_id} -o yaml
   ```

3. Validate task create does not depend on `--format`:
   ```bash
   ./scripts/orchestrator.sh task create --name "contract-check" --goal "check" --no-start
   ```

### Expected Result

- `workspace info` accepts positional workspace id.
- Output format flags work for commands that support `-o`.
- `task create --format ...` is never required in QA docs.

---

## Scenario 3: Banned Patterns Guard

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

### Expected Result

- Lint exits with code `0`.
- QA docs remain aligned with current repository structure and CLI surface.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Valid Top-Level Command Surface | ☐ | | | |
| 2 | Parameter Contract Check | ☐ | | | |
| 3 | Banned Patterns Guard | ☐ | | | |
