---
lifecycle: active
related_fr: FR-168
self_referential_safe: true
---

# Orchestrator - Task Delete Reference Disposition

**Module**: Persistence / Task lifecycle
**Scope**: that every reference to a deleted task is disposed of by a recorded ruling rather than
refusing the delete; that owned rows are destroyed and audit rows are kept with a null reference,
and that those two outcomes are told apart; that a reference nobody has ruled on refuses the delete
and names itself; that the blocking set is derived from the schema at runtime; that no `events` row
outlives the task it names; and that retention and an explicit delete give the same answer
**Scenarios**: 5
**Priority**: High

## Background

Ten tables reference `tasks(id)`. Two declare `ON DELETE CASCADE`. Of the eight that remain the
delete routine cleared exactly one — `task_items`, with its `command_runs` and the
foreign-key-less `events` rows — so the other **seven refused every delete**, with a bare
`FOREIGN KEY constraint failed` naming nothing. Any task that had used a handoff, a resume plan or
source ingest could be removed by neither `orchestrator task delete` nor retention. DD-150 reported
this and declined to fix it: deciding what an operator's delete may destroy is a per-table
judgement, not an implementation detail.

FR-168 makes that judgement. The ruling and its reasons are in
[DD-184](../../design_doc/orchestrator/184-task-delete-reference-disposition.md); the shape worth
knowing here is which half is derived and which is written down:

- the **set of references** is derived from the schema on every delete, so a table added later
  appears on its own;
- the **set of rulings** is written down, because no query can infer what a row means;
- anything derived but **not** ruled on is `block-and-report`, so the pair fails closed.

Everything below runs against temporary SQLite files created by the test harness under `$TMPDIR`.
No scenario starts a daemon, writes to `~/.orchestratord/agent_orchestrator.db`, or invokes a
provider. The schema under test is built by `PersistenceBootstrap::ensure_current`, the same
registered migration chain production runs.

Primary entry points:

```bash
cargo test -p orchestrator-persistence --test task_delete_disposition   # 7 tests
cargo test -p orchestrator-persistence --test round_trip                # 20 tests
cargo test -p agent-orchestrator --lib task_cleanup                     # 9 tests
cargo test -p agent-orchestrator --lib history_cleanup_visibility
cargo test -p orchestrator-scheduler --lib service::task           # 7 tests
```

---

## Scenario 1: Each reference is disposed of as ruled, and the two outcomes differ

**Steps**

```bash
cargo test -p orchestrator-persistence --test task_delete_disposition \
  each_reference_is_disposed_of_as_ruled
cargo test -p orchestrator-scheduler --lib \
  service::task::tests::a_task_with_a_handoff_deletes_through_the_service_path
```

The first seeds a task plus one row in each of the seven referencing tables, one table per
iteration, and deletes the task through the repository. The second is the acceptance criterion as
written — a task carrying a `handoff_snapshots` row, deleted through `delete_task`, the path an
operator reaches — because the service layer has its own reference check and its own ordering, and
"the repository does the right thing" is a different claim from "the operator gets it".

**Expected result**

Passes. For each of the seven the task is gone, and then the outcome the ruling names:

| Table.column | Expected after delete |
|---|---|
| `handoff_snapshots.task_id` | row gone |
| `resume_plans.task_id` | row gone |
| `source_bindings.task_id` | row gone |
| `resume_executions.child_task_id` | row present, column `NULL` |
| `source_events.routed_task_id` | row present, column `NULL` |
| `source_routing_attempts.task_id` | row present, column `NULL` |
| `source_automation_routes.task_id` | row present, column `NULL` |

The assertion is deliberately not "the delete succeeded". A cascade that destroyed all seven tables
satisfies that and also destroys the record that an inbound event ever arrived. The difference
between the two columns above is the entire content of the ruling.

---

## Scenario 2: The ruling is pinned, matches its justification, and stays live

**Steps**

```bash
cargo test -p orchestrator-persistence --test task_delete_disposition \
  the_recorded_map_is_the_ruling
cargo test -p orchestrator-persistence --test task_delete_disposition \
  each_ruling_matches_the_nullability_that_justified_it
cargo test -p orchestrator-persistence --test task_delete_disposition \
  no_ruling_names_a_reference_that_no_longer_exists
```

**Expected result**

All three pass. The first compares the shipped map against a restatement of the seven rulings held
in the test file; the second derives each column's nullability from the live schema and checks it
against the disposition — `NOT NULL` with delete-with-task, nullable with null-the-reference; the
third checks the map against the schema in both directions.

**On the third.** A ruling whose table or column was renamed or dropped matches nothing: it changes
no behaviour, produces no diagnostic and appears in no log, while the reference it governed
silently reverts to refusing every delete. The map's own lookup cannot notice — it returns the
fail-closed default and cannot tell "nobody ruled on this" from "somebody ruled on a name that no
longer exists". The converse half — every live reference has a ruling — is *allowed* to fail when
somebody adds a table; that is the design, and it should fail here rather than in front of an
operator whose delete stopped working.

**Why the first two.** Scenario 1 alone cannot fail. Its first version iterated the shipped map and asserted
the behaviour matched the recorded value, so flipping a ruling flipped the expectation with it:
changing `source_events.routed_task_id` to delete-with-task — which silently destroys inbound
audit rows — passed every test in this document. Restating a value the code already holds is
normally the wrong shape, but there is no ledger to derive a judgement from; DD-184 is the ledger
and the restatement is its mechanical echo. The nullability check is the derived half, and it
catches the case the echo cannot: a ruling changed in both places by somebody who did not notice
that null-the-reference cannot work on a `NOT NULL` column.

---

## Scenario 3: A reference nobody has ruled on refuses the delete and names itself

**Steps**

```bash
cargo test -p orchestrator-persistence --test task_delete_disposition \
  an_unruled_reference_refuses_the_delete_and_names_itself
cargo test -p orchestrator-scheduler --lib \
  service::task::tests::an_unruled_reference_refuses_before_the_runtime_is_stopped
```

Both create a table referencing `tasks(id)` with no cascade **inside the test**, pin the task with
it, and attempt the delete. The second additionally registers the task as running first, so that
stopping it is observable.

**Expected result**

Passes. The table appears in `blocking_references()` without anything being told about it; the
delete fails with a `TaskDeleteBlocked` naming `later_addition.task_id`; the rendered message
contains that string; and both the task and the pinning row are still present, because the refusal
happens before anything is mutated rather than being rolled back after.

The table is created rather than named from the schema on purpose. No table in the tree is
currently unruled, so a fixture naming one would break the moment somebody ruled on it. Creating
one also asserts the stronger property: a table that did not exist when the delete routine was
written is picked up anyway, which is the line between this design and a hand-written list of
seven.

The assertion is the **diagnostic**, not the exit status. A delete that merely failed cannot be
told apart from a disk error, and the operator learning which table holds the task is the whole
requirement.

The service-layer half also asserts the **ordering**: the task is still registered in
`state.running` and its stop flag is unraised, so the refusal happened before the runtime was
touched. `delete_task` used to stop first and discover the refusal afterwards, leaving the operator
with a task that was neither running nor deleted. That is observed through the runtime registration
rather than through the error's wording — an earlier version of this test checked that the message
contained "left running", which is a string literal in the error and would have passed with the two
steps in either order.

---

## Scenario 4: No `events` row outlives the task it names

**Steps**

```bash
cargo test -p orchestrator-persistence --test task_delete_disposition \
  no_event_outlives_the_task_it_names
```

Seeds two tasks, three `events` rows across them, and one reference of each disposition, then
deletes one task.

**Expected result**

Passes. No `events` row has a `task_id` absent from `tasks`, and the surviving task keeps its
event.

`events` carries no foreign key, so nothing in the schema would ever catch an orphan and no
constraint failure would be raised. The assertion is the closure property over the whole table
rather than the spelling of any one `DELETE`: a routine that dropped the events statement still
contains the word `events` and still passes a text check.

---

## Scenario 5: Retention and an explicit delete agree, and one bad task does not stop the sweep

**Steps**

```bash
cargo test -p agent-orchestrator --lib task_cleanup::tests::retention_skips_the_unruled_and_cleans_the_rest
cargo test -p agent-orchestrator --lib task_cleanup::tests::retention_and_explicit_delete_agree_on_the_same_fixture
```

**Expected result**

Both pass. The first seeds three aged tasks with the pinned one **first**, so it is the first row
the sweep reaches: two tasks are deleted, the pinned one survives, and the two behind it are gone.
Before FR-168 the batch died on the first task and everything behind it was never cleaned up on
that run or any later one, because the sweep hit the same row every time. The ordering is the
fixture — with the pinned task last, an aborting sweep and a skipping sweep return the same count.

The second builds the same fixture twice and drives it through both paths. Both leave the task
standing and the explicit refusal names `later_addition.task_id`. Asserted as observed behaviour on
one fixture rather than by checking that both call the same routine: a call-graph assertion passes
on two paths that share a routine and then disagree about what to do with its error, which is
exactly the state this FR found.

---

## Mutation Evidence

Each fixture was checked against a mutation chosen to be one the implementation would *not*
obviously catch, rather than the deletion its author had in mind.

| Mutation | Caught by | Diagnostic |
|---|---|---|
| `items.rs`: remove the `DELETE FROM events` statement | Scenario 4 | `an events row outlived the task it names, and no constraint would ever say so`, left 2 |
| `references.rs`: flip `source_events.routed_task_id` to delete-with-task | Scenario 2 (two of its three) | `no longer carries the disposition FR-168 ruled for it` and `is nullable but ruled delete-with-task` — two independent diagnostics, so the log says which way it broke |
| `service/task.rs`: move `stop_task_runtime_for_delete` back ahead of the reference check | Scenario 3 | `the refused delete deregistered the task's runtime, leaving it neither running nor deleted` |

The second mutation **survived the first version of this suite entirely**, passing all five tests
then present. That version had no Scenario 2: it iterated the shipped map and asserted the
behaviour agreed with it, which is a statement the code cannot violate. Scenario 2 was written in
response and both of its halves now fail on that mutation. Recorded here because the failure is the
interesting part: the tautology was written by somebody who had just finished reading §4.4 about
exactly this, and it looked like a derived assertion — it *was* derived, from the wrong source.

## Checklist

- [ ] Scenario 1: all seven references disposed of as ruled, with owned rows gone and audit rows
      kept with a null column
- [ ] Scenario 1: a task carrying a handoff deletes through the operator's service path
- [ ] Scenario 2: the shipped map matches the FR-168 ruling, each ruling matches the nullability
      that justified it, and every ruling names a live reference while every live reference has a
      ruling
- [ ] Scenario 3: an unruled reference refuses the delete, names itself, and leaves the task and
      the pinning row untouched
- [ ] Scenario 3: the refusal happens before the task's runtime is stopped, observed through
      `state.running` rather than through the error's wording
- [ ] Scenario 3: the blocking set includes a table created after the delete routine was written
- [ ] Scenario 4: no `events` row references a deleted task, and an unrelated task keeps its events
- [ ] Scenario 5: a sweep gets past a task it cannot delete, and both delete paths agree on one
      fixture
