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
removes the socket only while the path still resolves to the inode it bound, and the
pidfile only while it still names this process.

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

30 assertions; the gate is `manual-runbook` because it starts real daemons and needs
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
3. Start daemon B on that path and wait until it is ready. Record the socket's **inode**
   and the pidfile's contents while both daemons are alive.
4. Wait for A to self-terminate (~15s: `DATA_DIR_CHECK_PERIOD` × `DATA_DIR_CHECK_CONFIRMATIONS`).
5. Re-read the socket inode and the pidfile; run `orchestrator daemon status` and one
   real RPC (`orchestrator task list -o json`).

**Expected result**

- A exits within the budget, naming the data directory in its log.
- The socket is still present **with the same inode recorded in step 3** — presence
  alone is not sufficient, since a deleted socket recreated by anything would satisfy it.
- The pidfile still names B.
- `daemon status` reports B by PID, and the RPC succeeds — the file existing is not the
  same fact as the service answering.

Repeat for `mv data_dir away && mkdir data_dir`: every route that changes the
directory's `(dev, ino)` enters the same window and must reach the same end state.

## Scenario 2: The daemon count stays bounded

**Steps**

1. Reach the overlap of scenario 1, with B ready and A not yet exited.
2. Attempt to start a third daemon on the same path.

**Expected result**

Refused, with the diagnostic naming the holder:
`another orchestratord is already running (PID <B>)`. The **string and the PID** are
asserted, not the exit code — a start can fail many ways and a code cannot say which.
Before DD-186 this case passed only until A exited, at which point the pidfile was gone
and a third daemon started cleanly.

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

1. Start a daemon, wait for readiness, record its socket inode, then `SIGKILL` it
   (`gate_daemon_kill_hard`).
2. Confirm the socket and pidfile were left behind.
3. Start a new daemon on the same directory and wait for readiness.

**Expected result**

The debris survives the SIGKILL — that is what the entry-side removal exists for — and
the next start reclaims it and binds successfully, with a **different** socket inode.
The inode comparison is what distinguishes "the stale socket was removed and rebound"
from "the stale socket was reused"; an existence check cannot tell those apart.

## Checklist

- [ ] Scenario 1: after the predecessor exits, the socket is present with **the same
      inode** recorded during the overlap, and the pidfile still names the successor
- [ ] Scenario 1: `daemon status` names the successor and a real RPC through that
      socket succeeds — the file existing is not the service answering
- [ ] Scenario 1: the `mv` route reaches the same end state as `rm -rf`
- [ ] Scenario 2: a third daemon is refused with `another orchestratord is already
      running (PID <B>)` — the string and the PID, not the exit code
- [ ] Scenario 3: a clean SIGTERM stop removes both the socket and the pidfile
- [ ] Scenario 4: SIGKILL leaves both behind, and the next start binds a socket with a
      **different** inode
- [ ] Scenarios 1 and 3 give identical verdicts under `--foreground` and daemonize
- [ ] The gate reports a verdict, not a truncated run: the summary line is present

## Mutation Evidence

| Mutation | Caught by | Diagnostic |
|---|---|---|
| restore the pre-FR-170 unconditional `cleanup` (both `remove_file` calls unguarded) | Scenarios 1 and 2, in all four form/route combinations — **16 named failures**, while Scenarios 3 and 4 stay green | `socket inode was <N> during the overlap, now <gone>`; `pidfile should name <PID>, reads <gone>`; `daemon status did not name <PID>; got: orchestratord is not running` |
| `cleanup` that removes nothing at all | Scenario 3 only | `a clean stop left socket=present pidfile=present` |
| pidfile ownership checked by inode instead of contents | unit `cleanup_leaves_a_pid_file_naming_another_process` | the fixture overwrites in place, so the inode is unchanged and an identity check deletes a successor's pidfile |

Scenarios 3 and 4 staying green under the first mutation is the evidence that they
guard the opposite direction — an over-strict ownership check — rather than
duplicating their neighbours. The first mutation was also run to confirm the gate
produces a **verdict** and not an abort: an earlier revision of it exited before the
summary line, which is a truncated run and not a red one.

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
