---
self_referential_safe: false
---
# Orchestrator - CLI Force Gate Audit

**Module**: orchestrator
**Scope**: Verify all destructive CLI commands require `--force` confirmation before executing
**Scenarios**: 5
**Priority**: High

---

## Background

High-risk CLI operations that perform irreversible state changes must require `--force` to prevent accidental execution. This document validates that all destructive commands are gated and that the gate behaves correctly (warning message, non-zero exit, no side effects without `--force`).

### Force-Protected Commands Inventory

| Command | Risk | Gate Added |
|---------|------|------------|
| `delete <resource>` | Deletes resource from DB | ✓ existing |
| `task delete <id>` | Deletes task + stops runtime | ✓ existing |
| `db reset` | Drops and recreates database | ✓ existing |
| `qa project reset <project>` | Resets project state | ✓ existing |
| `qa project create <project>` | Overwrites existing project | ✓ existing |
| `init` | Overwrites existing config | ✓ existing |
| `task session close <id>` | Kills backing process | ✓ existing |
| `config backfill-events` | Bulk UPDATE all event rows | ✓ new |
| `task retry <item>` | Resets item execution state | ✓ new |

The `--unsafe` global CLI flag bypasses all force gates at once — see `docs/qa/orchestrator/45-cli-unsafe-mode.md`.

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: Config Backfill-Events Rejected Without --force

### Preconditions
- Orchestrator binary built

### Goal
Verify `config backfill-events` refuses to run without `--force`.

### Steps
1. Run without `--force`:
   ```bash
   ./scripts/orchestrator.sh config backfill-events 2>&1; echo "exit=$?"
   ```

2. Verify no database changes occurred:
   ```bash
   # If events exist, their payload should be unchanged
   sqlite3 data/agent_orchestrator.db "SELECT count(*) FROM events LIMIT 1;"
   ```

### Expected
- stderr contains: `Use --force to confirm`
- Exit code: 1
- No database rows modified

---

## Scenario 2: Config Backfill-Events Executes With --force

### Preconditions
- Orchestrator binary built

### Goal
Verify `config backfill-events --force` performs the backfill.

### Steps
1. Run with `--force`:
   ```bash
   ./scripts/orchestrator.sh config backfill-events --force
   ```

### Expected
- Output: `scanned N events, updated M, skipped K (already had step_scope)`
- Exit code: 0

---

## Scenario 3: Task Retry Rejected Without --force

### Preconditions
- A task with at least one failed/unresolved item exists

### Goal
Verify `task retry` refuses to run without `--force`.

### Steps
1. Find a failed item:
   ```bash
   ITEM_ID=$(sqlite3 data/agent_orchestrator.db \
     "SELECT id FROM task_items WHERE status IN ('qa_failed','unresolved') LIMIT 1;")
   ```

2. Attempt retry without `--force`:
   ```bash
   ./scripts/orchestrator.sh task retry "$ITEM_ID" 2>&1; echo "exit=$?"
   ```

3. Verify item state unchanged:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM task_items WHERE id='${ITEM_ID}';"
   ```

### Expected
- stderr contains: `Use --force to confirm`
- Exit code: 1
- Item status unchanged (still `qa_failed` or `unresolved`)

---

## Scenario 4: Task Retry Executes With --force

### Preconditions
- A task with at least one failed/unresolved item exists

### Goal
Verify `task retry --force` resets item and re-executes.

### Steps
1. Find a failed item:
   ```bash
   ITEM_ID=$(sqlite3 data/agent_orchestrator.db \
     "SELECT id FROM task_items WHERE status IN ('qa_failed','unresolved') LIMIT 1;")
   ```

2. Retry with `--force`:
   ```bash
   ./scripts/orchestrator.sh task retry "$ITEM_ID" --force || true
   ```

3. Check item state:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status, updated_at FROM task_items WHERE id='${ITEM_ID}';"
   ```

### Expected
- Exit code: 0
- Item `updated_at` changed
- Item enters retry execution flow

---

## Scenario 5: Existing Force Gates Regression Check

### Preconditions
- Orchestrator binary built

### Goal
Verify that pre-existing `--force` gates still function correctly.

### Steps
1. `task delete` without `--force`:
   ```bash
   ./scripts/orchestrator.sh task delete nonexistent-id 2>&1; echo "exit=$?"
   ```

2. `db reset` without `--force`:
   ```bash
   ./scripts/orchestrator.sh db reset 2>&1; echo "exit=$?"
   ```

3. Verify `--help` documents `--force` for each command:
   ```bash
   ./scripts/orchestrator.sh task delete --help 2>&1 | grep -c '\-\-force'
   ./scripts/orchestrator.sh task retry --help 2>&1 | grep -c '\-\-force'
   ./scripts/orchestrator.sh db reset --help 2>&1 | grep -c '\-\-force'
   ./scripts/orchestrator.sh config backfill-events --help 2>&1 | grep -c '\-\-force'
   ```

### Expected
- `task delete` without `--force`: prints confirmation prompt, exit code 0 (no deletion)
- `db reset` without `--force`: prints confirmation prompt, exit code 1
- All four `--help` outputs contain `--force` (grep count >= 1 each)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Config Backfill-Events Rejected Without --force | ☐ | | | |
| 2 | Config Backfill-Events Executes With --force | ☐ | | | |
| 3 | Task Retry Rejected Without --force | ☐ | | | |
| 4 | Task Retry Executes With --force | ☐ | | | |
| 5 | Existing Force Gates Regression Check | ☐ | | | |
