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

Entry point: `./scripts/orchestrator.sh`

---

## Scenario 1: Phase Result Transactional Persistence Completeness

### Preconditions
- Runtime initialized.
- Structured-output capable workflow/agent is applied.

### Steps
1. Create and run a task:
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/output-formats.yaml
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "single-persist" --goal "command run payload completeness" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
2. Verify run payload columns:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT phase, validation_status, length(output_json), length(artifacts_json), confidence, quality_score FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 20;"
   ```
3. Verify publish/validation events are tied to persisted run IDs:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT cr.id AS run_id, SUM(CASE WHEN e.event_type IN ('phase_output_published','bus_publish_failed') THEN 1 ELSE 0 END) AS publish_evt_count, SUM(CASE WHEN e.event_type='output_validation_failed' THEN 1 ELSE 0 END) AS validation_evt_count FROM command_runs cr LEFT JOIN events e ON e.task_id='${TASK_ID}' AND json_extract(e.payload_json,'$.run_id')=cr.id WHERE cr.task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') GROUP BY cr.id ORDER BY cr.started_at DESC LIMIT 20;"
   ```

### Expected
- No executed run falls back to empty structured payload defaults (`{}` / `[]`) for strict phases (`qa/fix/retest/guard`).
- `validation_status` is populated (`passed` or `failed`), not `unknown`.
- Each persisted run has exactly one publish-path event (`phase_output_published` or `bus_publish_failed`) with matching `run_id`.

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND phase IN ('qa','fix','retest','guard')
  AND (validation_status = 'unknown' OR output_json = '{}' OR artifacts_json = '[]');
-- Expected: 0
```

---

## Scenario 2: Bounded Phase Output Read Marks Truncated Payload

### Preconditions
- Runtime initialized.

### Steps
1. Create a temporary manifest where `qa` prints a very large JSON string (> 300KB):
   ```bash
   cat > /tmp/large-output-manifest.yaml <<'YAML'
   apiVersion: orchestrator/v1
   kind: Bundle
   metadata:
     name: large-output
   spec:
     workspaces:
       default:
         root_path: workspace/default
         qa_targets: ["docs/qa/**/*.md"]
         ticket_dir: docs/ticket
     agents:
       giant:
         metadata: { name: giant }
         capabilities: [qa]
         templates:
           qa: "python3 -c \"import json; print(json.dumps({'confidence':0.9,'quality_score':0.9,'artifacts':[],'payload':'A'*400000}))\""
     workflows:
       default:
         steps:
         - id: qa
           required_capability: qa
   YAML
   ./scripts/orchestrator.sh apply -f /tmp/large-output-manifest.yaml
   ```
2. Run task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "bounded-read" --goal "bounded output read" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
3. Verify truncated metadata in validation event payload:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT event_type, json_extract(payload_json,'$.stdout_truncated_prefix_bytes') AS stdout_cut, json_extract(payload_json,'$.stderr_truncated_prefix_bytes') AS stderr_cut FROM events WHERE task_id='${TASK_ID}' AND event_type='output_validation_failed' ORDER BY id DESC LIMIT 5;"
   sqlite3 data/agent_orchestrator.db "SELECT json_extract(output_json,'$.stdout') FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 1;"
   ```

### Expected
- For oversized strict-phase output, `output_validation_failed` payload records non-zero `stdout_truncated_prefix_bytes`.
- Persisted `output_json.stdout` remains raw tail content and does not prepend synthetic `[truncated ...]` marker text.
- Task still follows strict validation path (likely failed for truncated JSON).

### Expected Data State
```sql
SELECT COUNT(*)
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'output_validation_failed'
  AND CAST(json_extract(payload_json, '$.stdout_truncated_prefix_bytes') AS INTEGER) > 0;
-- Expected: >= 1
```

---

## Scenario 3: task logs Tail Works on Large Log File

### Preconditions
- A task exists with at least one `command_runs` record.

### Steps
1. Get a run stdout path:
   ```bash
   RUN_STDOUT=$(sqlite3 data/agent_orchestrator.db "SELECT stdout_path FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='{task_id}') ORDER BY started_at DESC LIMIT 1;")
   ```
2. Append many lines:
   ```bash
   seq 1 50000 | sed 's/^/tail-check-/' >> "${RUN_STDOUT}"
   ```
3. Read logs:
   ```bash
   ./scripts/orchestrator.sh task logs {task_id} --tail 1
   ```

### Expected
- Log output includes recent suffix lines (for example `tail-check-50000`).
- Command remains responsive without requiring full-file read.

### Expected Data State
```sql
SELECT stdout_path
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC
LIMIT 1;
-- Expected: path exists and was appended in this scenario
```

---

## Scenario 4: Atomic Claim Prevents Duplicate Consumption

### Preconditions
- At least one task is pending.

### Steps
1. Create one pending task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "atomic-claim" --goal "single winner" --detach | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
2. Start worker with parallel consumers:
   ```bash
   ./scripts/orchestrator.sh task worker start --poll-ms 200 --workers 2
   ```
3. Stop worker after completion:
   ```bash
   ./scripts/orchestrator.sh task worker stop
   ```
4. Verify task executed once by phase-run uniqueness:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}');"
   ```

### Expected
- Task transitions `pending -> running -> terminal` without duplicate queue consumption.
- No second worker re-claims the same pending task record.

### Expected Data State
```sql
SELECT status
FROM tasks
WHERE id = '{task_id}';
-- Expected: completed or failed (not left in pending/running due to duplicate claim race)
```

---

## Scenario 5: Multi-Worker Throughput Respects Global Concurrency Bound

### Preconditions
- Multiple pending tasks exist (for example, 20+).

### Steps
1. Batch create detached tasks:
   ```bash
   for i in $(seq 1 20); do
     ./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "mw-${i}" --goal "throughput" --detach >/dev/null
   done
   ```
2. Start high worker count:
   ```bash
   ./scripts/orchestrator.sh task worker start --poll-ms 200 --workers 20
   ```
3. During run, sample running count:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT COUNT(*) FROM tasks WHERE status='running';"
   ```
4. Stop worker:
   ```bash
   ./scripts/orchestrator.sh task worker stop
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
| 1 | Phase Result Transactional Persistence Completeness | ☐ | | | |
| 2 | Bounded Phase Output Read Marks Truncated Payload | ☐ | | | |
| 3 | task logs Tail Works on Large Log File | ☐ | | | |
| 4 | Atomic Claim Prevents Duplicate Consumption | ☐ | | | |
| 5 | Multi-Worker Throughput Respects Global Concurrency Bound | ☐ | | | |
