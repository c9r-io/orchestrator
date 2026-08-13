---
lifecycle: active
related_fr: FR-169
self_referential_safe: true
---

# Orchestrator - Daemon Self-Termination When Its Data Directory Vanishes

**Module**: Daemon lifecycle
**Scope**: that a daemon whose data directory is removed exits instead of running
forever unreachable; that delete-and-recreate is caught too, which a path check
cannot see; that a healthy daemon is never killed by the watcher; that the
hysteresis reset is load-bearing; and that the exit goes through the existing
shutdown sequence and names its cause
**Scenarios**: 4
**Priority**: High

## Background

A daemon whose data directory was removed underneath it used to run indefinitely:
its socket went with the directory so nothing could reach it, it held the database
open on an unlinked inode, and it wrote **zero bytes of log**. Two such processes
ran for 22h34m in the field. Worse, delete-and-recreate left the old daemon alive
holding 7 open fds on an orphaned database while a **second daemon started on the
same path** — a path-existence check reads healthy in exactly that case.

FR-169 makes the daemon exit within ~15s (5s period × 3 confirmations), through the
shutdown sequence it already had, naming the cause. See
[DD-185](../../design_doc/orchestrator/185-daemon-data-dir-vanish-self-termination.md).

Everything here runs against throwaway directories under `$TMPDIR`. No scenario
touches `~/.orchestratord`; the two shell probes assert their own isolation and
abort if `ORCHESTRATORD_DATA_DIR` is not inside their scratch tree.

Primary entry points:

```bash
cargo test -p orchestratord --bins lifecycle::tests          # 13 tests
bash <scratch>/datadir-vanish-probe.sh                       # removal, end to end
bash <scratch>/recreate-probe.sh                             # delete-and-recreate
```

---

## Scenario 1: A removed data directory ends the daemon, and it says so

**Steps**

```bash
cargo test -p orchestratord --bins lifecycle::tests::data_dir_identity_is_none_once_removed
bash <scratch>/datadir-vanish-probe.sh
```

The probe starts an isolated daemon through `scripts/lib/gate_daemon.sh`, waits for
readiness, removes the data directory, then polls liveness at t+2/5/10/20/30s.

**Expected result**

The daemon is ALIVE at t+2/5/10 and EXITED by t+20 — detection is ~15s, so the
transition must fall inside that window rather than at either end. The log carries
one `ERROR` naming the mechanism:

```
data directory is gone; this daemon can no longer serve anyone and is shutting down
  data_dir=… expected_dev=… expected_ino=… observed=None confirmations=3
```

The assertion is that line and the exit, **not** an exit code — an exit code cannot
distinguish this mechanism from any other failure. Before FR-169 this scenario
produced zero bytes of log and no exit at all, which is the measurement the fixture
replaced.

---

## Scenario 2: Delete-and-recreate is caught, which a path check cannot do

**Steps**

```bash
cargo test -p orchestratord --bins lifecycle::tests::data_dir_identity_changes_when_the_path_is_recreated
cargo test -p orchestratord --bins lifecycle::tests::a_replaced_directory_trips_it_like_a_removed_one
bash <scratch>/recreate-probe.sh
```

The probe removes the data directory and immediately recreates the same path, then
polls the old daemon.

**Expected result**

`path exists now: YES` and `identity changed: YES` on the same fixture — the two
readings disagree, and that disagreement is the point. The old daemon EXITED by
t+13s, and its open database fds go from **7 to 0**.

**This is the scenario that separates the implementation from the obvious one.**
Replacing `data_dir_identity` with `if path.exists()` leaves Scenarios 1, 3 and 4
green and fails only this one — verified, see Mutation Evidence. Without it,
nothing in this document would notice a daemon writing an orphaned inode while a
second daemon owns its name.

---

## Scenario 3: A healthy daemon is never killed, and the hysteresis is real

**Steps**

```bash
cargo test -p orchestratord --bins lifecycle::tests::an_untouched_data_dir_never_reaches_the_threshold
cargo test -p orchestratord --bins lifecycle::tests::a_recovery_between_failures_prevents_the_trip
cargo test -p orchestratord --bins lifecycle::tests::three_consecutive_vanishes_trip_it
```

**Expected result**

All pass. The first drives 100 observations of a live directory and requires the
counter to stay at zero — a measure meant to end processes must be shown not to end
healthy ones. The second applies **four** vanished observations across a recovery
against a threshold of three and requires **no** trip, because they were not
consecutive. The third confirms three consecutive observations do trip it, so the
second is not passing by never firing at all.

The logic is a pure function (`observe_data_dir`) rather than the watcher loop, so
none of this waits on wall-clock time. A hysteresis test written with `sleep` goes
flaky and then gets deleted.

---

## Scenario 4: The exit reuses the existing shutdown sequence and names its cause

**Steps**

```bash
bash <scratch>/datadir-vanish-probe.sh    # inspect the tail of the daemon log
cargo test -p orchestratord --bins        # the shutdown-path tests must not regress
```

**Expected result**

The daemon leaves through the same path as an RPC shutdown — the watcher calls
`shutdown_notify.notify_waiters()` and adds no second exit route — and
`shutdown_reason` reports `data_dir_vanished` rather than `shutdown` or
`external_stop_signal`.

That arm is **first** in the chain deliberately: once the directory is gone,
`worker_stop_signal_path(state).exists()` is false because that path lives inside
it, and `shutdown_requested` is true because the watcher set it, so both later arms
would name the wrong cause. Moving the arm below them is a silent misattribution,
not a failure.

The session-drain error on this path is `warn`, not `error`: the database went with
the directory, so it is guaranteed here and carries nothing the watcher's own line
did not. It is not silenced — "the drain did not run" is still a fact.

---

## Mutation Evidence

Each mutation was applied and run; each names the diagnostic it produced.

| Mutation | Caught by | Diagnostic |
|---|---|---|
| `data_dir_identity` returns `Some((0,0))` when the path exists — i.e. presence instead of identity | Scenario 2 only | `delete-and-recreate produced the same identity, so the watcher would not notice the daemon is writing an orphaned inode` — left `(0,0)`, right `(0,0)` |
| delete `*confirmations = 0` from `observe_data_dir` | Scenario 3, second case only | `a successful stat did not reset the counter` — left 2 |

Both mutations are ones an author would plausibly make: the first is the simpler
implementation anybody would reach for, and the second looks like a redundant line.
Neither is a deletion of the whole feature, which is the mutation that proves
least. In both cases the other scenarios stayed green, which is the evidence that
each fixture is aimed at something specific rather than duplicating its neighbours.

## Checklist

- [ ] Scenario 1: the daemon exits after its data dir is removed, within ~15s, and
      logs one ERROR naming path, expected/observed inode, and confirmations
- [ ] Scenario 2: `path exists = YES` while `identity changed = YES`, the old
      daemon exits, and its open DB fds fall from 7 to 0
- [ ] Scenario 2: a path-existence implementation fails this and only this
- [ ] Scenario 3: 100 healthy observations never accumulate a confirmation
- [ ] Scenario 3: four vanished observations across a recovery do not trip a
      threshold of three, while three consecutive ones do
- [ ] Scenario 4: `shutdown_reason` reports `data_dir_vanished`, and the exit adds
      no second shutdown route
- [ ] Scenario 4: the session-drain failure on this path is `warn`, not `error`
