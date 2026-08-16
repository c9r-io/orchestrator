---
lifecycle: active
related_fr: FR-170
self_referential_safe: true
---

# Orchestrator - A Daemon's Socket And Pidfile Survive Its Predecessor's Exit

**Module**: Daemon lifecycle
**Scope**: that a daemon exiting because its data directory vanished does not unlink
the *successor's* socket and pidfile; that the successor stays reachable and stoppable
afterwards, so the daemon count stays bounded; that a normal stop still cleans up after
itself; that a crashed daemon's debris is still reclaimed by the next start; and that
`--foreground` and daemonize give the same answers
**Scenarios**: 4
**Priority**: High

## Background

FR-169 made a daemon whose data directory vanished shut itself down, and
[DD-185](../../design_doc/orchestrator/185-daemon-data-dir-vanish-self-termination.md)
recorded the residue as a bounded ~15s overlap. It was not bounded. Teardown named its
two artifacts by path, and after delete-and-recreate those paths belong to the
successor: the predecessor's exit unlinked the **new** daemon's socket and pidfile,
leaving it alive with seven database fds and a listening socket on an unlinked inode —
invisible to `daemon status`, unreachable by `daemon stop`, and never self-terminating,
because its own data directory was intact so the vanish watcher never fired. With the
pidfile gone, a third daemon then started cleanly on the same path.

The repair is
[DD-186](../../design_doc/orchestrator/186-daemon-artifact-ownership.md): a daemon
removes the socket only while the ownership token beside it still names this process,
and the pidfile only while it still names this process.

**Amended 2026-08-15.** DD-186 shipped the socket half as an inode comparison, on the
reasoning that a socket has no readable content so its inode is its identity. The first
premise is true and the second does not follow: unlinking the path frees the inode even
while the listener is accepting on it, so the number is reusable at once — and Linux
reuses it, in 50 of 50 measured trials for a regular file. The guard was therefore
inverted on the platform this daemon ships to: a dying daemon would read a successor's
socket as its own and unlink it, which is the exact damage it was written to prevent.
Certification was on APFS, which does not reuse. The socket now gets the readable content
it lacked, in a `<socket>.owner` token holding a per-process UUID, and the comparison is
content — the same evidence the pidfile half always used.

## Safety

Every scenario runs against its own `mktemp` data directory and its own `HOME`. The
developer's daemon, database and `~/.orchestratord` are never touched, and no scenario
deletes a runtime database. Daemons are started and stopped through
`scripts/lib/gate_daemon.sh` per the `CLAUDE.md` daemon-lifecycle rule.

## Automated coverage

```bash
cargo build -p orchestratord -p orchestrator-cli
bash scripts/qa/test-daemon-artifact-ownership.sh
```

42 assertions; the gate is `manual-runbook` because it starts real daemons and needs
built binaries. On macOS, `TMPDIR` is a 48-character `/var/folders/...` path and a UDS
path has a hard 104-byte limit, so the gate checks the budget up front and fails by
name rather than emitting four "the daemon never became ready" timeouts.

The unit half runs in CI:

```bash
cargo test -p orchestratord --bins lifecycle::tests::cleanup
cargo test -p orchestratord --bins lifecycle::tests::pid_file_is_ours
```

---

## Scenario 1: The successor survives its predecessor's exit

**Steps**

1. Start daemon A on a fresh data directory and wait until it is ready.
2. `rm -rf` the data directory and `mkdir` it again at the same path.
3. Start daemon B on that path and wait until it is ready. Record the socket's **inode**,
   its **ownership token** and the pidfile's contents while both daemons are alive.
4. Wait for A to self-terminate (~15s: `DATA_DIR_CHECK_PERIOD` × `DATA_DIR_CHECK_CONFIRMATIONS`).
5. Re-read the socket inode, its ownership token and the pidfile; run
   `orchestrator daemon status` and one real RPC (`orchestrator task list -o json`).
6. Count A's open database fds during the overlap and again after it exits.

**Expected result**

- A exits within the budget, naming the data directory in its log.
- The socket is still present **with the inode and the ownership token recorded in step
  3**. Presence alone is not sufficient, since a deleted socket recreated by anything
  would satisfy it — and on Linux neither is the inode alone, since a rebound socket
  usually takes the freed number straight back. The token cannot collide.
- The pidfile still names B.
- A holds database fds on the orphaned inode **during** the overlap and 0 after — 7 → 0.
  The `7` is the half that carries the weight: "a dead process holds no fds" is true of
  any dead PID and of one that never existed, so the post-exit count alone is vacuous.
- `daemon status` reports B by PID, and the RPC succeeds — the file existing is not the
  same fact as the service answering.

Repeat for `mv data_dir away && mkdir data_dir`: every route that changes the
directory's `(dev, ino)` enters the same window and must reach the same end state.

## Scenario 2: The daemon count stays bounded

**Steps**

1. Reach the overlap of scenario 1, with B ready and A not yet exited.
2. Attempt to start a third daemon on the same path.
3. Wait for A to exit, then attempt a third daemon **again** — the decisive half.

**Expected result**

Refused both times, with the diagnostic naming the holder:
`another orchestratord is already running (PID <B>)`. The **string and the PID** are
asserted, not the exit code — a start can fail many ways and a code cannot say which.

The second attempt is the one that matters. During the overlap the pidfile still
existed, so a third daemon was refused before DD-186 too; it is *after* A's teardown —
which used to delete that pidfile — that the refusal had nothing left to stand on, and
a third daemon started cleanly onto a path already held by two processes.

## Scenario 3 (negative): A normal stop still cleans up after itself

**Steps**

1. Start a daemon, wait for readiness, and stop it with `gate_daemon_stop` (SIGTERM).
2. Inspect the data directory.

**Expected result**

Both the socket and the pidfile are **removed**. This is the path that runs every day
and the one an over-strict ownership check breaks. Without it, a `cleanup` that removes
nothing at all would satisfy scenarios 1 and 2.

## Scenario 4 (negative): Crash debris is still reclaimed

**Steps**

1. Start a daemon, wait for readiness, record its socket ownership token, then
   `SIGKILL` it (`gate_daemon_kill_hard`).
2. Confirm the socket and pidfile were left behind.
3. Start a new daemon on the same directory and wait for readiness.

**Expected result**

The debris survives the SIGKILL — that is what the entry-side removal exists for — and
the next start reclaims it and binds successfully, writing a **different** ownership
token. That comparison is what distinguishes "the stale socket was removed and rebound"
from "the stale socket was reused"; an existence check cannot tell those apart, and
neither can an inode comparison on Linux — this assertion read `REBOUND_INO != STALE_INO`
until 2026-08-15, which would have failed there on a restart that worked perfectly.

## Checklist

- [ ] Scenario 1: after the predecessor exits, the socket is present with **the same
      inode** recorded during the overlap, and the pidfile still names the successor
- [ ] Scenario 1: `daemon status` names the successor and a real RPC through that
      socket succeeds — the file existing is not the service answering
- [ ] Scenario 1: the `mv` route reaches the same end state as `rm -rf`
- [ ] Scenario 1: the predecessor holds database fds on the orphaned inode **during**
      the overlap, and they fall to 0 after it exits (7 → 0)
- [ ] Scenario 2: a third daemon is refused with `another orchestratord is already
      running (PID <B>)` — the string and the PID, not the exit code
- [ ] Scenario 2: still refused **after** the predecessor's teardown, which is the
      case that actually failed before DD-186
- [ ] Scenario 3: a clean SIGTERM stop removes both the socket and the pidfile
- [ ] Scenario 4: SIGKILL leaves both behind, and the next start binds a socket it
      claims with a **different** ownership token
- [ ] Scenarios 1 and 3 give identical verdicts under `--foreground` and daemonize
- [ ] The gate reports a verdict, not a truncated run: the summary line is present

## Mutation Evidence

| Mutation | Caught by | Diagnostic |
|---|---|---|
| restore the pre-FR-170 unconditional `cleanup` (both `remove_file` calls unguarded) | Scenarios 1 and 2, in all four form/route combinations — **22 passed, 20 failed**, while Scenarios 3 and 4 stay green | `socket inode was <N> during the overlap, now <gone>`; `pidfile should name <PID>, reads <gone>`; `daemon status did not name <PID>; got: orchestratord is not running`; and the decisive one, `after the predecessor's exit a third daemon STARTED — the pidfile that refuses it is gone` |
| `cleanup` that removes nothing at all | Scenario 3 only | `a clean stop left socket=present pidfile=present` |
| pidfile ownership checked by inode instead of contents | unit `cleanup_leaves_a_pid_file_naming_another_process` | the fixture overwrites in place, so the inode is unchanged and an identity check deletes a successor's pidfile |
| socket ownership checked by inode instead of the token (the pre-2026-08-15 shape) | unit `cleanup_leaves_a_socket_that_is_no_longer_the_one_we_bound`, **on Linux only** | the successor's claim overwrites in place, so the inode is unchanged and an identity check unlinks a live successor's socket; green on macOS, which is how it shipped |
| remove the `.owner` token and keep the claim | unit `cleanup_leaves_a_socket_whose_token_has_been_removed` | an unreadable token must fail closed, or a crash between the two unlinks turns into a successor's socket removed by a stranger |

Scenarios 3 and 4 staying green under the first mutation is the evidence that they
guard the opposite direction — an over-strict ownership check — rather than
duplicating their neighbours.

The mutation run is also how two defects **in the gate itself** were found, both of
which would have made a regression unreportable rather than red:

- `inode_of` returned non-zero for a missing socket, `set -e` took the enclosing
  assignment, and the run ended before its summary line — a truncated run, which reads
  exactly like a complete one to anyone trusting the exit code (§4.4 shape 7). The
  helper is now total and an EXIT trap announces a run that never reached its summary.
- The third-daemon probe ran `orchestratord --foreground` in a command substitution,
  which assumes the refusal. Under the regression the daemon *starts*, and in
  foreground it never returns: the gate **hung** instead of failing. macOS has no
  `timeout(1)`, so `probe_third_daemon` bounds the wait itself, reports "a third daemon
  STARTED" as the failure, and reclaims the process it started.

Run every new gate against the broken state it exists to catch, and check that it
produced a verdict — not merely a non-zero exit, and not a hang.

## Known limits

- **A daemonized predecessor cannot explain itself after `rm -rf`.** It redirects its
  output to `$data_dir/daemon.log`, so its account of the vanish is written into the
  directory that vanished. After `mv` the log is readable at its new location; after
  `rm -rf` it is an unlinked inode. Scenario 1 asserts the diagnostic where it is
  observable and, for `rm` + daemonize, asserts instead that the recreated
  `daemon.log` is the *successor's* and carries no vanish diagnostic — which
  distinguishes "the name was re-taken" from "the predecessor never explained itself".
- **The ~15s overlap is deliberate and remains.** Work in flight in the predecessor
  lands in the unlinked database and is lost. See DD-186.
