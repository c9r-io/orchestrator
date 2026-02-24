# Orchestrator - Exec Interactive Simulation

**Module**: orchestrator  
**Scope**: Validate interactive execution simulation for `orchestrator exec -it task/<task_id>/step/<step_id>`  
**Scenarios**: 5  
**Priority**: High

---

## Background

This document focuses on interaction simulation approaches for `exec -it`:

- stdin pipe simulation (`printf ... | exec -it ... -- cat`)
- here-doc shell simulation (`cat <<EOF | exec -it ... -- /bin/bash`)
- non-tty guard behavior for `-it`
- reusable automation script under `docs/qa/script/`

Entry point: `./scripts/orchestrator.sh`

---

## Scenario 1: Build Runnable Context for Interactive Exec

### Preconditions

- Release binary exists.
- Runtime can be re-initialized for isolated QA.

### Steps

1. Build and reset:
   ```bash
   (cd core && cargo build --release)
   ./scripts/orchestrator.sh init --force
   ./scripts/orchestrator.sh db reset --force --include-config
   ```
2. Execute reusable script setup:
   ```bash
   ./docs/qa/script/test-exec-interactive.sh --json
   ```

### Expected

- Script returns JSON summary without setup errors.
- One task is created with inserted `plan` step (`tty=true`).

### Expected Data State
```sql
SELECT COUNT(*)
FROM tasks
WHERE workflow_id = 'exec_interactive_flow';
-- Expected: >= 1
```

---

## Scenario 2: Simulate Interactive Input via stdin Pipe

### Preconditions

- A task exists with inserted `plan-*` step where `tty=true`.

### Steps

1. Resolve `task_id` and `plan step id` from latest script output.
2. Run:
   ```bash
   printf 'sim-tty-input\n' | ./scripts/orchestrator.sh exec -it task/{task_id}/step/{plan_step_id} -- cat
   ```

### Expected

- Command exits successfully.
- Output contains `sim-tty-input`.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: valid lifecycle state (no corruption)
```

---

## Scenario 3: Simulate Multi-line Interaction via Here-Doc

### Preconditions

- Same task and `plan` step from Scenario 2.

### Steps

1. Run:
   ```bash
   cat <<'EOF' | ./scripts/orchestrator.sh exec -it task/{task_id}/step/{plan_step_id} -- /bin/bash
   echo SIM-HEREDOC
   exit
   EOF
   ```

### Expected

- Command exits successfully.
- Output contains `SIM-HEREDOC`.

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND phase = 'plan';
-- Expected: >= 1
```

---

## Scenario 4: `-it` Rejects Non-TTY Step

### Preconditions

- Same task has step `qa` with `tty=false`.

### Steps

1. Run:
   ```bash
   ./scripts/orchestrator.sh exec -it task/{task_id}/step/qa -- cat
   ```

### Expected

- Command fails with non-zero exit code.
- Error message indicates `tty` is disabled for step `qa`.

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND phase = 'qa'
  AND command LIKE '%-- cat%';
-- Expected: 0
```

---

## Scenario 5: Reusable Script Regression Execution

### Preconditions

- Script file exists and is executable.

### Steps

1. Execute:
   ```bash
   ./docs/qa/script/test-exec-interactive.sh --json
   ```
2. Validate returned fields:
   - `task_id`
   - `plan_step_id`
   - `pipe_pass`
   - `heredoc_pass`
   - `non_tty_reject_pass`
   - `pass`

### Expected

- JSON output includes all fields.
- `pipe_pass=true`
- `heredoc_pass=true`
- `non_tty_reject_pass=true`
- `pass=true`

### Expected Data State
```sql
SELECT COUNT(*)
FROM events
WHERE event_type IN ('step_started', 'step_finished')
  AND task_id = '{task_id}';
-- Expected: >= 2
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Build Runnable Context for Interactive Exec | ☐ | | | |
| 2 | Simulate Interactive Input via stdin Pipe | ☐ | | | |
| 3 | Simulate Multi-line Interaction via Here-Doc | ☐ | | | |
| 4 | `-it` Rejects Non-TTY Step | ☐ | | | |
| 5 | Reusable Script Regression Execution | ☐ | | | |
