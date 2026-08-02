---
lifecycle: active
related_fr: FR-159
---

# DD-171: Interactive Session Process Reclamation

**Status**: Released
**Module**: Orchestrator Daemon / Session Store / Scheduler Spawn
**Related**: DD-108, DD-112, DD-115 (session control plane), FR-033, FR-040/046
**QA**: [QA 209](../../qa/orchestrator/209-interactive-session-process-reclamation.md)

## Problem

Interactive session processes were reclaimed by nothing, and the leak was
measured rather than inferred: 28 `session-control-mock` processes alive up to 19
days on the development machine, 23 reparented to `init`, 6 orphan `orchestratord`
still listening on 19394–19399, 133.7 hours of accumulated CPU (2026-08-02).

Four mechanisms combined, each defensible alone:

1. Children are spawned with `process_group(0)` so a group signal can take a
   whole subtree — which also makes them immune to anything that kills only the
   daemon's group.
2. Nothing in the scheduler could reach a session. This is the layer the FR got
   wrong and it matters for the design, so it is stated precisely below.
3. `reconcile_sessions` changed database rows and never signalled the OS, even
   though `process_identity_status` had already computed the judgement it needed.
4. `cleanup_stale_sessions` deletes rows in `failed` — the state (3) assigns to a
   live orphan — so the system would abandon reclamation and then destroy the
   only record of what it had abandoned.

## What the FR got wrong, and why it changed the design

Three of FR-159's claims did not survive verification against the tree. Two of
them changed what was built.

### The scheduler cannot reach a session at all

The FR said `shutdown_running_tasks` misses sessions because it iterates
`state.running`. The mechanism is different and worse: a tty session's child is
**never registered in `runtime.child`**, because `phase_runner/spawn.rs` takes
the `tty_early_return` branch and returns before the assignment. Every kill path
in the scheduler — shutdown, `task delete`, step timeout, stall auto-kill,
cross-process pause — goes through that field, so none of them can reach a
session, running or not, graceful or not.

The only reaper left was tokio's `kill_on_drop`, and it signals a **single PID**,
not the group. That predicts precisely the shape the triage recorded: 18 of 23
groups had a dead leader and a live `sh -c` with its `sleep` beneath it. The
leader was decapitated; the group was orphaned.

Consequences: `task delete` and step timeout also orphan sessions, not only
daemon death; and requirement 4 (shutdown drain) is not an extra layer over an
existing one — it is the only graceful reclamation a session has ever had.

### The FR's primary acceptance criterion contradicts DD-112

FR-159 asked that a session be reclaimed after the daemon is `SIGKILL`ed and
restarted. Implementing that requires a parentage test — a live process that is
not a child of the running daemon — and doing so **deletes a working feature**.

DD-112 §54 makes *verified live process plus transport* converge to `active`,
`detached` or `draining` by design. §35 records keeping an orchestrator-owned
child alive across daemon runtime teardown as an intended property. QA 149
scenario 5 asserts the session stays attachable across a restart. This is not an
oversight: the input FIFO is a named pipe on disk and the output capture is a
file, so a new daemon genuinely can drive a session that outlived its parent.

The parentage rule was implemented, and the restart scenario failed with
`session is not attachable` — reconciliation marked a session that DD-112
requires to be `detached` as `failed` and killed it. The rule was reverted.

**Decision**: reclamation triggers on *transport gone*, the rule FR-159's own
requirement 1 specifies. It matches the leak actually observed — those temp
directories had been removed, so the FIFOs went with them. The residual case, a
daemon `SIGKILL`ed with its data directory intact, is the resumable session
DD-112 intends to keep; its other entrances are covered by the shutdown drain and
the QA harness sweep.

### `cleanup_stale_sessions` has never run

Zero production call sites: three definitions and two tests. The amnesia path the
FR calls its sharpest point is real in the code and latent, not active. It is
fixed as public API, not as a cause of the observed leak.

## Design

### The reclamation primitive

`session_store::reclaim_process_group(pid, expected_fingerprint, signal)` refuses
unless three things hold, each evaluated **adjacent to the signal** rather than
inherited from the reconciliation pass that produced the candidate. A fingerprint
verified early and acted on late is not a PID-reuse guard at all.

1. `process_identity_status` is `VerifiedLive`. `Mismatch` and `Unsupported`
   never signal: mistakenly killing a reused PID costs more than leaking one
   process.
2. `getpgid(pid) == pid`. **Not in the FR.** A fingerprint proves the PID is the
   same process it always was; it says nothing about group membership, and
   `kill(-pid, …)` addresses whichever group carries that number. Identity
   standing in for leadership is a proxy for a different property.
3. `session_reclaim_enabled`, checked one layer up where configuration is
   readable.

The signal goes to `-pid`. `Immediate` sends one `SIGKILL`, for an orphan whose
transport is already gone. `Graceful` sends `SIGTERM`, waits, then `SIGKILL` —
used on shutdown, where sessions are still healthy and may flush. Every group in
the recorded triage exited on `SIGTERM` alone.

### Why a new policy flag

`session_reclaim_enabled` defaults to `true` and is deliberately **not** a facet
of `session_control_enabled`, which defaults to `false`. Hanging reclamation off
the existing flag would have left this fix inert in every deployment that had not
opted into session mutation — a gate certifying an enforcement it does not
perform.

Failure is closed: if the active configuration cannot be read, nothing is
signalled. A daemon that cannot tell whether it may kill processes must not.

### Where the kill lives

`reconcile_sessions` stays a pure `&Connection` state transition and returns
`ReconcileOutcome { changes, reclaim_candidates }`. The daemon loop performs the
signal, because that is the layer with both configuration and the event sink, and
because the identity re-check only means something next to the kill.

Rows already in `failed` are re-examined for reclamation even though their state
cannot move. Without that a single missed reclamation is permanent: the first
pass marks the orphan `failed` and every later pass filters it out before looking
at it.

### Directory reclamation

The session's own directory is removed, and **only after a signal actually went
out**, so a refusal never destroys the evidence of what it refused.

The path is derived, never assumed: `input_fifo_path`, `transcript_path` and,
when present, `output_json_path` must share one parent; that parent must be named
for the session; its parent must be `sessions`. Anything else yields `None` and
nothing is deleted.

`stdout_path` and `stderr_path` are excluded, and the first implementation
included them. They are the step's run logs and live elsewhere, so requiring
their agreement rejected every real session and silently cleaned nothing. The
stricter rule looked safer and was simply wrong — an over-reaching predicate
costs nothing until the case that trips it appears, and here it was every case.
Only running the gate caught it.

Per FR-159, no `$TMPDIR` sweeper was added. A background process holding delete
authority with only a filename and an mtime to judge by is one prefix away from
deleting the wrong thing, and CLAUDE.md's first prohibition is about exactly that.

### Session stdin

`phase_runner/spawn.rs` binds session stdin with `0<>` rather than `<`.

`write_fifo_atomically` opens, writes and closes on every `send-input`. Under a
read-only redirect the FIFO reaches EOF as soon as the first message is
delivered, so a session that blocks on `read` exits after one message, and one
that loops on EOF spins. The mock fixture's `sleep 0.05` poll was a workaround
for this, at roughly 315 minutes of CPU per orphaned process.

Both the premature exit and the spin are properties of the redirect rather than
of any agent command, so the repair is at the call site that builds it. Every
interactive session gets it; any real agent reading stdin had the same
one-message lifetime.

Measured: with `<`, a blocking reader receives exactly the first message and
dies; with `0<>`, it receives all three across separate writer cycles and sits at
0:00.01 CPU.

### `AgentSessionClose`

Signals the process group when the PID provably leads one, falling back to the
single-process form otherwise — it can under-reach, never over-reach. Negating a
PID that is not a group leader would deliver the signal to an unrelated group.

## Temp-directory leaks

FR-159 measured 14937 leaked directories with
`find $TMPDIR -maxdepth 3 -name agent_orchestrator.db`. That marker is scoped to
one filename and to directories, so it could not see four more producers leaking
6047 further entries. An enumeration guards exactly what its author knew, and
what falls outside it never produces a line in any log — here the instrument was
the FR's own.

Re-derived by grouping every top-level `$TMPDIR` entry by prefix shape:

| Shape | Count | Producer | In the FR? |
|---|---|---|---|
| `agent-orchestrator-test-*` | 10843 | `db_write.rs` `mem::forget` | yes, cause misdiagnosed |
| `config-load-test-*` | 4021 | `make_test_db` | yes |
| `test-guard-*.db-wal`/`-shm` | 2882 | `config_load/build.rs` ×6 | no |
| `db-test-*` | 2859 | `db.rs` `tmp_db_path` | no |
| `item-exec-test-*` | 267 | `item_executor/tests.rs` | no |
| `orch-streaming-*` | 39 | dead producer, residue | no |

Three shapes are worth recording individually.

**The 10843 were not a `Drop` failure.** FR-159 instructed that the cause be
established before touching the code, and it is: `core/src/db_write.rs` called
`std::mem::forget(fixture)` in a shared helper, with a comment explaining why —
the helper returns only `Arc<InnerState>`, so an in-place drop would delete the
directory before the test used it. Exactly 38 tests call it, `make_test_db` has
exactly 14 call sites, and 38:14 = 19:7, the ratio the FR observed. `TestState`'s
`Drop` works and a test already proved it. The repair returns the fixture.

**`config_load/build.rs` is the sharpest.** Six tests ended with
`remove_file(&db_path)`, and that call works — zero `.db` files remain. What
remained was 1441 `-wal` and 1441 `-shm`: cleanup that names one artefact removes
exactly the artefact it names and knows nothing of SQLite's two sidecars.

**Ten of eleven remembered.** `item_executor/tests.rs` gave each test a trailing
`remove_dir_all`; the one that forgot, `slv-pathkey`, is the only one of the
eleven that accumulated. A guard replaces eleven chances to forget, and unlike a
trailing call it also survives a panicking test.

The FR's per-day ratio claim was overstated — 11 of 15 days match 19:7, four
deviate — but the conclusion it drew stands, now carried by the code.

Measured after: a full `cargo test --workspace` under a private `TMPDIR` reports
2851 passed, 0 failed and leaves that directory empty. The test count is part of
the evidence, because a run that never started also leaves it empty.

## Known limits

- `cleanup_stale_sessions` remains uninvoked in production.
- A session orphaned by `SIGKILL` with its data directory intact is not reclaimed
  by the daemon; this is DD-112's resumable session, kept deliberately.
- Temp-directory sites that clean up with a trailing `remove_dir_all` rather than
  an owned guard still leak on a panicking test. The six that accumulated
  measurably are repaired; the panic path is not swept, and the surface is 78
  `std::env::temp_dir()` call sites across 13 files.
- The reclamation gate is `manual-runbook`, not `ci-required`: it starts a daemon
  and signals process groups.
- **`coordination-collapse-ledger.json`'s `capturesOrJsonPath` coordinate counts
  `output_json_path`.** The scanner matches `/captures|json_path/` per line, and
  `output_json_path` — the session's structured-output spill path, a live feature
  with no relation to the retired JSONPath extraction the coordinate tracks —
  contains that substring. This FR moved the number 53 → 55 purely by touching
  `output_json_path` twice more in production code. Across tracked Rust sources
  59 of 192 matching lines are `output_json_path`, so the coordinate overstates
  the debt it claims to measure and will drift again for the same reason whenever
  session code is touched. The baseline was regenerated per the documented
  workflow rather than the matcher narrowed: anchoring it changes the meaning of
  a reviewed ledger coordinate, which belongs to the coordination-collapse owner
  and not to this FR. Recorded here so the next author of that ledger does not
  have to rediscover it. An unanchored substring is a predicate with an open end,
  and the question it never answers is what it includes that nobody named.
- **`scripts/qa/ci-liveness.rb` is red for reasons unrelated to this work.** Its
  job records were taken at `45fbf3c4`, before `.github/workflows/ci.yml` last
  changed at `ceccf4f5`; both predate this FR's branch point and nothing here
  touched the workflow. Refreshing it requires a real CI run, so it is left
  failing rather than papered over.
