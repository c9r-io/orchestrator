# Design Doc 57: Long-Lived Command Guard

## Problem

QA agent executing streaming CLI commands (`task watch`, `task follow`) via Bash
tool can stall indefinitely, blocking the entire pipeline for 30+ minutes. The
root cause is that these commands produce an infinite stream and never exit unless
the task reaches a terminal state.

## Solution

Three complementary mechanisms prevent pipeline stalls from long-lived commands:

### 1. `task watch --timeout <seconds>`

Added a `--timeout` CLI parameter to `orchestrator task watch`. When set to a
non-zero value, the watch loop exits with code 0 after the specified duration,
printing a final status snapshot.

**Files:**
- `proto/orchestrator.proto` — `timeout_secs` field on `TaskWatchRequest`
- `crates/cli/src/cli.rs` — `--timeout` arg (default 0 = no timeout)
- `crates/cli/src/commands/task.rs` — client-side deadline on stream reads
- `crates/daemon/src/server/task.rs` — server-side deadline on watch loop
- `core/src/scheduler/query/watch.rs` — `timeout_secs` param on `watch_task()`

### 2. Stall Auto-Termination

Extended the existing heartbeat monitoring in `phase_runner/wait.rs` to
automatically kill steps that exhibit prolonged `low_output` stagnation.

- **Default threshold**: 30 consecutive stagnant heartbeats (30 x 30s = 900s)
- **Action**: Kill process group, insert `step_stall_killed` event, exit code -7
- **Constant**: `STALL_AUTO_KILL_CONSECUTIVE_HEARTBEATS` in `phase_runner/types.rs`
- **Configurable**: `stall_timeout_secs` at workflow `safety` level or per-step

This builds on the existing `low_output` detection (3 consecutive heartbeats
with <= 32 bytes delta after 90s elapsed) but adds enforcement rather than just
observation.

#### Per-step override

The stall threshold can be overridden globally or per step to accommodate
long-running operations like full compilation that produce low output:

```yaml
# Global: workflow-level safety setting
safety:
  stall_timeout_secs: 1800   # 30 minutes

# Per-step: overrides global for this step only
steps:
  - id: qa_testing
    type: qa_testing
    stall_timeout_secs: 2400  # 40 minutes for compilation-heavy QA
```

When `stall_timeout_secs` is set, it is converted to heartbeat count
(`secs / 30`, minimum 1) and used in place of the built-in 900s default.
Per-step values take priority over global `safety.stall_timeout_secs`.

### 3. QA Agent Timeout Guidance

The `qa_testing` step template prompt in `self-bootstrap.yaml` now instructs the
QA agent to always use `--timeout` or shell `timeout` when executing streaming
commands.

## Trade-offs

- The 900s stall threshold is conservative to avoid false positives from
  legitimate slow operations (e.g., large compilation). The `stall_timeout_secs`
  field allows per-step or global overrides for workflows that need longer.
- `--timeout 0` (default) preserves backward compatibility for interactive use.
