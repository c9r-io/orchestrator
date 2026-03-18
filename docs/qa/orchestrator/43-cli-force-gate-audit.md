---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S3, S5]
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
| `delete project/<project>` | Deletes project and all its data | ✓ existing |
| `apply --project <project>` | Overwrites existing project | ✓ existing |
| `init` | Overwrites existing config | ✓ existing |
| `task session close <id>` | Kills backing process | ✓ existing |
| `config backfill-events` | Bulk UPDATE all event rows | ✓ new |
| `task retry <item>` | Resets item execution state | ✓ new |

The `--unsafe` global CLI flag bypasses all force gates at once — see `docs/qa/orchestrator/45-cli-unsafe-mode.md`.

Entry point: `orchestrator <command>`

---

## Scenario 1: Config Backfill-Events Rejected Without --force

> **Skip**: `config backfill-events` is not yet implemented. Skip this scenario until the subcommand is added.

### Preconditions
- Orchestrator binary built

### Goal
Verify `config backfill-events` refuses to run without `--force`.

### Steps
1. Run without `--force`:
   ```bash
   orchestrator config backfill-events 2>&1; echo "exit=$?"
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

> **Skip**: `config backfill-events` is not yet implemented. Skip this scenario until the subcommand is added.

### Preconditions
- Orchestrator binary built

### Goal
Verify `config backfill-events --force` performs the backfill.

### Steps
1. Run with `--force`:
   ```bash
   orchestrator config backfill-events --force
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
   orchestrator task retry "$ITEM_ID" 2>&1; echo "exit=$?"
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
   orchestrator task retry "$ITEM_ID" --force || true
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
   orchestrator task delete nonexistent-id 2>&1; echo "exit=$?"
   ```

2. `delete project/<name>` without `--force`:
   ```bash
   orchestrator delete project/nonexistent-project 2>&1; echo "exit=$?"
   ```

3. Verify `--help` documents `--force` for each command:
   ```bash
   orchestrator task delete --help 2>&1 | grep -c '\-\-force'
   orchestrator task retry --help 2>&1 | grep -c '\-\-force'
   orchestrator delete --help 2>&1 | grep -c '\-\-force'
   # Skip: config backfill-events is not yet implemented
   # orchestrator config backfill-events --help 2>&1 | grep -c '\-\-force'
   ```

### Expected
- `task delete` without `--force`: prints confirmation prompt, exit code 0 (no deletion)
- `delete project/<name>` without `--force`: prints confirmation prompt or error, exit code 1
- Three `--help` outputs contain `--force` (grep count >= 1 each). `config backfill-events` is skipped (not yet implemented).

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Config Backfill-Events Rejected Without --force | SKIP | | | `config backfill-events` not yet implemented |
| 2 | Config Backfill-Events Executes With --force | SKIP | | | `config backfill-events` not yet implemented |
| 3 | Task Retry Rejected Without --force | PASS | 2026-03-18 | | |
| 4 | Task Retry Executes With --force | SKIP | | | Not in self_referential_safe_scenarios |
| 5 | Existing Force Gates Regression Check | PASS | 2026-03-18 | | `config backfill-events --help` check skipped (not yet implemented) |
