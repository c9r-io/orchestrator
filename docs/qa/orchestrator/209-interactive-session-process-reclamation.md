---
lifecycle: active
related_fr: FR-159
self_referential_safe: true
---

# Orchestrator - Interactive Session Process Reclamation

**Module**: Orchestrator Daemon / Session Store / Scheduler Spawn / QA Harness
**Scope**: OS-level reclamation of unreachable session process groups, stale-record
retention, session stdin transport, temp-directory reclamation
**Scenarios**: 5
**Priority**: High

## Background

Interactive sessions were reclaimed by nothing. `reconcile_sessions` changed
database rows and never signalled the OS, `shutdown_running_tasks` cannot reach a
session at all because a tty child is never stored in `runtime.child`, and the
only reaper left was tokio's `kill_on_drop` — which signals a single PID rather
than the process group, decapitating the leader and orphaning its descendants.

Measured on the development machine 2026-08-02: 28 `session-control-mock`
processes alive up to 19 days, 23 reparented to `init`, 6 orphan `orchestratord`
still listening on 19394–19399, 133.7 hours of accumulated CPU.

This document verifies the reclamation path, the negative fixtures that keep its
assertions honest, and the temp-directory leaks found alongside it.

## Automated script

`scripts/qa/test-session-process-reclamation.sh` covers scenarios 1–4. It starts
its own daemon over UDS with a private data directory, never touches
`~/.orchestratord`, and sweeps its own fixture leftovers at startup.

```bash
bash scripts/qa/test-session-process-reclamation.sh
```

Expected: `Session process reclamation QA: 13 passed, 0 failed`, exit 0.

## Scenario 1 — An unreachable session process is reclaimed

The trigger is **transport gone**, not daemon death. See "Why not parentage"
below; this distinction is the substance of the scenario.

### Steps

1. Apply a `RuntimePolicy` with `session_reclaim_enabled: true`.
2. Start a task on the `session-control-mock` bundle and wait for its session row
   to carry a non-zero pid.
3. Confirm the process is running. (Without this the rest is vacuous — "the
   process is gone" is true of a process that never started.)
4. Delete the session's `input_fifo_path`.
5. Wait up to 20s for the process to exit.

### Expected result

- The session process is gone within two reconciliation cycles.
- A `session_process_reclaimed` event exists for the session, with
  `outcome == "reclaimed"`.
- The session's own `logs/sessions/<session_id>/` directory is removed.
- `data/agent_orchestrator.db` and the rest of the data directory are untouched.

## Scenario 2 (negative) — Reclamation disabled leaves the process alone

Without this scenario, scenario 1 certifies the machine's state rather than the
feature: on a machine with no orphans, "the process is gone" passes with nothing
implemented.

### Steps

1. Apply a `RuntimePolicy` with `session_reclaim_enabled: false`.
2. Start a second mock session; delete its FIFO.
3. Wait 25s — more than two 10s reconciliation cycles.

### Expected result

- The process is **still running**.
- The row still moved to `failed`: only the signal is gated, not reconciliation.

## Scenario 3 (negative) — A mismatched fingerprint signals nothing

### Steps

1. With `session_reclaim_enabled: true`, start a third mock session.
2. `UPDATE agent_sessions SET process_fingerprint='deliberately-wrong'`.
3. Delete its FIFO; wait 25s.

### Expected result

- The process is still running. Asserting only that an error was returned would
  pass equally on an implementation that signalled first and reported second, so
  the assertion is on the process, not the return value.

## Scenario 4 — Graceful shutdown drains a healthy session, leaving nothing behind

Requirement 4. The session here has an intact transport, so the periodic path
would never touch it; if it dies, the shutdown drain reclaimed it.

Scenarios 2 and 3 exist to prove processes *survive*, so they are still alive by
design and are reclaimed at the end of this scenario before the final count.
Anything left after that was leaked rather than spared.

### Steps

1. Start a mock session and confirm it is running.
2. Stop the daemon with `SIGTERM`; poll until the PID file is released.
3. Wait up to 15s for the session process to exit.
4. Kill the process groups spared by scenarios 2 and 3.
5. Count `ppid == 1` processes whose command line carries the mock marker.

### Expected result

- The drained session's process group is gone. `SIGTERM` alone suffices — every
  group in the 2026-08-03 triage exited without needing `SIGKILL`.
- The final count is zero, and it **fails closed on an empty process table**: a
  `ps` that returned nothing and a machine running nothing are the same zero
  rows, and every count derived from them reads as clean.

## Scenario 5 — The QA harness does not accumulate orphans across interrupted runs

Requirement 6, verified by hand. `test-agent-session-control-plane.sh` now records
every real session PID as it appears, reclaims by process group, and sweeps the
previous run's leftovers at startup.

### Steps

1. Start `scripts/qa/test-agent-session-control-plane.sh` in its own process
   group, wait until a real mock session is observable, then `SIGKILL` the run.
2. Repeat once.
3. Run the script to completion.

Keep the marker used to detect the session out of the invoking command line — a
`ps | grep` from an interactive shell matches that shell's own command and
reports the fixture up before it exists, which kills the run early and produces a
vacuous zero.

### Expected result

| Round | Action | Orphans after |
|---|---|---|
| 1 | SIGKILL mid-run | 1 |
| 2 | SIGKILL mid-run | 1 — *not 2*; the startup sweep prevents accumulation |
| 3 | clean run, 6 passed | 0 |

Recorded 2026-08-03: exactly these values.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Unreachable session process reclaimed, recorded, directory removed | PASS | 2026-08-03 | Claude | Process gone within two cycles; `session_process_reclaimed` with `outcome=reclaimed`; `logs/sessions/<id>/` removed; database untouched |
| 2 | Reclamation disabled leaves the process alone | PASS | 2026-08-03 | Claude | Survives >25s (two 10s cycles); row still moves to `failed`, so only the signal is gated |
| 3 | Mismatched fingerprint signals nothing | PASS | 2026-08-03 | Claude | Process still running; assertion is on the process, not the return value |
| 4 | Graceful shutdown drains a healthy session, nothing left behind | PASS | 2026-08-03 | Claude | Drained on `SIGTERM` alone; final orphan count 0, fails closed on an empty `ps` |
| 5 | QA harness does not accumulate across interrupted runs | PASS | 2026-08-03 | Claude | SIGKILL rounds gave 1, then 1 (not 2), then 0 after a clean run |

Gate output 2026-08-03: `Session process reclamation QA: 13 passed, 0 failed`,
exit 0, and again on an immediate second run with nothing to sweep.

## Certification run

Recorded 2026-08-03 at `3d8afb2c`, worktree clean and revision identical before
and after.

| Check | Result |
|---|---|
| `cargo test --workspace` | 2859 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean (exit status captured directly, not through a pipe) |
| `scripts/qa/test-session-process-reclamation.sh` | 13 passed, 0 failed |
| `scripts/qa/test-agent-session-control-plane.sh` | 6 passed, 0 failed |
| Derived ci-required sweep | **51 passed, 1 failed, 0 missing of 52** |

The gate list was derived, not typed:

```bash
jq -r '.scripts[] | select(.enforcement == "ci-required") | .path' \
  config/governance/qa-gate-surface.json
```

`certify-slack-managed-live.sh` is invoked as `status`, its read-only subcommand,
per the manifest note — run bare it prints usage and exits 2, which would raise a
failure that is not there.

The single failure is `scripts/qa/ci-liveness.rb`, and it is **not caused by this
work**. Its job records were taken at `45fbf3c4`; `.github/workflows/ci.yml` last
changed at `ceccf4f5`; `45fbf3c4` is an ancestor of `ceccf4f5`, and `ceccf4f5` is
an ancestor of this FR's base `ae42b87b`. The failing condition therefore already
held before the first FR-159 commit, and no commit here touches `ci.yml` or the
liveness record. Refreshing it requires a real CI run.

## Unit coverage

`cargo test -p orchestrator-persistence --lib session_store`

- `reclaim_kills_the_whole_group_not_just_the_leader` — asserts the session's
  **child** dies. A leader-only signal is invisible in the leader's exit status,
  so a test that only checked the leader would pass on the defect.
- `reclaim_refuses_and_sends_nothing_when_the_fingerprint_mismatches` — asserts
  the process is still running afterwards.
- `reclaim_refuses_a_pid_that_does_not_lead_its_group` — the fixture is live and
  fingerprint-verifiable, so leadership is the only precondition that fails.
- `reclaim_refuses_a_process_that_has_already_exited`
- `session_owned_dir_refuses_every_path_it_cannot_prove_it_owns` — one accepted
  layout plus four rejections, each relaxing exactly one condition. The accepted
  case matters: a rule that always returned `None` would be perfectly safe and
  would clean nothing.
- `cleanup_stale_sessions_retains_records_of_live_processes` — two rows identical
  in state, age and shape, differing only in whether their PID answers.

`cargo test -p orchestrator-scheduler --lib phase_runner::spawn`

- `session_stdin_survives_repeated_writer_open_close_cycles` — drives a real
  shell through three separate writer open/close cycles, the same pattern
  `send-input` uses.
- `read_only_redirect_loses_the_session_after_the_first_message` — the negative
  fixture, pinning the defect `0<>` fixes.

### Mutations run

| Mutation | Gate that failed | Diagnostic |
|---|---|---|
| `kill(pid)` instead of `kill(-pid)` | group reclamation | grandchild survived |
| drop the `getpgid` leadership check | non-leader refusal | `NotGroupLeader` not returned |
| `< fifo` instead of `0<> fifo` | stdin survival | `left: 1, right: 3` |
| delete every stale candidate regardless of liveness | live retention | `left: 2, right: 1` |

## Temp-directory reclamation

Run any bounded test suite under a **private** `TMPDIR` and count before and
after. Measuring against the shared `$TMPDIR` races with everything else on the
machine.

```bash
PRIV=$(mktemp -d)
TMPDIR="$PRIV" cargo test --workspace > run.log 2>&1; echo "exit=$?"
grep -E "^test result:" run.log | awk '{p+=$4; f+=$6} END {print "passed="p" failed="f}'
find "$PRIV" -maxdepth 1 | sed "s#^$PRIV##" | grep -v '^$' | wc -l
```

### Expected result

- The directory is **empty**.
- The test count is part of the evidence: a run that never started also leaves it
  empty, so the delta alone cannot distinguish success from a build failure.

Recorded 2026-08-03: 2851 passed, 0 failed, zero residue.

### Producers repaired

| Shape | Accumulated | Producer |
|---|---|---|
| `agent-orchestrator-test-*` | 10843 | `core/src/db_write.rs` `mem::forget` |
| `config-load-test-*` | 4021 | `core/src/config_load/mod.rs` `make_test_db` |
| `test-guard-*.db-wal`/`-shm` | 2882 | `core/src/config_load/build.rs` ×6 |
| `db-test-*` | 2859 | `core/src/db.rs` `tmp_db_path` |
| `item-exec-test-*` | 267 | `item_executor/tests.rs` `temp_dir` |
| `orch-streaming-*` | 39 | dead producer; residue only |

Four of the six were invisible to FR-159's own inventory, which enumerated
directories holding `agent_orchestrator.db`. Re-derive by grouping every
top-level `$TMPDIR` entry by prefix shape instead — a marker scoped to one
filename guards exactly what its author knew.

## Why not parentage

The sharper orphan signal would be "this process is not a child of the running
daemon". It is not used, and the reason is worth recording because FR-159's first
acceptance criterion asked for exactly that.

DD-112 §54 makes *verified live process plus transport* converge to `detached` by
design; §35 records keeping an orchestrator-owned child alive across daemon
teardown as an intended property; QA 149 scenario 5 asserts the session stays
attachable after a restart. That is deliberate and it works: the FIFO is a file
on disk and the output capture is a file too, so a new daemon really can drive a
session that outlived its parent.

Implementing the parentage rule deleted that feature — the restart scenario in
`test-agent-session-control-plane.sh` failed with `session is not attachable`,
because reconciliation marked a session that should have been `detached` as
`failed` and killed it.

The transport rule is kept, and it matches the leak actually observed: those temp
directories had been removed, so the FIFOs were gone with them. The residual case
— daemon `SIGKILL`ed with the data directory intact — is the resumable session
DD-112 intends to keep, and is covered on the other entrances by the shutdown
drain (scenario 4) and the QA harness sweep (scenario 6).

## Known limits

- `cleanup_stale_sessions` has **no production call site**. The live-record
  retention in scenario-adjacent unit coverage guards a public API that nothing
  currently invokes; the amnesia path FR-159 describes is real in the code and
  has never run.
- A session orphaned by `SIGKILL` with its data directory intact is not reclaimed
  by the daemon, by design (see "Why not parentage").
- The temp-directory sites that clean up via a trailing `remove_dir_all` rather
  than an owned guard still leak when their test panics. The six that accumulated
  measurably are repaired; the panic path is not swept.
