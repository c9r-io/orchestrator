---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S2, S3]
---

# Orchestrator - Structured Output Mainline and Worker Scheduler

**Module**: orchestrator
**Scope**: Validate strict structured output enforcement, command_runs structured persistence, and detach/worker scheduling flow
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the refactor that moved `collab` capabilities into the scheduler main path:

- strict JSON output validation for `qa`/`fix`/`retest`/`guard`
- structured output persistence in `command_runs`
- phase execution result publication to MessageBus with observable events
- dual CLI model: foreground run and detach queue + worker loop
- C/S mode: daemon-embedded workers replace standalone worker lifecycle commands

Entry point: `orchestrator` (CLI client) or `orchestratord` (daemon)

**C/S mode note**: Scenarios 4 and 5 can also be validated through the C/S architecture where `orchestratord --workers N` embeds the worker loop directly in the daemon process. See `docs/qa/orchestrator/53-client-server-architecture.md` for dedicated C/S scenarios.

---

## Database Schema Reference

### Table: command_runs
| Column | Type | Notes |
|--------|------|-------|
| output_json | TEXT | Serialized `AgentOutput` |
| artifacts_json | TEXT | Serialized artifact list |
| confidence | REAL | Parsed confidence value |
| quality_score | REAL | Parsed quality score value |
| validation_status | TEXT | `passed` / `failed` / `unknown` |

### Table: events
| Column | Type | Notes |
|--------|------|-------|
| event_type | TEXT | Includes `output_validation_failed`, `phase_output_published`, `scheduler_enqueued` |
| payload_json | TEXT | Event payload details |

---

## Scenario 1: Strict Validation Rejects Non-JSON QA Output

### Preconditions
- Rust toolchain available

### Goal
Verify strict-mode validation fails phase output when `qa` stdout is not JSON — validated via code review + unit test.

### Steps
1. **Code review** — verify strict phase validation logic:
   ```bash
   rg -n "strict_phase|is_strict_phase|requires_json" core/src/output_validation.rs
   ```

2. **Code review** — verify validation failure maps to exit_code -6:
   ```bash
   rg -n "exit_code.*-6|validation_failure.*exit|effective_exit_code" \
     crates/orchestrator-scheduler/src/scheduler/phase_runner/tests.rs \
     crates/orchestrator-scheduler/src/scheduler/phase_runner/util.rs \
     crates/orchestrator-scheduler/src/scheduler/phase_runner/validate.rs
   ```

3. **Unit test** — run strict validation tests:
   ```bash
   cargo test --workspace --lib -- strict_phase_requires_json strict_phase_accepts_json effective_exit_code_maps_validation 2>&1 | tail -5
   ```

### Expected
- `strict_phase_requires_json` passes: non-JSON qa output is rejected
- `strict_phase_accepts_json` passes: valid JSON qa output is accepted
- `effective_exit_code_maps_validation_failure_to_nonzero` passes: validation failure → exit_code -6

---

## Scenario 2: Structured Output Persists Into command_runs

### Preconditions
- Rust toolchain available

### Goal
Verify structured fields (output_json, artifacts_json, confidence, quality_score, validation_status) are captured and persisted — validated via code review + unit test.

### Steps
1. **Code review** — verify AgentOutput struct fields:
   ```bash
   rg -n "struct AgentOutput|confidence|quality_score|artifacts" core/src/collab/output.rs
   ```

2. **Code review** — verify command_run insertion includes structured fields:
   ```bash
   rg -n "insert_command_run|output_json|artifacts_json|validation_status" \
     core/src/task_repository/mod.rs \
     core/src/task_repository/write_ops.rs
   ```

3. **Unit test** — run output capture and persistence tests:
   ```bash
   cargo test --workspace --lib -- test_agent_output_creation apply_captures_stdout_json_path insert_command_run_with_all_optional 2>&1 | tail -5
   ```

### Expected
- `test_agent_output_creation` passes: AgentOutput holds confidence, quality_score, artifacts
- `apply_captures_stdout_json_path_extracts_score` passes: JSON path extraction works for structured fields
- `insert_command_run_with_all_optional_fields` passes: all structured columns persisted to DB

---

## Scenario 3: Scheduler Publishes Phase Output Events

### Preconditions
- Rust toolchain available

### Goal
Verify phase outputs are published as observable events — validated via code review + unit test.

### Steps
1. **Code review** — verify event publication in phase runner:
   ```bash
   rg -n "phase_output_published|bus_publish_failed|output_validation_failed" \
     crates/orchestrator-scheduler/src/scheduler/phase_runner/record.rs
   ```

2. **Code review** — verify event types are stored with run_id:
   ```bash
   rg -n "run_id|event_type.*phase_output" \
     core/src/db_write.rs \
     core/src/task_repository/write_ops.rs
   ```

3. **Unit test** — run trace and event tests:
   ```bash
   cargo test --workspace --lib -- build_trace single_cycle_with_steps extract_event_promoted 2>&1 | tail -5
   ```

### Expected
- Phase runner emits `phase_output_published` event on success path
- Phase runner emits `output_validation_failed` event when validation fails
- `build_trace_*` tests pass: events are correctly captured in execution trace
- `extract_event_promoted_fields_*` tests pass: event payload fields extracted correctly

---

## Scenario 4: Queue-Only Lifecycle Enqueues Tasks

### Preconditions
- Runtime initialized and config applied.

### Goal
Verify task lifecycle commands no longer execute inline and always enqueue work for daemon processing.

### Steps
1. Create a task:
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "queue-create" --goal "queue" | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Enqueue an existing task explicitly:
   ```bash
   orchestrator task start "${TASK_ID}"
   ```
3. Query queue and scheduling events:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
   sqlite3 data/agent_orchestrator.db "SELECT event_type FROM events WHERE task_id='${TASK_ID}' AND event_type='scheduler_enqueued' ORDER BY id DESC LIMIT 5;"
   ```

### Expected
- Task status remains `pending` until a worker consumes it.
- `scheduler_enqueued` event exists.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: 'pending' before worker consumption
```

---

## Scenario 5: Worker Start/Stop and Queue Consumption

### Preconditions
- At least one pending task exists.

### Goal
Verify worker loop consumes pending tasks and honors stop signal.

### Steps
1. Start the daemon with multiple embedded workers in terminal A:
   ```bash
   ./target/release/orchestratord --foreground --workers 3
   ```
2. In terminal B, monitor queue and task progress:
   ```bash
   orchestrator task list -o json
   orchestrator task watch "${TASK_ID}" --interval 1
   ```
3. Stop the daemon after the queue drains:
   ```bash
   kill "${DAEMON_PID}"
   ```
4. Wait for daemon process to fully exit:
   ```bash
   while kill -0 "${DAEMON_PID}" 2>/dev/null; do sleep 1; done
   ```

### Expected
- Worker consumes pending tasks and updates task status to terminal state.
- Pending queue claim is atomic under parallel consumers (no duplicate pending-task execution).
- Stopping the daemon terminates embedded workers gracefully.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `stop_signal: true` after worker exits | Worker exited with error before cleanup ran | Fixed: cleanup now runs before error propagation. If still seen, check for process crash. |

### Expected Data State
```sql
SELECT id, status
FROM tasks
WHERE id = '{task_id}';
-- Expected: status transitions from pending -> running -> completed/failed
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Strict Validation Rejects Non-JSON QA Output | PASS | 2026-03-21 | Claude | Code review + unit test (strict_phase_requires_json, strict_phase_accepts_json, effective_exit_code_maps_validation_failure_to_nonzero) |
| 2 | Structured Output Persists Into command_runs | PASS | 2026-03-21 | Claude | Code review + unit test (test_agent_output_creation, apply_captures_stdout_json_path_extracts_score, insert_command_run_with_all_optional_fields) |
| 3 | Scheduler Publishes Phase Output Events | PASS | 2026-03-21 | Claude | Code review + unit test (build_trace_*, single_cycle_with_steps, extract_event_promoted_fields_*) |
| 4 | Detach Mode Enqueues Tasks | SKIP | | | UNSAFE — daemon queue lifecycle (self-referential mode) |
| 5 | Worker Start/Stop and Queue Consumption | SKIP | | | UNSAFE — daemon worker lifecycle (start/kill) (self-referential mode) |
