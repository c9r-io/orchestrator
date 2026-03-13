# Design Doc 44: Self-Referential Daemon PID Guard

**FR**: FR-034
**Status**: Implemented
**Date**: 2026-03-13

## Problem

When the orchestrator runs self-bootstrap tasks (self-referential workspace), QA testing agents can execute commands like `kill $(cat data/daemon.pid)` that terminate the daemon hosting the task. This causes cascading failures: all running items abort, false tickets are generated, and manual recovery is required.

## Solution: Runner-Level PID Guard

### Data Flow

```
task_ctx.self_referential
  → PhaseRunRequest.self_referential
    → setup_phase_execution injects ORCHESTRATOR_DAEMON_PID into extra_env
      → spawn.rs detects env var, calls guard_daemon_pid_kill(command, pid)
        → DaemonPidGuardBlocked error
          → run_phase_with_timeout returns RunResult(exit=0, skipped)
            → daemon_pid_kill_blocked event recorded
```

### Components

1. **Env Var Injection** (`phase_runner/setup.rs`): When `self_referential == true`, `ORCHESTRATOR_DAEMON_PID` is injected into the runner environment with the current process ID.

2. **Command Guard** (`runner/policy.rs`): `guard_daemon_pid_kill(command, daemon_pid)` detects kill commands targeting the daemon via 5 patterns:
   - `kill` + `daemon.pid` file reference
   - `kill` + literal daemon PID number
   - `kill` + `$ORCHESTRATOR_DAEMON_PID` env var
   - `pkill orchestratord`
   - `killall orchestratord`

3. **Spawn Integration** (`runner/spawn.rs`): After `enforce_runner_policy()`, checks for `ORCHESTRATOR_DAEMON_PID` in `extra_env` and calls the guard.

4. **Graceful Handling** (`phase_runner/mod.rs`): `handle_daemon_pid_guard_blocked()` catches the error, records a `daemon_pid_kill_blocked` event, and returns `RunResult { exit_code: 0, validation_status: "skipped" }`.

5. **QA Doc Annotation** (`53-client-server-architecture.md`): Marked with `self_referential_safe: false` frontmatter for prehook-level filtering.

### Design Decisions

- **Guard location**: Runner policy layer (pre-spawn) rather than kernel-level sandbox, balancing detection coverage with implementation simplicity.
- **Exit code 0**: Blocked steps return success/skipped to avoid false failure tickets.
- **Env var as signal**: The presence of `ORCHESTRATOR_DAEMON_PID` in `extra_env` signals self-referential mode, avoiding additional parameters to `spawn()`.
- **Pattern matching**: Heuristic-based (not regex) for performance and simplicity. Covers realistic attack surface; deliberate obfuscation is out of scope.

### Limitations

- Cannot detect obfuscated kill commands (e.g., `eval $(echo ... | base64 -d)`). Full process-level protection would require kernel-level sandboxing (future work).
