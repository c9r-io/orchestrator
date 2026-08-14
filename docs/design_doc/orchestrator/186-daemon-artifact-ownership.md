---
lifecycle: active
related_fr: FR-170
---

# DD-186: A Daemon's Socket And Pidfile Are Its Own By Identity, Not By Path

**Status**: Released
**QA**: [224](../../qa/orchestrator/224-daemon-artifact-ownership.md)
**Closes the residual recorded by**: [DD-185](185-daemon-data-dir-vanish-self-termination.md)

## The problem, which was not the one that was filed

FR-170 was filed against the singleton guard: `detect_running_daemon` reads a pidfile
that lives *inside* `data_dir`, so deleting the directory deletes the evidence, and a
second daemon starts on the same path. The socket cannot stand in for the pidfile
either, because the daemon unconditionally removes any existing socket before binding.
Three links — socket guarded by pidfile, pidfile guarded by the data directory, data
directory guarded by nothing.

All of that is true, and it is not where the harm was.

DD-185 recorded the residual as a bounded ~15s overlap: FR-169 made the old daemon
notice its directory was gone and exit, so two daemons coexist only until it does.
Step-0 verification measured the window and then measured what happens *at the end* of
it. Both numbers held — the successor is ready in 1s, the predecessor exits after 15s
with its seven database fds going to zero — and the end state was this:

| After the predecessor exits | Observed |
|---|---|
| successor process | **alive**, 7 db fds, listening on an **unlinked** socket inode |
| socket on disk | **gone** |
| pidfile | **gone** |
| `orchestrator daemon status` | `orchestratord is not running` |
| `orchestrator daemon stop` | `orchestratord is not running (no PID file)`; daemon survives |
| will it ever self-terminate? | **no** — its own `data_dir` identity is intact, so FR-169's watcher never fires |
| a third daemon on the same path | **starts and becomes ready** |

The predecessor's teardown is `lifecycle::cleanup(&socket_path, &pid_path)`, and both
arguments are paths. After delete-and-recreate those paths are the **successor's**
files. So the dying daemon unlinks its successor's socket and pidfile, and what is left
is precisely the orphan DD-174 was written about: alive, unreachable, unstoppable,
invisible — plus, with the pidfile gone, nothing left to bound the daemon count.

FR-169 manufactured the thing it was written to prevent. The window was never the
problem; the exit was. Bounding the window tighter would not have helped, because the
damage is done by the exit and not by the overlap.

## The ruling: (a), with the half the reading was missing

FR-170 required a ruling before a mechanism, between two readings:

- **(a)** the second daemon is legitimate — a recreated directory is a new state
  directory, and the disease is that the old process is still alive;
- **(b)** one path admits one daemon regardless of what the directory went through,
  which needs evidence that survives outside `data_dir`.

**(a) is the ruling.** But (a) as the FR stated it is incomplete: "the old process
exits" is not sufficient, because the old process's *exit* is what destroys the new
one. (a) requires that a daemon exit without touching artifacts it no longer owns.

**Why (b) is rejected, recorded so the next reader can tell this was decided and not
skipped.** (b) does not fix more, and it costs more:

- It still needs this same repair. The measured harm happens on the way out, and no
  admission check on the way in prevents a daemon that is already running from
  unlinking a file at exit.
- Every candidate for data-dir-external evidence was priced and none survives. An
  `flock` on a lockfile inside `data_dir` has the pidfile's disease exactly. Abstract
  namespace sockets are Linux-only and this repository runs on macOS too. A lock at a
  fixed OS location keyed by a hash of the `data_dir` path mints a new permanent
  global concept, and path aliasing — a symlink, `/tmp` versus `/private/tmp` — hashes
  one directory to two keys. Scanning the process table turns the guard into an
  inference about who has some inode open and needs traversal permission.
- The tree has **no `flock` precedent** at all (zero occurrences outside prose).

**Concept budget.** (a) mints nothing: no new persistent artifact, no new user-facing
noun, no new `resource.<kind>` audit action name. What changes is the meaning of an
existing teardown. That is what FR-170's own concept-budget section demanded be argued
for before anything was created.

**The ~15s window stays.** DD-185 chose 5s × 3 confirmations so that a single transient
`stat` failure cannot end a healthy daemon, and 15s is far below any supervisor
reaction time. With ownership fixed the overlap is inert: two daemons, two different
databases, the predecessor serving nobody because its own socket is unlinked. Measured
again after the fix at 15s, unchanged.

## What it says

`lifecycle::cleanup` takes the identity of the socket this process bound, and removes
each artifact only while it is still that process's:

- **socket**, by `(st_dev, st_ino)` recorded in `main.rs` immediately after `bind` and
  `set_permissions`. `None` on the `--bind` TCP paths, which bind no socket file at all
  — previously those runs deleted a socket some earlier UDS daemon had left, which was
  never theirs to delete.
- **pidfile**, by its **contents** naming this process.

The two use deliberately different evidence, and the asymmetry is the point. A socket
has no readable content, so its inode is its identity. A pidfile has one — and for it
the inode is the *wrong* question, because `std::fs::write` truncates in place: a
successor writing the same path keeps the same inode, and an identity check would call
its pidfile ours. The unit fixture for that case therefore overwrites in place rather
than deleting and rewriting, which is the mutation an inode-based implementation would
pass for the wrong reason.

The entry-side `remove_file(&socket_path)` before `bind` was re-argued rather than
inherited, which FR-170 required. It stays unconditional: reaching it means the pidfile
guard found no live daemon, and a socket left behind by a SIGKILL must be removable or
a crashed daemon could never restart — a state `orchestrator-client`'s `connect.rs`
already explains to users. What no longer stays is the discarded error. `NotFound` is
tolerated; anything else now fails naming the socket path, where before an EACCES or a
directory at that path surfaced as `failed to bind UDS`, naming the wrong file.

## Routes in, counted rather than assumed

FR-170 listed three uncounted possibilities. All were checked:

- **`mv` away, symlink replacement, remount** all reduce to a `(dev, ino)` change,
  which `data_dir_identity` already detects. They enter the same window and therefore
  the same detonation, so they are one assertion in the gate rather than a second
  mechanism.
- **Concurrent start on a healthy directory is not another way in.** Measured: two
  daemons started simultaneously, and the loser died on `failed to configure sqlite wal
  mode / database is locked`. The SQLite lock is an accidental **inode-scoped** guard
  that is stronger than the pidfile — and that is exactly why delete-and-recreate was
  the hole. A new directory means a new database inode, so the one guard that actually
  holds does not apply to it.
- **`slack-gateway` has no singleton guard at all** — no pidfile, no socket. It binds a
  TCP port and the OS bind is its guard, so there is no same-shaped guard to repair.

## Known limits

- **The ~15s overlap remains, deliberately.** Two daemons are alive, on two different
  databases. The predecessor serves nobody, but work already in flight in it lands in
  the unlinked database and is lost. The directory was deleted by an operator, so that
  is their ruling and not the daemon's; it is recorded rather than fixed.
- **A daemonized predecessor cannot explain itself after `rm -rf`.** `orchestratord`
  redirects stdout/stderr to `$data_dir/daemon.log` when it daemonizes, so its account
  of "my data directory vanished" is written *into the directory that vanished*. After
  `mv` the file is still readable at its new location; after `rm -rf` it is an unlinked
  inode with no name, and no assertion can recover it. This is FR-170's own shape —
  evidence stored where it cannot survive — applied to a diagnostic rather than to a
  socket. The QA gate asserts what is observable in each form and states which case it
  cannot cover; nothing here fixes it.
- **Only whole-directory loss is detected**, unchanged from DD-185: the socket deleted
  on its own, the database deleted on its own, or the database truncated are each a
  distinct "I can no longer serve" signal and none is measured.
- **Behaviour on the real `~/.orchestratord` is inferred, not observed.** Every
  measurement ran against isolated data directories, deliberately.
- **A gate that guards against a defect can carry it.** This one did: `inode_of`
  returned non-zero for a missing socket, `set -e` took the enclosing assignment, and
  against the pre-fix binary the gate printed two PASS lines, **no FAIL line and no
  summary**, while the defect it exists to catch was present. A truncated run reads
  exactly like a complete one to a reader who trusts the exit code. Both halves are
  repaired — the helper is total, and the EXIT trap announces a run that ended before
  its summary — but the general lesson is in the skill's §4.4, not only here.
