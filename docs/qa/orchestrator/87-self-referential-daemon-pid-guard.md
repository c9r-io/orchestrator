---
self_referential_safe: false
---
# Self-Referential Daemon PID Guard

**Module**: orchestrator
**Scope**: Verify daemon PID kill guard prevents self-referential workspace from killing its own daemon
**Scenarios**: 4

---

## Scenario 1: Guard blocks `kill $(cat data/daemon.pid)` in self-referential mode

**Precondition**: Workspace is self-referential (`self_referential: true`)

### Steps

1. Submit a task with a step whose command contains `kill $(cat data/daemon.pid)`
2. Observe that the step returns exit_code=0, validation_status="skipped"
3. Verify a `daemon_pid_kill_blocked` event is recorded in the events table
4. Verify the daemon process is still alive

### Expected

- Step completes with exit 0 (not failure)
- No false failure tickets generated
- Event payload contains `matched_pattern`, `reason`, `daemon_pid`

---

## Scenario 2: Guard allows daemon lifecycle tests in non-self-referential mode

**Precondition**: Workspace is NOT self-referential (`self_referential: false`)

### Steps

1. Submit a task whose step command contains `kill $(cat data/daemon.pid)`
2. Observe that the command executes normally (not intercepted)

### Expected

- Command runs without guard intervention
- `ORCHESTRATOR_DAEMON_PID` env var is NOT present in the process environment

---

## Scenario 3: Guard blocks pkill/killall targeting orchestratord

**Precondition**: Workspace is self-referential

### Steps

1. Submit a task with `pkill orchestratord` command
2. Submit a task with `killall orchestratord` command
3. Verify both are blocked with `daemon_pid_kill_blocked` events

### Expected

- Both commands blocked, exit 0, skipped
- Events recorded with appropriate `matched_pattern`

---

## Scenario 4: Guard allows kill commands targeting other processes

**Precondition**: Workspace is self-referential

### Steps

1. Submit a task with `kill 99999` (non-daemon PID)
2. Submit a task with `pkill some-other-process`

### Expected

- Commands proceed normally (not blocked by guard)
- No `daemon_pid_kill_blocked` events

---

## Unit Test Coverage

- `runner::policy::tests::blocks_kill_cat_daemon_pid`
- `runner::policy::tests::blocks_kill_literal_daemon_pid`
- `runner::policy::tests::blocks_kill_env_var`
- `runner::policy::tests::blocks_kill_env_var_braced`
- `runner::policy::tests::blocks_pkill_orchestratord`
- `runner::policy::tests::blocks_killall_orchestratord`
- `runner::policy::tests::blocks_kill_signal_then_pid`
- `runner::policy::tests::blocks_kill_pid_in_compound_command`
- `runner::policy::tests::allows_kill_different_pid`
- `runner::policy::tests::allows_normal_command`
- `runner::policy::tests::allows_kill_word_in_echo`
- `runner::policy::tests::does_not_false_positive_on_pid_substring`
- `runner::policy::tests::does_not_false_positive_on_pid_prefix`

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
