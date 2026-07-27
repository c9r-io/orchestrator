---
lifecycle: active
related_fr: FR-142
self_referential_safe: true
---

# Orchestrator - Trigger History Limit Cascade

**Module**: Trigger engine / Persistence
**Scope**: that `Trigger.historyLimit` actually deletes a task that ran; that a task it may not
delete is left byte-for-byte intact and reported by cause; that a failure which is not a child row
is not filed as a retention skip; and that the frozen schema is unchanged
**Scenarios**: 5
**Priority**: High

## Background

`historyLimit` had never deleted a task. `trigger_state::delete_tasks` was a bare
`DELETE FROM tasks`; `task_items` references `tasks(id)` without a cascade, foreign keys are
enforced, and every task carries a `task_items` row from creation — so the delete was refused every
time, and the failure was logged at `debug!` under a default filter of `info`. No log line, no
shrinking table. See DD-150.

Each candidate now goes through `task_repository`'s existing cascade. Of the ten tables referencing
`tasks(id)`, two cascade and are removed by SQLite, that cascade clears `task_items` with its
command runs and events, and the remaining seven still refuse — those tasks are skipped whole and
named.

Everything here runs against temporary SQLite files created by the test harness under `$TMPDIR`.
No scenario starts a daemon, writes to `~/.orchestratord/agent_orchestrator.db`, or invokes a
provider. The schema under test is built by `PersistenceBootstrap::ensure_current`, the same
registered migration chain production runs.

Primary entry points:

```bash
cargo test -p orchestrator-persistence --test round_trip     # 20 tests
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

---

## Scenario 2: The defect itself, still reproducible

**Steps**

Same test. Before the sweep it issues the original statement directly against the connection:

```sql
DELETE FROM tasks WHERE id = ?1
```

on a task that has one `task_items` row and nothing else.

**Expected result**

The statement fails with `FOREIGN KEY constraint failed`.

This is the assertion DD-148 pinned, rewritten rather than removed as FR-142 required. It is now
raw SQL because the API no longer makes that statement; what it pins is the reason the API had to
change. If it ever stops failing, the test says so explicitly — the message states that the defect
can no longer be reproduced and the test therefore no longer shows why the cascade is needed.

---

## Scenario 3: A task that may not be deleted is left whole and named

**Steps**

```bash
cargo test -p orchestrator-persistence --test round_trip \
  a_task_the_history_limit_cannot_remove_is_left_whole_and_named
```

Seeds a task with one item, one command run, two events, a `resume_plans` row (one of the seven
references the cascade does not clear) and a `task_graph_runs` row (which cascades, so it must
*not* be reported).

**Expected result**

Passes. `deleted == 0`; `skipped` is exactly one entry whose `blocked_by` is
`["resume_plans.task_id"]` and nothing else; `log_paths` is empty.

Then every seeded count is unchanged: 1 task, 1 item, 2 events, 1 command run. This is the
rollback assertion. The failure it guards against is not the task surviving — it is the task
surviving *stripped*, because the cascade deletes events and command runs before it hits the
foreign key on `DELETE FROM tasks`.

---

## Scenario 4: A failure that is not a child row is not a retention skip

**Steps**

Same test. It sweeps an id that does not exist.

**Expected result**

The call returns `Err`, and the message contains `not a retention skip`.

A missing task is the reachable instance — another writer can remove one between the retention
query and the sweep. The assertion exists because the whole defect being closed was a real failure
wearing a shape nobody looked at, and a skip list that quietly absorbs unrelated failures would
rebuild it one level up.

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
| `items.rs`: replace `conn.unchecked_transaction()` with the bare connection and drop the commit | Scenario 3 | `the refused sweep changed the items of a task it did not delete: left 0, right 1` — the half-emptied task, exactly the state the transaction prevents |
| `trigger_state.rs`: drop `AND UPPER(COALESCE(f.on_delete,'')) <> 'CASCADE'` | Scenario 3 | `blocked_by` became `["resume_plans.task_id", "task_graph_runs.task_id"]`, naming a table that never refused anything |
| `trigger_state.rs`: replace `if blocked_by.is_empty()` with `if false`, filing every failure as a skip | Scenario 4 | `a missing task was absorbed as a skip: SkippedTask { task_id: "no-such-task", blocked_by: [] }` |

The third mutation **survived the first version of this suite**. Scenario 4 was written after it
was observed to survive, not before. Recorded because the gap was in the tests, not in the code,
and the audit that finds such a gap is worth more than the fixture it produces.

---

## Checklist

- [ ] Scenario 1: a task with items, a command run and events is deleted, and the log paths come back
- [ ] Scenario 2: a bare `DELETE FROM tasks` still fails with `FOREIGN KEY constraint failed`
- [ ] Scenario 3: a task pinned by `resume_plans` is skipped, named, and left with every row intact
- [ ] Scenario 3: a cascading `task_graph_runs` row is not reported as a blocker
- [ ] Scenario 4: a missing task surfaces as an error, not as a skip
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

Filled in at closure; see the FR-142 closure commit.
