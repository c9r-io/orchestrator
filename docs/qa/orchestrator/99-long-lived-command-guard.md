---
self_referential_safe: false
self_referential_safe_scenarios: [S5]
---

# QA: Long-Lived Command Guard

Verifies FR-045: `task watch --timeout`, stall auto-termination, and QA agent timeout guidance.

---

## Scenario 1: task watch --timeout exits after deadline

### Steps

1. Ensure the daemon is running:
   ```bash
   orchestratord --foreground --workers 1 &
   ```
2. Create a task (do not start it, so it stays in `pending`):
   ```bash
   TASK_ID=$(orchestrator task create --workspace test-workspace --project default --goal "timeout test" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```
3. Watch with a 5-second timeout:
   ```bash
   orchestrator task watch "$TASK_ID" --interval 1 --timeout 5
   ```

### Expected

- The watch command exits after ~5 seconds with exit code 0.
- Stderr contains `watch: timeout after 5s`.
- A final status snapshot is printed before exit.

---

## Scenario 2: task watch without --timeout runs indefinitely until terminal

### Steps

1. With a running daemon and a task in `running` state:
   ```bash
   orchestrator task watch "$TASK_ID" --interval 2
   ```
2. Wait for the task to complete naturally.

### Expected

- The watch command runs continuously until the task reaches `completed` or `failed`.
- No timeout message is printed.

---

## Scenario 3: stall auto-termination kills stagnant step

### Steps

1. Create a step that produces no output (e.g., `sleep 99999`) in a workflow.
2. Monitor the daemon logs for heartbeat events.
3. Wait for 30 consecutive stagnant heartbeats (~15 minutes).

### Expected

- After 30 stagnant heartbeats, the daemon kills the process group.
- A `step_stall_killed` event is recorded with:
  - `stagnant_heartbeats >= 30`
  - `pid` of the killed process
- The step finishes with exit code -7.
- Subsequent steps in the pipeline continue executing.

---

## Scenario 4: stall_timeout_secs overrides default threshold

### Steps

1. Create a workflow YAML with a step that sets `stall_timeout_secs`:
   ```yaml
   safety:
     stall_timeout_secs: 120   # global: 2 minutes
   steps:
     - id: slow_step
       type: qa_testing
       stall_timeout_secs: 180  # per-step override: 3 minutes
   ```
2. Apply the manifest and start a task that exercises the `slow_step`.
3. In a separate session, create a step that produces no output (`sleep 99999`)
   using a workflow where `stall_timeout_secs: 90`.
4. Monitor heartbeat events.

### Expected

- The per-step `stall_timeout_secs` (180s = 6 heartbeats) takes priority over
  global `safety.stall_timeout_secs` (120s = 4 heartbeats).
- If neither is set, the built-in default (900s = 30 heartbeats) applies.
- The `step_stall_killed` event fires after the expected heartbeat count.

### Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| Step killed after 900s despite `stall_timeout_secs` set | Field not reaching phase runner — check manifest apply | Verify with `orchestrator task show` that safety config is persisted |
| Step killed too early | `stall_timeout_secs` too low for the workload | Increase value; minimum effective is 30s (1 heartbeat) |

---

## Scenario 5: qa_testing template includes timeout guidance

### Steps

1. Verify the timeout guidance exists in the self-bootstrap workflow fixture:
   ```bash
   rg -n "timeout" fixtures/workflow/self-bootstrap.yaml
   ```
2. Code review — confirm the `qa_testing` step template in `fixtures/workflow/self-bootstrap.yaml` (lines 165-167):
   - Template prompt contains guidance about using `--timeout` where available
   - Template prompt wraps with shell `timeout` command to prevent indefinite blocking
   - Example command uses `--timeout` flag: `orchestrator task watch <task_id> --interval 1 --timeout 30`

### Expected

- `rg` output shows 3+ matches for `timeout` in the fixture file
- The template prompt contains guidance about using `--timeout` or `timeout` wrapper for streaming commands
- Example command in prompt uses `--timeout` flag

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
