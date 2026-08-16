---
lifecycle: active
related_fr: FR-142
self_referential_safe: true
---

# Orchestrator - Trigger History Limit Cascade

**Module**: Trigger engine / Persistence
**Scope**: that `Trigger.historyLimit` actually deletes a task that ran; that a task it may not
delete is left byte-for-byte intact and reported by cause; that a failure which is not a child row
is not filed as a retention skip; that every sweep is audible at the daemon's default log filter;
and that the frozen schema is unchanged
**Scenarios**: 5
**Priority**: High

## Background

`historyLimit` had never deleted a task. `trigger_state::delete_tasks` was a bare
`DELETE FROM tasks`; `task_items` references `tasks(id)` without a cascade, foreign keys are
enforced, and every task carries a `task_items` row from creation — so the delete was refused every
time, and the failure was logged at `debug!` under a default filter of `info`. No log line, no
shrinking table. See DD-150.

Each candidate now goes through `task_repository`'s existing cascade. Of the ten tables referencing
`tasks(id)`, two cascade and are removed by SQLite, and that cascade clears `task_items` with its
command runs and events.

**The known limitation recorded here is closed** (FR-168,
[DD-184](../../design_doc/orchestrator/184-task-delete-reference-disposition.md)). It read: the
remaining seven tables still refuse, retention skips and names them, and `orchestrator task delete`
propagates a bare `FOREIGN KEY constraint failed` naming nothing. All seven now carry a recorded
disposition applied inside the cascade itself, so both paths dispose of them identically — three
are deleted with the task, four keep their row with a null reference. The attribution moved with
it: `blocking_references()` and `references_holding()` are no longer local to `trigger_state.rs`
and the sweep no longer recomputes what the cascade already decided; it consumes a typed
`TaskDeleteBlocked`.

What survives is the *mechanism*, now reached only by a reference nobody has ruled on: skip whole,
name the cause, continue. Scenario 2 below is written against that, and had to change — it used to
pin a task with `resume_plans`, which no longer refuses anything.

Everything here runs against temporary SQLite files created by the test harness under `$TMPDIR`.
No scenario starts a daemon, writes to `~/.orchestratord/agent_orchestrator.db`, or invokes a
provider. The schema under test is built by `PersistenceBootstrap::ensure_current`, the same
registered migration chain production runs.

Primary entry points:

```bash
cargo test -p orchestrator-persistence --test round_trip     # 20 tests
cargo test -p agent-orchestrator --lib history_cleanup_visibility
ruby scripts/qa/persistence-dependency.rb                    # the ledger this change moves
git diff --stat config/governance/schema-snapshot.sql        # must be empty
```

---

## Scenario 1: A task that ran is deleted, with everything hanging off it

**Steps**

```bash
cargo test -p orchestrator-persistence --test round_trip \
  trigger_history_retention_keeps_the_newest_and_selects_nothing_else
```

The test seeds a task with a `task_items` row, a `command_runs` row carrying two log paths, and an
`events` row, then sweeps it through `delete_tasks_within_history_limit`.

**Expected result**

Passes. `deleted == 1`, `skipped` empty, and `log_paths` is exactly
`["/tmp/history-err.log", "/tmp/history-out.log"]`. Counting afterwards, `tasks`, `task_items` and
`events` hold zero rows for that id and the `command_runs` row is gone with the item it hung off.

The assertion is on the rows, not on the return code. `DELETE` returning `Ok` is what the original
defect *never* produced — the value of asserting the database state is that it cannot be satisfied
by a call that reports success without doing anything.

The same test first issues the original statement directly against the connection —
`DELETE FROM tasks WHERE id = ?1`, on a task holding one `task_items` row — and requires it to fail
with `FOREIGN KEY constraint failed`. That is the assertion DD-148 pinned, rewritten rather than
removed as FR-142 required. It is raw SQL because the API no longer makes that statement; what it
pins is the reason the API had to change. If it ever stops failing, the message says so: the defect
can no longer be reproduced, so the test no longer shows why the cascade is needed.

---

## Scenario 2: A task that may not be deleted is left whole and named

**Steps**

```bash
cargo test -p orchestrator-persistence --test round_trip \
  a_task_the_history_limit_cannot_remove_is_left_whole_and_named
```

Seeds a task with one item, one command run, two events, a row in a table created by the test that
references `tasks(id)` without a cascade and carries no recorded disposition, and a
`task_graph_runs` row (which cascades, so it must *not* be reported).

The pin used to be a `resume_plans` row. FR-168 ruled that table delete-with-task, so it stopped
refusing anything and could no longer drive this path. Creating the table in the test is the
stronger fixture anyway: it also proves the blocking set is derived from the schema at runtime,
because the table did not exist when the sweep was written.

**Expected result**

Passes. `deleted == 0`; `skipped` is exactly one entry whose `blocked_by` is
`["later_addition.task_id"]` and nothing else; `log_paths` is empty.

Then every seeded count is unchanged: 1 task, 1 item, 2 events, 1 command run. This is the
rollback assertion. The failure it guards against is not the task surviving — it is the task
surviving *stripped*, because the cascade deletes events and command runs before it hits the
foreign key on `DELETE FROM tasks`.

---

## Scenario 3: A failure that is not a child row is not a retention skip

**Steps**

Same test. It sweeps an id that does not exist.

**Expected result**

The call returns `Err`, and the message contains `not a retention skip`.

A missing task is the reachable instance — another writer can remove one between the retention
query and the sweep. The assertion exists because the whole defect being closed was a real failure
wearing a shape nobody looked at, and a skip list that quietly absorbs unrelated failures would
rebuild it one level up.

---

## Scenario 4: The sweep reports itself at the daemon's default log level

**Steps**

```bash
cargo test -p agent-orchestrator --lib history_cleanup_visibility
```

Three completed runs of one trigger, keeping one; of the two beyond retention the older is
deletable and the newer is pinned by a reference nobody has ruled on (a table the test creates), so
a single sweep produces both a delete and a skip. `cleanup_history` runs under a `tracing`
subscriber whose filter is
`EnvFilter::new("info")` — the exact fallback `crates/daemon/src/main.rs` uses when neither
`ORCHESTRATOR_LOG` nor `RUST_LOG` is set — and the emitted bytes are captured.

**Expected result**

The deletable task is gone from `tasks`, and the captured log contains `trigger history cleanup`
with `deleted=1`, the line `history limit skipped a task still referenced elsewhere`, and the
string `later_addition.task_id`. The pinned task also keeps its item, so a sweep that stripped a
task it refused to delete fails here too.

This is the half a correctness assertion cannot reach. A sweep can compute the right answer and
still be silent, and silence is what the FR existed to end: the original failure was logged at
`debug!` under this same filter, so there was no symptom at all. Asserting the counts without
asserting the output would leave the severity free to regress with every test still green.

---

## Scenario 5: The frozen schema and the ledger

**Steps**

```bash
git diff --stat config/governance/schema-snapshot.sql
ruby scripts/qa/persistence-dependency.rb; echo "exit=$?"
cargo test -p orchestrator-persistence --test round_trip \
  task_graph_rows_cascade_and_do_not_pin_a_task
```

**Expected result**

The schema diff is empty — FR-142's acceptance criterion, and the reason option A was not taken.
The ledger gate exits 0 against the regenerated
`config/governance/persistence-dependency-ledger.json`, whose only movement is
`trigger_state.rs sql 7 → 8` and the total `514 → 515`: one `DELETE` removed, two `SELECT`s added.

The third command asserts that a task carrying `task_graph_runs` and `task_graph_snapshots` deletes
cleanly and takes both with it — the correction to FR-142's claim that the repository's cascades
were not on this chain.

---

## Mutation Evidence

Each mutation was applied to the implementation, the suite was run, and the file was restored and
verified byte-identical with `diff -q` before the next one.

| Mutation | Assertion that failed | Observed |
|---|---|---|
| `items.rs`: replace `conn.unchecked_transaction()` with the bare connection and drop the commit | Scenario 2 | `the refused sweep changed the items of a task it did not delete: left 0, right 1` — the half-emptied task, exactly the state the transaction prevents |
| `references.rs`: drop `AND UPPER(COALESCE(f.on_delete,'')) <> 'CASCADE'` | Scenario 2 | `blocked_by` became `["later_addition.task_id", "task_graph_runs.task_id"]`, naming a table that never refused anything |
| `trigger_state.rs`: treat every error as a skip rather than only a `TaskDeleteBlocked` | Scenario 3 | `a missing task was absorbed as a skip: SkippedTask { task_id: "no-such-task", blocked_by: [] }` |

| `trigger_engine.rs`: regress the skip `warn!` and the summary `info!` back to `debug!` | Scenario 4 | the captured log came back **empty** — the original defect, reproduced exactly |

The third mutation **survived the first version of this suite**. Scenario 4 was written after it
was observed to survive, not before. So was scenario 4: the first version of this work asserted the
counts and the rows and left the logging — the FR's entire reason for existing — with no condition
on it at all. Both are recorded because the gap was in the tests, not in the code, and the audit
that finds such a gap is worth more than the fixture it produces.

---

## Checklist

- [ ] Scenario 1: a task with items, a command run and events is deleted, and the log paths come back
- [ ] Scenario 1: a bare `DELETE FROM tasks` still fails with `FOREIGN KEY constraint failed`
- [ ] Scenario 2: a task pinned by an unruled reference is skipped, named, and left with every row intact
- [ ] Scenario 2: a cascading `task_graph_runs` row is not reported as a blocker
- [ ] Scenario 3: a missing task surfaces as an error, not as a skip
- [ ] Scenario 4: a sweep prints its summary and its skip cause at an `info` filter
- [ ] Scenario 5: `config/governance/schema-snapshot.sql` is unchanged
- [ ] Scenario 5: the persistence ledger moves by exactly `trigger_state.rs sql 7 → 8`
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean

## Certification Conditions

A run counts as closure evidence only when all hold; otherwise it is void and must be re-run.

1. `git status --porcelain` is empty at start and at end.
2. Nothing else writes to the repository during the run.
3. `git rev-parse HEAD` is recorded before and after and matches.
4. Each command is invoked as `<cmd> > log 2>&1` with `$?` captured directly, never through a pager.
5. The final summary line of each log is present.

## Recorded Runs

Certified at `e73e701cdf3455b545b52aee6a26b6e8bfa42099`, macOS 25.5.0, 2026-07-27. `git rev-parse
HEAD` was recorded before and after and matched; `git status --porcelain` was empty at both ends;
each command was run as `bash -c '<cmd>' > log 2>&1` with `$?` captured directly, never through a
pager.

| Command | Exit | Final line |
|---|---|---|
| `cargo fmt --all --check` | 0 | (no output — the pass condition) |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | `Finished dev profile ... in 11.91s` |
| `cargo test --workspace` | 0 | 39 test binaries, **2,723 passed, 0 failed** |
| `ruby scripts/qa/persistence-dependency.rb` | 0 | `237 driver reference(s) and 515 SQL statement(s) across 43 file(s) outside core` |
| `bash scripts/qa/test-persistence-dependency.sh` | 0 | `20 passed, 0 failed` |
| `ruby scripts/qa/core-boundary.rb` | 0 | `rusqlite: 9 reference(s) across 3 file(s) in core` |
| `ruby scripts/qa/coordination-governance.rb` | 0 | (emits the ledger) |
| `bash scripts/qa/test-persistence-extraction.sh` | 0 | `11 passed, 0 failed, 0 skipped` |
| `ruby scripts/qa/doc-lifecycle.rb` | 0 | `256 carry related_fr across 128 feature request(s)` |
| `bash scripts/qa/test-doc-lifecycle.sh` | 0 | `12 passed, 0 failed` |
| `bash scripts/qa-doc-lint.sh` | 0 | `[qa-doc-lint] PASS` |
| `bash scripts/qa/test-markdown-link-integrity.sh` | 0 | `2 passed, 0 failed` |
| `bash scripts/qa/test-docs-publishing-integrity.sh` | 0 | `7 passed, 0 failed` |
| `bash scripts/qa/test-qa-gate-surface.sh` | 0 | `13 passed, 0 failed` |
| `bash scripts/qa/test-qa-gate-surface.sh --fixture-test` | 0 | `34 passed, 0 failed` |
| `ruby scripts/qa/bash32-compat.rb` | 0 | `PASS (97 shell file(s) scanned, 0 finding(s))` |
| `bash scripts/qa/test-bash32-compat.sh` | 0 | `23 passed, 0 failed, 0 skipped` |
| `ruby scripts/qa/ci-liveness.rb` | 0 | `14 job(s) recorded across 3 in-scope workflow(s); 0 known-failing` |
| `bash scripts/qa/test-ci-liveness.sh` | 0 | `9 passed, 0 failed` |

`cargo test --workspace`'s literal last line reports the final (empty) test binary, so the aggregate
was taken across all `^test result:` lines instead: no line lacked ` 0 failed;` and no line read
`FAILED`.

Six `cert-*.log` files in the scratch directory predated this run (03:39–03:40, the FR-137 session)
and were deleted and regenerated rather than cited. Every log above has an mtime at or after
10:44:23.

`git diff --stat` over `config/governance/schema-snapshot.sql` across the whole FR is empty.
