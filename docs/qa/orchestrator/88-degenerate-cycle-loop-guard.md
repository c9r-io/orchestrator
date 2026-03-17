---
self_referential_safe: false
---

# Orchestrator - Degenerate Cycle Loop Guard Verification

**Module**: orchestrator
**Scope**: Validate rapid cycle detection (L2), trace anomaly reporting, blocked item recovery, and unit-tested circuit breaker (L1)
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the degenerate cycle detection and circuit breaker mechanism (FR-035). The feature provides layered defense against tasks that enter rapid-fire failing cycles:

- **L1**: Per-item per-step circuit breaker — blocks individual items after `max_item_step_failures` consecutive failures, with exponential backoff (30s/120s) before blocking.
- **L2**: Rapid cycle detection — pauses the entire task when the last 3 inter-cycle intervals are all below `min_cycle_interval_secs`.
- **Trace**: `DegenerateLoop` anomaly is emitted when command_runs show 3+ consecutive failures for the same item-phase pair.

In practice, L2 fires first on rapid cycles (preventing wasteful retries); L1 backoff prevents re-execution until the delay expires. The circuit breaker activates on the 3rd failure after backoff periods elapse (~150s). Because L1 requires wall-clock backoff waits, it is validated via unit tests (Scenario 5); L2 and trace anomaly are validated via live CLI execution.

Design doc: `docs/design_doc/orchestrator/12-degenerate-cycle-loop-guard.md`

### Common Preconditions

```bash
# 1. Clean stale tickets
rm -f fixtures/ticket/auto_*.md 2>/dev/null || true

# 2. Apply fixture and create isolated QA project
QA_PROJECT="qa-fr035-${USER}-$(date +%Y%m%d%H%M%S)"
./target/debug/orchestrator apply -f fixtures/manifests/bundles/degenerate-loop-guard.yaml
./target/debug/orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
./target/debug/orchestrator apply -f fixtures/manifests/bundles/degenerate-loop-guard.yaml --project "${QA_PROJECT}"
echo "QA_PROJECT=${QA_PROJECT}"
```

Fixture: `fixtures/manifests/bundles/degenerate-loop-guard.yaml`
- Workspace: `fixtures/qa-fr035/` (2 narrow QA target files)
- `circuit_breaker_test`: item-scoped `qa_testing` always exits 1, `min_cycle_interval_secs: 1`, `max_item_step_failures: 3`, up to 8 cycles
- `rapid_cycle_test`: item-scoped `qa_testing` succeeds instantly, `min_cycle_interval_secs: 600`, `stop_when_no_unresolved: false`, up to 8 cycles
- `normal_flow_test`: single `qa_testing` succeeds, mode once

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Rapid cycle detection not triggered | Task finishes before 4 cycles | Increase `max_cycles` or verify loop mode is `infinite` |
| Task completes after 1 cycle with `no_unresolved_items` | `stop_when_no_unresolved` defaults to `true`; items resolved after first QA pass | Set `stop_when_no_unresolved: false` in the workflow loop config |
| Stale agents interfere with selection | `apply` is additive; other fixtures injected agents | Recreate isolated project with fresh `QA_PROJECT` |
| No degenerate_loop anomaly in trace | Fewer than 3 command_runs for the same item-phase pair | Verify command_runs table has 3+ failing entries |

---

## Scenario 1: Rapid Cycle Detection (L2) Auto-Pauses Task

### Goal

Verify that when cycle intervals are shorter than `min_cycle_interval_secs`, the task auto-pauses after 4 cycles.

### Preconditions

- Common Preconditions applied

### Steps

1. Create and start a task with `rapid_cycle_test` workflow (`min_cycle_interval_secs: 600`):
   ```bash
   S2_OUT=$(./target/debug/orchestrator task create \
     --name "s1-rapid-cycle" \
     --goal "verify rapid cycle auto-pause" \
     --project "${QA_PROJECT}" \
     --workflow rapid_cycle_test \
     --no-start 2>&1)
   S2_TASK=$(echo "${S2_OUT}" | sed -n 's/.*Task created: //p')
   echo "S2_TASK=${S2_TASK}"
   ./target/debug/orchestrator task start "${S2_TASK}"
   ```

2. Wait for detection (~10s, cycles are instant):
   ```bash
   sleep 10
   ```

3. Check task status:
   ```bash
   ./target/debug/orchestrator task info "${S2_TASK}"
   ```

4. Verify the degenerate cycle event:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, json_extract(payload_json, '$.cycle') AS cycle,
             json_extract(payload_json, '$.min_cycle_interval_secs') AS threshold
      FROM events
      WHERE task_id = '${S2_TASK}'
        AND event_type = 'degenerate_cycle_detected'"
   ```

### Expected Results

- Task status is `paused` (not `completed` or `failed`)
- Current cycle >= 4
- A `degenerate_cycle_detected` event exists with `min_cycle_interval_secs: 600`
- Task stopped executing new cycles after detection

---

## Scenario 2: Failing Step Triggers Backoff and Trace Anomaly

### Goal

Verify that repeated item-step failures produce backoff skip events and a `degenerate_loop` trace anomaly once enough command_runs accumulate.

### Preconditions

- Common Preconditions applied

### Steps

1. Create and start a task with `circuit_breaker_test` workflow (`min_cycle_interval_secs: 1`):
   ```bash
   S1_OUT=$(./target/debug/orchestrator task create \
     --name "s2-circuit-breaker" \
     --goal "verify backoff and trace anomaly" \
     --project "${QA_PROJECT}" \
     --workflow circuit_breaker_test \
     --no-start 2>&1)
   S1_TASK=$(echo "${S1_OUT}" | sed -n 's/.*Task created: //p')
   echo "S1_TASK=${S1_TASK}"
   ./target/debug/orchestrator task start "${S1_TASK}"
   ```

2. Wait for task to complete or pause:
   ```bash
   sleep 15
   ```

3. Check events:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, COUNT(*)
      FROM events WHERE task_id = '${S1_TASK}'
      GROUP BY event_type ORDER BY event_type"
   ```

4. Check command_runs per item:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT task_item_id, phase, exit_code, COUNT(*) as runs
      FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '${S1_TASK}')
      GROUP BY task_item_id, phase"
   ```

5. Check trace:
   ```bash
   ./target/debug/orchestrator task trace "${S1_TASK}"
   ```

### Expected Results

- The first cycle executes `qa_testing` for each item (exit 1)
- Subsequent cycles skip items due to `retry_backoff` (event: `step_skipped` with reason `retry_backoff`)
- `nonzero_exit` anomalies appear in trace for the failing command_runs
- If the task ran long enough for 3+ command_runs per item-phase, a `degenerate_loop` anomaly also appears
- The task eventually pauses (L2) or reaches max_cycles (failed)

---

## Scenario 3: Trace Anomaly Validates DegenerateLoop Detection

### Goal

Verify that the `detect_degenerate_loop` trace detector correctly identifies item-phase pairs with 3+ consecutive command_run failures.

### Preconditions

- A completed/paused task with at least 3 consecutive failing command_runs for the same (item_id, phase). If Scenario 2 did not produce enough runs, manually insert test data:
  ```bash
  # Find an item from Scenario 2
  ITEM_ID=$(sqlite3 data/agent_orchestrator.db \
    "SELECT id FROM task_items WHERE task_id = '${S1_TASK}' LIMIT 1")
  # Insert additional failing command_runs to reach 3+ consecutive
  sqlite3 data/agent_orchestrator.db \
    "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, started_at, interrupted)
     VALUES
     ('fr035-run-2', '${ITEM_ID}', 'qa_testing', 'echo fail', '.', 'default', 'mock_always_fail', 1, '', '', datetime('now', '-2 seconds'), 0),
     ('fr035-run-3', '${ITEM_ID}', 'qa_testing', 'echo fail', '.', 'default', 'mock_always_fail', 1, '', '', datetime('now', '-1 seconds'), 0)"
  ```

### Steps

1. Run trace:
   ```bash
   ./target/debug/orchestrator task trace "${S1_TASK}"
   ```

2. Check for degenerate_loop anomaly in JSON:
   ```bash
   ./target/debug/orchestrator task trace "${S1_TASK}" --json 2>/dev/null | \
     python3 -c "import sys,json; d=json.load(sys.stdin); [print(a['rule'],a['severity'],a['message']) for a in d.get('anomalies',[]) if a['rule']=='degenerate_loop']"
   ```

### Expected Results

- Trace ANOMALIES section contains `degenerate_loop` with severity `error`
- Anomaly message identifies the item and phase, e.g.:
  ```
  [ERROR] degenerate_loop: Item '<item-id>' phase 'qa_testing' failed N times consecutively (last exit: 1)
  ```
- Escalation is `intervene`

---

## Scenario 4: Blocked Item Recovery via `task resume --reset-blocked`

### Goal

Verify that `--reset-blocked` resets blocked items back to `unresolved` and the task resumes.

### Preconditions

- A task with at least one blocked item. Set one manually:
  ```bash
  # Use a paused task from earlier scenarios
  ITEM_ID=$(sqlite3 data/agent_orchestrator.db \
    "SELECT id FROM task_items WHERE task_id = '${S1_TASK}' LIMIT 1")
  sqlite3 data/agent_orchestrator.db \
    "UPDATE task_items SET status = 'blocked' WHERE id = '${ITEM_ID}'"
  # Ensure task is paused
  ./target/debug/orchestrator task pause "${S1_TASK}" 2>/dev/null || true
  sleep 2
  ```

### Steps

1. Verify item is blocked:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, status FROM task_items WHERE task_id = '${S1_TASK}' AND status = 'blocked'"
   ```

2. Check task info shows `[BLOCKED]`:
   ```bash
   ./target/debug/orchestrator task info "${S1_TASK}"
   ```

3. Resume with `--reset-blocked`:
   ```bash
   ./target/debug/orchestrator task resume "${S1_TASK}" --reset-blocked
   ```

4. Verify items unblocked:
   ```bash
   sleep 2
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, status FROM task_items WHERE task_id = '${S1_TASK}'"
   ```

5. Check task info:
   ```bash
   ./target/debug/orchestrator task info "${S1_TASK}"
   ```

### Expected Results

- Before reset: item status is `blocked`, `task info` shows `[BLOCKED]` tag
- After `resume --reset-blocked`: items return to `unresolved`
- The CLI prints confirmation message including task ID
- The `[BLOCKED]` tag no longer appears in `task info`

---

## Scenario 5: Unit Test Validation (Circuit Breaker + Config + Anomaly)

### Goal

Verify that all FR-035 unit tests pass: safety config serde, anomaly rule definitions, circuit breaker logic, and degenerate loop trace detection.

### Preconditions

- Rust toolchain available, project builds successfully

### Steps

1. Run FR-035 related unit tests:
   ```bash
   cd core && cargo test fr035 2>&1
   cargo test degenerate 2>&1
   cargo test circuit 2>&1
   cargo test -- --test-threads=1 anomaly::tests 2>&1
   cargo test -- --test-threads=1 safety::tests 2>&1
   ```

### Expected Results

- `test_fr035_fields_serde_round_trip` — passes: `max_item_step_failures` and `min_cycle_interval_secs` serialize/deserialize correctly
- `test_fr035_fields_explicit_json_deserialization` — passes: explicit JSON values override defaults
- `canonical_name_roundtrip` — passes: `DegenerateLoop` round-trips through `from_canonical`
- `severity_mapping` — passes: `DegenerateLoop` maps to `Error` severity
- `escalation_mapping` — passes: `DegenerateLoop` maps to `Intervene` escalation
- `degenerate_loop_emits_anomaly_on_three_consecutive_failures` — passes: 3+ consecutive exit-1 runs trigger anomaly
- `degenerate_loop_no_anomaly_when_fewer_than_three_consecutive_failures` — passes: 2 failures produce no anomaly
- `degenerate_loop_no_anomaly_when_failures_are_non_consecutive` — passes: interrupted failure streak produces no anomaly

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
