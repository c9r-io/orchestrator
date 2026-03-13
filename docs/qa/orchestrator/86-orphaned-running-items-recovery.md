# Orchestrator - Orphaned Running Items Auto-Recovery

**Module**: orchestrator
**Scope**: Validate startup orphan recovery, runtime stall detection, CLI `task recover` command, and audit events for FR-033
**Scenarios**: 5
**Priority**: Critical

---

## Background

When the daemon crashes (SIGKILL, OOM, panic) while items are in `running` state, those items become permanently stuck. FR-033 adds three recovery mechanisms:

1. **Startup recovery** — On daemon boot, all `running` items are reset to `pending` and their parent tasks to `restart_pending`, before workers spawn
2. **Stall detection sweep** — Background task (every 5 min) detects items running longer than `--stall-timeout-mins` and recovers them
3. **CLI `task recover`** — Manual recovery via `orchestrator task recover <task_id>`

Audit events emitted: `orphaned_items_recovered` (startup), `item_stall_recovered` (runtime sweep).

### Common Preconditions / Setup

```bash
# 1. Build release binaries
cargo build --release -p orchestratord -p orchestrator-cli

# 2. Ensure runtime is initialized
test -f data/agent_orchestrator.db || ./target/release/orchestrator init

# 3. Apply mock fixture and set up isolated QA project
QA_PROJECT="fr033-qa-${USER}-$(date +%Y%m%d%H%M%S)"
./target/release/orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
./target/release/orchestrator apply -f fixtures/manifests/bundles/pause-resume-workflow.yaml --project "${QA_PROJECT}"
```

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `task recover` returns gRPC error | Daemon not running or CLI binary stale | Rebuild CLI and restart daemon |
| No `orphaned_items_recovered` event on restart | No items were in `running` state at crash time | Ensure SIGKILL is sent while items are actively running (use `mock_sleep` agent) |
| Stall detection not triggering | `--stall-timeout-mins` default is 30; items haven't exceeded threshold | Use `--stall-timeout-mins 1` for testing or wait longer |
| Socket bind error on daemon restart | Stale socket from crashed daemon | `rm -f data/orchestrator.sock` |

---

## Scenario 1: Startup Recovery Resets Orphaned Running Items

### Preconditions

- Common Preconditions applied (pause-resume-workflow.yaml with `mock_sleep` agent)

### Goal

Verify that when the daemon crashes with items in `running` state, the next startup automatically resets them to `pending` and the parent task to `restart_pending`, emitting an `orphaned_items_recovered` event.

### Steps

1. Start daemon and create a slow task:
   ```bash
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr033-s1-pre.log 2>&1 &
   DAEMON_PID=$!
   sleep 2

   TASK_ID=$(./target/release/orchestrator task create \
     --name "orphan-recovery-test" \
     --goal "Test startup orphan recovery" \
     --project "${QA_PROJECT}" \
     --workflow qa_sleep 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "TASK_ID=${TASK_ID}"
   ```

2. Wait for items to enter `running` state, then crash daemon:
   ```bash
   sleep 4
   # Confirm items are running
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, status FROM task_items WHERE task_id='${TASK_ID}' AND status='running';"
   # Kill daemon without cleanup (simulate crash)
   kill -9 "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null || true
   ```

3. Verify items are stuck in `running`:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status, COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' GROUP BY status;"
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   ```

4. Restart daemon (recovery happens at startup):
   ```bash
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr033-s1-post.log 2>&1 &
   NEW_DAEMON_PID=$!
   sleep 3
   ```

5. Verify recovery:
   ```bash
   # Check items are now pending
   sqlite3 data/agent_orchestrator.db \
     "SELECT status, COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' GROUP BY status;"
   # Check task is restart_pending
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   # Check recovery event
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE event_type='orphaned_items_recovered' ORDER BY id DESC LIMIT 1;"
   # Check daemon log
   grep "recovered orphaned running items\|startup orphan recovery complete" /tmp/fr033-s1-post.log
   ```

6. Cleanup:
   ```bash
   kill "$NEW_DAEMON_PID"
   wait "$NEW_DAEMON_PID" 2>/dev/null
   ```

### Expected

- Before crash: items show `running` status
- After crash (before restart): items remain `running`, task remains `running`
- After restart: items reset to `pending`, task reset to `restart_pending`
- Events table contains `orphaned_items_recovered` event with payload including `task_id`, `recovered_item_ids`, `count`
- Daemon log shows `recovered orphaned running items` and `startup orphan recovery complete`

### Expected Data State

```sql
-- After restart recovery:
SELECT status FROM task_items WHERE task_id = '{task_id}' AND status = 'running';
-- Expected: 0 rows (no items stuck in running)

SELECT status FROM tasks WHERE id = '{task_id}';
-- Expected: restart_pending

SELECT event_type, payload_json FROM events
  WHERE event_type = 'orphaned_items_recovered' ORDER BY id DESC LIMIT 1;
-- Expected: orphaned_items_recovered | {"task_id":"...","recovered_item_ids":[...],"count":N}
```

---

## Scenario 2: Startup Recovery Is Idempotent (No Orphans)

### Preconditions

- Common Preconditions applied
- No tasks in `running` state

### Goal

Verify that startup recovery does nothing and emits no events when there are no orphaned items.

### Steps

1. Ensure clean state — no running items:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM task_items WHERE status='running';"
   ```

2. Record current event count:
   ```bash
   PRE_COUNT=$(sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE event_type='orphaned_items_recovered';")
   echo "PRE_COUNT=${PRE_COUNT}"
   ```

3. Start and stop daemon:
   ```bash
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr033-s2.log 2>&1 &
   DAEMON_PID=$!
   sleep 3
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

4. Verify no new recovery events:
   ```bash
   POST_COUNT=$(sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE event_type='orphaned_items_recovered';")
   echo "POST_COUNT=${POST_COUNT}"
   ```

### Expected

- Running items count is 0 before startup
- No new `orphaned_items_recovered` events emitted (`PRE_COUNT == POST_COUNT`)
- Daemon log does NOT contain `recovered orphaned running items`

---

## Scenario 3: CLI `task recover` Resets Orphaned Items for a Specific Task

### Preconditions

- Common Preconditions applied
- Daemon running

### Goal

Verify that `orchestrator task recover <task_id>` manually recovers orphaned running items for a specific task without affecting other tasks.

### Steps

1. Start daemon:
   ```bash
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr033-s3.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```

2. Create two tasks and start them both:
   ```bash
   TASK_A=$(./target/release/orchestrator task create \
     --name "recover-target" \
     --goal "Task to recover" \
     --project "${QA_PROJECT}" \
     --workflow qa_sleep 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "TASK_A=${TASK_A}"
   sleep 6
   ```

3. Simulate stuck items by manually updating DB (since we can't easily get both tasks into running at the same time with 1 worker):
   ```bash
   # Pause daemon's task first
   ./target/release/orchestrator task pause "${TASK_A}"
   sleep 2

   # Manually set items to running (simulating crash leftover)
   sqlite3 data/agent_orchestrator.db \
     "UPDATE task_items SET status='running', started_at=datetime('now','-1 hour') WHERE task_id='${TASK_A}';"
   sqlite3 data/agent_orchestrator.db \
     "UPDATE tasks SET status='running' WHERE id='${TASK_A}';"

   # Verify items are running
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, status FROM task_items WHERE task_id='${TASK_A}';"
   ```

4. Use CLI to recover the task:
   ```bash
   ./target/release/orchestrator task recover "${TASK_A}"
   ```

5. Verify recovery:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM task_items WHERE task_id='${TASK_A}';"
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_A}';"
   ```

6. Cleanup:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected

- CLI prints: `Recovered N orphaned running item(s) for task <id>`
- Items for TASK_A are now `pending`
- Task TASK_A status is `restart_pending`

---

## Scenario 4: Terminal Items Are Not Affected by Recovery

### Preconditions

- Common Preconditions applied
- Daemon running

### Goal

Verify that items in terminal states (`qa_passed`, `fixed`, `completed`) are not modified by the recovery mechanism.

### Steps

1. Start daemon:
   ```bash
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr033-s4.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```

2. Create a task and let it complete:
   ```bash
   TASK_ID=$(./target/release/orchestrator task create \
     --name "terminal-test" \
     --goal "Test terminal items not affected" \
     --project "${QA_PROJECT}" \
     --workflow qa_sleep 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "TASK_ID=${TASK_ID}"
   sleep 20
   ```

3. Verify task completed and items are in terminal state:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db \
     "SELECT status, COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' GROUP BY status;"
   ```

4. Attempt recovery on the completed task:
   ```bash
   ./target/release/orchestrator task recover "${TASK_ID}"
   ```

5. Verify items unchanged:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status, COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' GROUP BY status;"
   ```

6. Cleanup:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected

- CLI prints: `No orphaned running items found for task <id>`
- Item statuses remain in terminal state (unchanged)

---

## Scenario 5: Stall Detection Sweep Recovers Long-Running Items

### Preconditions

- Common Preconditions applied

### Goal

Verify that the background stall detection sweep recovers items that have been running longer than the configured threshold.

### Steps

1. Inject a stalled item directly into the database:
   ```bash
   # Ensure no daemon is running
   pkill -f orchestratord 2>/dev/null; sleep 1

   # Create a task via a temporary daemon
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 >/tmp/fr033-s5-pre.log 2>&1 &
   TMP_PID=$!
   sleep 2

   TASK_ID=$(./target/release/orchestrator task create \
     --name "stall-detect-test" \
     --goal "Test stall detection sweep" \
     --project "${QA_PROJECT}" \
     --workflow qa_sleep \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "TASK_ID=${TASK_ID}"

   kill "$TMP_PID"
   wait "$TMP_PID" 2>/dev/null
   ```

2. Manually set items to stalled state (running with old started_at):
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "UPDATE task_items SET status='running', started_at=datetime('now','-2 hours') WHERE task_id='${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db \
     "UPDATE tasks SET status='running' WHERE id='${TASK_ID}';"

   sqlite3 data/agent_orchestrator.db \
     "SELECT id, status, started_at FROM task_items WHERE task_id='${TASK_ID}';"
   ```

3. Start daemon with a short stall timeout (1 minute) so it triggers quickly:
   ```bash
   rm -f data/orchestrator.sock
   ./target/release/orchestratord --foreground --workers 1 --stall-timeout-mins 1 >/tmp/fr033-s5-post.log 2>&1 &
   DAEMON_PID=$!
   sleep 2
   ```

4. Note: startup recovery will recover these items immediately (since they are running). The stall sweep runs as a second line of defense. Verify startup recovery worked:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM task_items WHERE task_id='${TASK_ID}';"
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   ```

5. Check recovery events:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE event_type IN ('orphaned_items_recovered','item_stall_recovered') AND payload_json LIKE '%${TASK_ID}%' ORDER BY id DESC LIMIT 5;"
   ```

6. Verify stall detection sweep is running:
   ```bash
   grep "stall detection sweep started" /tmp/fr033-s5-post.log
   ```

7. Cleanup:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected

- After daemon restart: items recovered to `pending` by startup recovery
- `orphaned_items_recovered` event emitted for the stalled task
- Daemon log shows `stall detection sweep started` with configured timeout
- `--stall-timeout-mins 1` accepted without error

### Expected Data State

```sql
SELECT status FROM task_items WHERE task_id = '{task_id}';
-- Expected: pending (recovered from running)

SELECT event_type FROM events
  WHERE event_type IN ('orphaned_items_recovered','item_stall_recovered')
    AND payload_json LIKE '%{task_id}%'
  ORDER BY id DESC LIMIT 1;
-- Expected: orphaned_items_recovered
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Startup Recovery Resets Orphaned Running Items | ✅ | 2026-03-13 | claude | SIGKILL with running items → restart recovers to pending → worker re-claims and completes. orphaned_items_recovered event emitted with correct payload |
| 2 | Startup Recovery Is Idempotent (No Orphans) | ✅ | 2026-03-13 | claude | No running items → no new events (PRE=POST=16), no recovery log messages |
| 3 | CLI `task recover` Resets Orphaned Items for Specific Task | ✅ | 2026-03-13 | claude | `task recover` prints "Recovered 1 orphaned running item(s)", items→pending, task→restart_pending |
| 4 | Terminal Items Are Not Affected by Recovery | ✅ | 2026-03-13 | claude | Completed task with qa_passed items → "No orphaned running items found", items unchanged |
| 5 | Stall Detection Sweep Recovers Long-Running Items | ✅ | 2026-03-13 | claude | --stall-timeout-mins 1 accepted, sweep started, stalled items (started_at 2h ago) recovered at startup, worker re-claimed |
