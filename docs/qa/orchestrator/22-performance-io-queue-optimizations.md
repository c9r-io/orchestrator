---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S2, S3]
---

# Orchestrator - Performance IO and Queue Optimization Regression

**Module**: orchestrator
**Scope**: Validate phase-result transactional persistence, bounded phase output reads, true log tail behavior, and atomic multi-worker queue consumption
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates performance-related refactor behavior introduced in scheduler/db-writer paths:

- phase result persistence writes `command_runs` and related phase events in one transaction
- phase output reads are bounded (tail-based read with size cap)
- bounded read metadata is captured in `output_validation_failed` event payload, without polluting persisted stdout text
- `task logs` tail behavior uses reverse seek scanning for large files
- pending queue consumption is atomic claim-and-run
- worker supports concurrent consumers via `--workers N`, while runtime remains bounded by global semaphore

Entry point: `orchestrator`

---

## Scenario 1: Phase Result Transactional Persistence Completeness

### Preconditions
- Rust toolchain available

### Goal
Verify phase result writes persist all structured fields (output_json, artifacts_json, validation_status) in one transaction — validated via code review + unit test.

### Steps
1. **Code review** — verify transactional write path in phase runner:
   ```bash
   rg -n "insert_command_run|output_json|artifacts_json|validation_status" \
     core/src/task_repository/mod.rs \
     core/src/task_repository/write_ops.rs
   ```

2. **Code review** — verify event publication is tied to run ID:
   ```bash
   rg -n "run_id.*event|event.*run_id" \
     crates/orchestrator-scheduler/src/scheduler/phase_runner/record.rs \
     crates/orchestrator-scheduler/src/scheduler/phase_runner/validate.rs
   ```

3. **Unit test** — run persistence and capture tests:
   ```bash
   cargo test -p agent-orchestrator -- insert_command_run_with_all_optional apply_captures_exit_code extract_event_promoted_fields 2>&1 | tail -5
   ```

### Expected
- `insert_command_run_with_all_optional_fields` passes: all structured columns written
- `apply_captures_exit_code` passes: exit code captured correctly
- `extract_event_promoted_fields_*` passes: event payload includes step/phase info
- No strict-phase run falls back to empty `output_json = '{}'`

---

## Scenario 2: Bounded Phase Output Read Marks Truncated Payload

### Preconditions
- Rust toolchain available

### Goal
Verify bounded output reads track truncation metadata without polluting persisted stdout — validated via code review + unit test.

### Steps
1. **Code review** — verify bounded read / spill logic:
   ```bash
   rg -n "spill_to_file|spill_large_var|truncat" \
     crates/orchestrator-scheduler/src/scheduler/item_executor/spill.rs \
     crates/orchestrator-scheduler/src/scheduler/item_executor/tests.rs
   ```

2. **Code review** — verify output capture redaction:
   ```bash
   rg -n "streaming_redactor|output_capture" crates/orchestrator-runner/src/output_capture.rs | head -10
   ```

3. **Unit test** — run spill and bounded read tests:
   ```bash
   cargo test --workspace --lib -- spill_to_file spill_large_var streaming_redactor resolve_pipeline_var_content_truncated 2>&1 | tail -5
   ```

### Expected
- `spill_to_file_one_byte_over_returns_some` passes: oversized content triggers spill
- `spill_large_var_large_value_sets_correct_path_key` passes: spill path metadata preserved
- `resolve_pipeline_var_content_truncated` passes: truncation metadata captured
- `streaming_redactor_preserves_visible_text` passes: visible text not corrupted by redaction

---

## Scenario 3: task logs Tail Works on Large Log File

### Preconditions
- Rust toolchain available

### Goal
Verify log tail implementation uses efficient reverse-seek scanning — validated via code review + unit test.

### Steps
1. **Code review** — verify tail implementation uses reverse seek:
   ```bash
   rg -n "tail|reverse.*seek|SeekFrom::End|BufRead" \
     crates/orchestrator-scheduler/src/scheduler/query/log_stream.rs
   ```

2. **Code review** — verify stdout spill path supports tail reading:
   ```bash
   rg -n "stdout_path|task_logs|spill.*path" \
     core/src/task_repository/mod.rs \
     core/src/task_repository/write_ops.rs \
     crates/orchestrator-scheduler/src/scheduler/item_executor/tests.rs
   ```

3. **Unit test** — run capture spill tests (validates log file creation path):
   ```bash
   cargo test --workspace --lib -- apply_captures_stdout_spills_under_task_logs 2>&1 | tail -5
   ```

### Expected
- Tail implementation reads from end of file (not full-file scan)
- `apply_captures_stdout_spills_under_task_logs_dir` passes: large outputs spill to task log directory
- stdout_path in command_runs points to valid log file paths

---

## Scenario 4: Atomic Claim Prevents Duplicate Consumption

### Preconditions
- At least one task is pending.

### Steps
1. Create one pending task:
   ```bash
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "atomic-claim" --goal "single winner" | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Start the daemon with parallel embedded workers:
   ```bash
   ./target/release/orchestratord --foreground --workers 2
   ```
3. Stop the daemon after completion:
   ```bash
   kill "${DAEMON_PID}"
   ```
4. Verify task executed once by phase-run uniqueness:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}');"
   ```
5. Verify no duplicate runs **per phase** for this task (important: always scope to `${TASK_ID}`):
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT cr.task_item_id, cr.phase, COUNT(*) as run_count FROM command_runs cr JOIN task_items ti ON cr.task_item_id = ti.id WHERE ti.task_id = '${TASK_ID}' GROUP BY cr.task_item_id, cr.phase HAVING run_count > 1;"
   ```

### Expected
- Task transitions `pending -> running -> terminal` without duplicate queue consumption.
- No second worker re-claims the same pending task record.
- Each task_item has exactly **one** command_run **per phase**. A multi-step workflow naturally produces multiple command_runs per task_item (one per step/phase), which is expected and is NOT a duplicate.
- Step 5 query must return **zero rows** (no duplicate runs for the same item+phase combination).

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: completed or failed (not left in pending/running due to duplicate claim race)

-- Duplicate detection: always scope to the test task and group by phase.
-- A task_item with multiple runs across DIFFERENT phases is expected (multi-step workflow).
-- Only same-phase duplicates indicate a real atomicity issue.
SELECT cr.task_item_id, cr.phase, COUNT(*) as run_count
FROM command_runs cr
JOIN task_items ti ON cr.task_item_id = ti.id
WHERE ti.task_id = '{task_id}'
GROUP BY cr.task_item_id, cr.phase
HAVING run_count > 1;
-- Expected: 0 rows
```

### Troubleshooting

| Symptom | Likely Cause | Resolution |
|---------|-------------|------------|
| Global `GROUP BY task_item_id HAVING COUNT(*) > 1` shows duplicates | Query is not scoped to the test task — picks up runs from other tasks with multi-step workflows | Always filter by `ti.task_id = '${TASK_ID}'` and group by `cr.phase` |
| Multiple runs per item but each has a **different** phase | Expected behavior for workflows with multiple steps (e.g., qa → fix → retest) | Not a bug — each step creates its own command_run |
| Multiple runs for the **same** item+phase | Real atomicity issue — investigate claim mechanism or loop re-entry | File a ticket with task_id-scoped evidence |

---

## Scenario 5: Multi-Worker Throughput Respects Global Concurrency Bound

### Preconditions
- Multiple pending tasks exist (for example, 20+).

### Steps
1. Batch create queued tasks:
   ```bash
   for i in $(seq 1 20); do
     orchestrator task create --project "${QA_PROJECT}" --name "mw-${i}" --goal "throughput" >/dev/null
   done
   ```
2. Start high daemon worker count:
   ```bash
   ./target/release/orchestratord --foreground --workers 20
   ```
3. During run, sample running count:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM tasks WHERE status='running';"
   ```
4. Stop the daemon:
   ```bash
   kill "${DAEMON_PID}"
   ```

### Expected
- Pending queue drains faster than single worker baseline.
- Running task count should stay bounded by configured runtime semaphore cap.

### Expected Data State
```sql
SELECT COUNT(*)
FROM tasks
WHERE status = 'running';
-- Expected: value never exceeds runtime semaphore max (default 10)
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Phase Result Transactional Persistence Completeness | ✅ | 2026-03-30 | claude | Code review + unit test: insert_command_run_with_all_optional_fields, apply_captures_exit_code, extract_event_promoted_fields (7 sub-tests) all pass |
| 2 | Bounded Phase Output Read Marks Truncated Payload | ✅ | 2026-03-30 | claude | Code review + unit test: spill_to_file*, spill_large_var*, streaming_redactor*, resolve_pipeline_var_content_truncated* (14 tests) all pass |
| 3 | task logs Tail Works on Large Log File | ✅ | 2026-03-30 | claude | Code review + unit test: tail uses SeekFrom::End, apply_captures_stdout_spills_under_task_logs_dir passes |
| 4 | Atomic Claim Prevents Duplicate Consumption | — | | | UNSAFE — skipped per self_referential_safe_scenarios |
| 5 | Multi-Worker Throughput Respects Global Concurrency Bound | — | | | UNSAFE — skipped per self_referential_safe_scenarios |
