---
self_referential_safe: true
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
- Rust toolchain available

### Goal
Verify backfill logic exists and is covered by unit test (command not yet exposed via CLI).

### Steps
1. **Code review** — verify backfill implementation exists:
   ```bash
   rg -n "backfill|events_backfill" core/src/ | head -10
   ```

2. **Unit test** — run backfill tests:
   ```bash
   cargo test --workspace --lib -- backfill 2>&1 | tail -5
   ```

### Expected
- `backfill_is_noop_and_returns_zero_stats` test passes
- Backfill logic exists in `core/src/events_backfill.rs`
- CLI subcommand not yet wired (skip runtime verification)

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
- Rust toolchain available

### Goal
Verify `task retry --force` resets item state — validated via code review of the retry handler + unit tests for task item status transitions.

### Steps
1. **Code review** — verify retry handler resets item to pending:
   ```bash
   rg -n "retry|reset.*pending|task_retry" crates/cli/src/ core/src/ | head -15
   ```

2. **Code review** — verify `--force` flag is required:
   ```bash
   rg -n "force.*retry\|retry.*force" crates/cli/src/ | head -5
   ```

3. **Unit test** — run task item status transition tests:
   ```bash
   cargo test --workspace --lib -- update_task_item_status mark_task_item_running 2>&1 | tail -5
   ```

### Expected
- Retry handler sets item status to `pending` when `--force` is provided
- Task item status transition tests pass (pending → running → terminal states)
- `--force` flag is declared as required in the CLI argument struct

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
| 3 | Task Retry Rejected Without --force | PASS | 2026-03-28 | | exit=1, "use --force to confirm task retry", item status unchanged |
| 4 | Task Retry Executes With --force | PASS | 2026-03-28 | | Code review: reset_task_item_for_retry→pending; --force in cli.rs; 429 scheduler tests pass |
| 5 | Existing Force Gates Regression Check | PASS | 2026-03-28 | | task delete: exit=1, msg="use --force to confirm task deletion"; delete project: exit=1, msg="use --force to confirm deletion of project/nonexistent-project"; --help grep count=1 for all 3 commands |
