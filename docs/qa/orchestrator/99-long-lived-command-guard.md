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

## Scenario 4: qa_testing template includes timeout guidance

### Steps

1. Read `fixtures/workflow/self-bootstrap.yaml`.
2. Find the `qa_testing` step template.

### Expected

- The template prompt contains guidance about using `--timeout` or `timeout` wrapper for streaming commands.
- Example command in prompt uses `--timeout` flag.

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
