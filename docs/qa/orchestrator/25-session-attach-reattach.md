# Orchestrator - Real Session Attach/Re-attach

**Module**: orchestrator  
**Scope**: Validate real session lifecycle for tty-enabled steps, including session discovery, attach, re-attach, command injection, and session close behavior  
**Scenarios**: 5  
**Priority**: High

---

## Background

This document validates the session-oriented interaction flow introduced for tty-enabled workflow steps:

- Session registry commands: `task session list|info|close`
- Exec target selector: `session/<session_id>`
- Auto-routing behavior: `exec task/<task_id>/step/<step_id>` prefers active session when present
- Attach and re-attach behavior for interactive sessions

Entry point: `./scripts/orchestrator.sh`

---

## Scenario 1: CLI Surface Exposes Session Management and Session Target

### Preconditions

- CLI binary is available.

### Goal

Validate that users can discover session-oriented commands and target syntax from help output.

### Steps

1. Verify task session subcommand family:
   ```bash
   ./scripts/orchestrator.sh task --help | rg "session"
   ./scripts/orchestrator.sh task session --help
   ```
2. Verify session subcommand details:
   ```bash
   ./scripts/orchestrator.sh task session list --help
   ./scripts/orchestrator.sh task session info --help
   ./scripts/orchestrator.sh task session close --help
   ```
3. Verify `exec` target supports both selector formats:
   ```bash
   ./scripts/orchestrator.sh exec --help
   ```

### Expected

- `task --help` shows `session` command family.
- `task session --help` shows `list`, `info`, and `close`.
- `exec --help` describes target selector formats including `task/<task_id>/step/<step_id>` and `session/<session_id>`.

### Expected Data State
```sql
SELECT COUNT(*)
FROM tasks;
-- Expected: unchanged by help-only scenario
```

---

## Scenario 2: TTY Step Run Creates Active Session Record

### Preconditions

- Runtime is initialized.
- A test task can be created with a `plan` step where `tty=true`.

### Goal

Validate that executing a tty-enabled step creates a persisted active session.

### Steps

1. Create and prepare isolated task context (can reuse existing script):
   ```bash
   ./docs/qa/script/test-exec-interactive.sh --json
   ```
2. Extract `{task_id}` and `{plan_step_id}` from script output.
3. Query session records:
   ```bash
   sqlite3 data/agent_orchestrator.db "
   SELECT id, task_id, step_id, state, pid, input_fifo_path, stdout_path
   FROM agent_sessions
   WHERE task_id = '{task_id}' AND step_id = '{plan_step_id}'
   ORDER BY created_at DESC
   LIMIT 1;"
   ```

### Expected

- One latest session record exists for `{task_id}` and `{plan_step_id}`.
- `state` is `active` (or `detached` if implementation marks detached after writer disconnect).
- `pid` is greater than 0 for running session-backed process.
- `input_fifo_path` and `stdout_path` are non-empty.

### Expected Data State
```sql
SELECT COUNT(*)
FROM agent_sessions
WHERE task_id = '{task_id}'
  AND step_id = '{plan_step_id}'
  AND state IN ('active', 'detached', 'closed');
-- Expected: >= 1
```

---

## Scenario 3: Attach by Session ID and Re-attach by Task Step Target

### Preconditions

- Scenario 2 completed.
- `{session_id}` is resolvable from `agent_sessions`.

### Goal

Validate both direct session attach and task-step auto-routing to active session.

### Steps

1. Attach directly by session id:
   ```bash
   printf 'session-direct-attach\n' | ./scripts/orchestrator.sh exec -it session/{session_id} -- cat
   ```
2. Detach after output verification (Ctrl+C or process completion).
3. Re-attach via task-step target:
   ```bash
   printf 'session-reattach\n' | ./scripts/orchestrator.sh exec -it task/{task_id}/step/{plan_step_id} -- cat
   ```

### Expected

- Direct attach command returns successfully and outputs `session-direct-attach`.
- Re-attach command returns successfully and outputs `session-reattach`.
- Re-attach does not require creating a new task.

### Expected Data State
```sql
SELECT COUNT(*)
FROM session_attachments
WHERE session_id = '{session_id}'
  AND mode IN ('writer', 'reader');
-- Expected: >= 2
```

---

## Scenario 4: Non-interactive Injection to Active Session

### Preconditions

- Scenario 2 completed and session is still attachable (`active` or `detached`).

### Goal

Validate command injection path without `-it` against `session/<session_id>`.

### Steps

1. Inject a non-interactive command:
   ```bash
   ./scripts/orchestrator.sh exec session/{session_id} -- echo session-noninteractive
   ```
2. Read last output lines:
   ```bash
   tail -n 100 {stdout_path}
   ```

### Expected

- Command returns success.
- Session stdout includes `session-noninteractive`.
- Task lifecycle remains valid after injection.

### Expected Data State
```sql
SELECT state
FROM tasks
WHERE id = '{task_id}';
-- Expected: valid lifecycle state (pending/running/paused/completed/failed)
```

---

## Scenario 5: Close Session and Verify Attach Rejection

### Preconditions

- A session exists for `{session_id}`.

### Goal

Validate closure semantics and post-close attach rejection behavior.

### Steps

1. Close session gracefully:
   ```bash
   ./scripts/orchestrator.sh task session close {session_id}
   ```
2. Validate state changed:
   ```bash
   ./scripts/orchestrator.sh task session info {session_id} -o json
   ```
3. Try to attach closed session:
   ```bash
   ./scripts/orchestrator.sh exec -it session/{session_id} -- cat
   ```

### Expected

- Close command succeeds.
- `task session info` shows state `closed`.
- Attach to closed session fails with non-zero exit and explicit not-attachable/closed message.

### Expected Data State
```sql
SELECT state, ended_at
FROM agent_sessions
WHERE id = '{session_id}';
-- Expected: state='closed' AND ended_at IS NOT NULL
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | CLI Surface Exposes Session Management and Session Target | ☐ | | | |
| 2 | TTY Step Run Creates Active Session Record | ☐ | | | |
| 3 | Attach by Session ID and Re-attach by Task Step Target | ☐ | | | |
| 4 | Non-interactive Injection to Active Session | ☐ | | | |
| 5 | Close Session and Verify Attach Rejection | ☐ | | | |
