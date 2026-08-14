---
lifecycle: active
related_fr: FR-169
---

# DD-185: A Daemon That Cannot Serve Anyone Stops Being a Process

**Status**: Released
**QA**: [223](../../qa/orchestrator/223-daemon-data-dir-vanish-self-termination.md)
**Machine-side half of**: [DD-174](174-qa-harness-daemon-teardown.md)'s recorded residual

## The problem

Two `orchestratord` processes leaked from a probe script on 2026-08-12 and ran for
22h34m. The leak's cause is closed elsewhere (DD-174, plus a `CLAUDE.md` rule). This
document is about what the daemon did for those 22 hours.

Measured in an isolated data directory before anything was written:

| Observation | Result |
|---|---|
| daemon alive after its data dir was removed | **yes**, at t+2/5/10/20/30s |
| bytes of log written after the removal | **0** (frozen at 413) |
| CLI reachability | **cannot connect** |
| exit | never |

Its socket went with the directory, so no client could reach it; it held the
database open on an unlinked inode. An unreachable process, alive indefinitely,
saying nothing. The 22-hour instance was not an outlier — nothing existed that
would ever stop it.

### The half that raised the severity

Governance then measured delete-**and-recreate**, which the FR had not claimed:

| Observation | Result |
|---|---|
| path exists afterwards | **yes** — a path check reads healthy |
| `(dev, ino)` | **changed** |
| old daemon | **alive**, 7 open fds on the unlinked database |
| a second daemon on the same path | **starts and becomes ready** |

Two daemons, one path, one of them invisible and holding a live database on an
orphaned inode. That is a data-integrity risk rather than housekeeping, and the FR
was re-filed P1 on it.

The mechanism that admits the second daemon is recorded here because it is *not*
fixed by this FR: the singleton guard `detect_running_daemon`
(`crates/daemon/src/main.rs`) reads the pidfile **inside the data directory**, so
deleting that directory destroys the guard's only evidence. Where singleton state
should live when the state directory can vanish is a separate design question with
its own blast radius — see Known limits.

## The design

### Identity, not presence

`lifecycle::data_dir_identity` returns `(st_dev, st_ino)`; `data_dir_vanished`
is true when the current identity is absent **or** different.

A path check answers a different question than the one being asked. It catches
deletion and reports delete-and-recreate as healthy — the worse of the two, since
that is the case where a second daemon takes the name while the first keeps
writing. The distinction is asserted by a unit test that recreates the path and
requires the identity to differ; swapping the implementation for `path.exists()`
fails that test and only that test.

### One shutdown sequence, not two

The watcher does not exit the process. It calls `shutdown_notify.notify_waiters()`
— the same handle the RPC shutdown uses — so this adds a *trigger* on the existing
sequence rather than a second sequence that must be kept in agreement with it. The
same rule DD-150 and FR-168 applied to task deletion, applied to shutdown.

### Hysteresis that is real

`observe_data_dir` folds one observation into a counter and resets it on any
match. The reset is the whole of the hysteresis: without it a daemon seeing one
failed `stat` per hour eventually accumulates three and exits for no reason.

It is a separate function from the watcher loop so the property can be asserted
without wall-clock time. A test that proved this by sleeping would eventually go
flaky and be deleted. The fixture applies four vanished observations *across a
recovery* against a threshold of three and requires no trip; deleting the reset
line fails it, and no other test in the file notices.

### 5 seconds, 3 confirmations, no parameters

Detection lands within ~15s. Both are constants, and the FR's own request for
configurable flags was refused during governance: the sole argument for them was
that some filesystem might need different values, which is a guess until a real
case exists, and a parameter is permanent surface while a constant can become one
at any time. **Concept budget: zero.**

The numbers are chosen by argument, and this is stated rather than dressed up.
The FR asked for a measured distribution citing §4.4 shape 11, and that citation
does not transfer: shape 11's premise is a population that must *separate*, whereas
this is a retry count for a safety margin. On a local volume `stat` of a live
directory does not fail transiently, so the population is degenerate and demanding
a distribution would have manufactured one. Three tolerates two transient failures;
15s is far below any human or supervisor reaction time and long enough that a
paused VM or one slow network round-trip does not end a healthy daemon.

### What it says

`shutdown_reason` gains a `data_dir_vanished` arm, placed **first**. Order matters:
once the directory is gone, `worker_stop_signal_path(state).exists()` is false
because that path lives inside it, and `shutdown_requested` is true because the
watcher set it — so both later arms would name the wrong cause. The watcher also
logs one `error!` carrying the path, the expected and observed `(dev, ino)`, and
the confirmation count. Before this, the event produced no output at all.

## Implementation note found by running it

The first end-to-end run exited correctly and produced a **new** `error!`:
`failed to enumerate interactive sessions for shutdown drain: failed to open
sqlite db`. It is guaranteed on this path — the database went with the directory —
and carries no information the watcher's own line did not already give. It is now
`warn!` when the vanish flag is set, and left at `error!` otherwise. Not silenced:
"the drain did not run" is still a fact about this shutdown. An error line that is
expected on the one path where it always fires trains readers to skip error lines.

Found by running the probe, not by reading the diff.

## Known limits

- **The split-brain window is bounded, not closed.** Between the recreate and the
  old daemon's exit (~15s) a second daemon can start, and for that window both are
  alive. The singleton guard keeps its evidence inside the directory whose loss it
  must survive, and the chain is three links, not one: the socket cannot stand in
  for the pidfile either, because `main.rs` unconditionally removes any existing
  socket before binding — the socket is guarded by the pidfile, the pidfile by the
  data directory, and the data directory by nothing.

  **Correction (FR-170, measured after this document was written).** The sentence
  above understated the residue, and the understatement mattered. The window was
  not merely a period during which two daemons coexist: at the *end* of it, this
  document's own self-termination unlinked the **successor's** socket and pidfile,
  because teardown named both by path and those paths had become the successor's.
  The survivor was left alive holding seven database fds on an unlinked socket,
  invisible to `daemon status`, unreachable by `daemon stop`, never self-terminating
  (its own data directory was intact, so this watcher never fired) — and with the
  pidfile gone, a third daemon started cleanly on the same path. The mechanism this
  document describes is unchanged and correct; what was wrong was calling the
  residue bounded. Ruled and closed by
  [DD-186](186-daemon-artifact-ownership.md), which also records why the ruling was
  artifact ownership rather than the lock FR-170 anticipated.
- **Only whole-directory loss is detected.** The socket file deleted on its own,
  the database file deleted on its own, or the database truncated, are each a
  distinct "I can no longer serve" signal, and none is measured or handled. The FR
  recorded this and it is unchanged.
- **Behaviour on the real `~/.orchestratord` is inferred, not observed.** Every
  measurement ran against an isolated data directory, deliberately — the runtime
  database is not something to delete for a test.
- **`slack-gateway` owns a separate database** (`separate-database` in the
  persistence ledger) and its reaction to losing its own data directory is not
  measured. The shape generalises; the ruling does not.
- **The watcher is not armed if the data directory cannot be stat'd at startup.**
  It warns and continues rather than refusing to start, because refusing would
  turn a diagnostic into an outage. A daemon in that state has larger problems
  than this watcher.
