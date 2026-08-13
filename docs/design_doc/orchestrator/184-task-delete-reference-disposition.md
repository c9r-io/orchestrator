---
lifecycle: active
related_fr: FR-168
---

# DD-184: What a Task Delete Does to the Rows That Reference the Task

**Status**: Released
**Supersedes the cascade scope in**: [DD-85](85-data-lifecycle-governance.md) ("items, runs, events, log files")
**Closes the first known limit in**: [DD-150](150-trigger-history-limit-cascade.md)
**QA**: [222](../../qa/orchestrator/222-task-delete-reference-disposition.md), [188](../../qa/orchestrator/188-trigger-history-limit-cascade.md)

## The problem

Ten tables reference `tasks(id)`. Two declare `ON DELETE CASCADE` and are SQLite's
problem. Of the eight that remain, `delete_task_and_collect_log_paths` cleared exactly
one — `task_items`, with the `command_runs` hanging off it and the `events` rows, which
carry no foreign key at all.

The other **seven refused every delete**, with a bare `FOREIGN KEY constraint failed`
naming nothing:

| Table | Column |
|---|---|
| `handoff_snapshots` | `task_id` |
| `resume_plans` | `task_id` |
| `source_bindings` | `task_id` |
| `resume_executions` | `child_task_id` |
| `source_events` | `routed_task_id` |
| `source_routing_attempts` | `task_id` |
| `source_automation_routes` | `task_id` |

So any task that had used a handoff, a resume plan or source ingest could be removed by
neither `orchestrator task delete` nor the retention sweep. Reproduced per table during
governance: seed one row in each, run the four `DELETE` statements verbatim under
`PRAGMA foreign_keys = ON`, and all seven refuse with `FOREIGN KEY constraint failed
(19)`. Foreign keys are enforced in production at
`crates/orchestrator-persistence/src/sqlite.rs:22`.

This was a **design** gap, not an implementation defect. DD-85 scoped the cascade to
"items, runs, events, log files" and `items.rs` implemented exactly that. DD-150 found
the consequence, reported it, and correctly declined to fix it: deciding what an
operator's explicit delete may destroy is a judgement per table, and it has a wider
blast radius than the FR that found it.

## The ruling

Three dispositions were available. Which are *available* per table is not a matter of
taste — `null-the-reference` requires a nullable column — so nullability was derived
from the schema before anything was chosen. It came back lined up with ownership:

| Table.column | `notnull` | Disposition | Reason |
|---|---|---|---|
| `handoff_snapshots.task_id` | NOT NULL | delete-with-task | the snapshot is *of* that task |
| `resume_plans.task_id` | NOT NULL | delete-with-task | a plan *for* that task |
| `source_bindings.task_id` | NOT NULL | delete-with-task | routes future messages to a task that no longer exists |
| `resume_executions.child_task_id` | nullable | null-the-reference | an audit of an operator action; the column points at *another* task |
| `source_events.routed_task_id` | nullable | null-the-reference | "this event arrived" must survive the task it produced |
| `source_routing_attempts.task_id` | nullable | null-the-reference | inbound audit, same argument |
| `source_automation_routes.task_id` | nullable | null-the-reference | see below |

Every `NOT NULL` reference turned out to belong to a row the task owns; every nullable
one to a record that outlives it. That is not a coincidence worth relying on blindly,
but it is a strong signal that the schema authors had already made this distinction
implicitly, and it is asserted as a property rather than assumed
(`each_ruling_matches_the_nullability_that_justified_it`).

`source_automation_routes` is the case where the two candidate answers differ most.
The row carries a `UNIQUE deterministic_task_id`, the idempotency key for the delivery
that created the task. Deleting the row frees that key, so a replay of the same delivery
would fire a second task; nulling the reference keeps the key and the replay stays
suppressed. The audit argument and the correctness argument point the same way here,
which is why it is not a close call despite looking like one.

### `--force` does not change any of this

`--force` on `task delete` is a confirmation gate. It was not given power over
disposition, and a reference nobody has ruled on refuses the delete with `--force` as
without it. Overloading a flag that currently means "yes, I mean it" with "and destroy
the audit rows too" would make the documented meaning in
[QA 43](../../qa/orchestrator/43-cli-force-gate-audit.md) false, and would put the one
irreversible widening behind the flag people type reflexively.

### Retention and an explicit delete get the same answer

DD-150 ruled that a retention sweep skips a blocked task whole and names it. That
mechanism survives, but it is now reached only by a reference with no recorded ruling.
For the seven, both paths do the same thing, because both go through the same routine
and the routine is where the disposition lives.

The alternative — retention more conservative than an operator — was considered and
rejected. Two behaviours would need two sets of assertions, and the argument for it
("an unattended sweep should destroy less") is already served by the fail-closed
default: the thing an unattended sweep must not do is destroy something nobody ruled
on, and it cannot.

## The shape that matters

The design is an asymmetry, and it is the part to preserve if anything here is
revisited:

- **The set of references is derived from the schema on every delete.** A table added
  later appears on its own. A hand-written list of the seven would be correct today and
  silently short by one the next time somebody adds a table.
- **The set of rulings is written down**, because a disposition is a judgement about
  what a row *means* and no query can infer it.
- **Anything derived but not ruled on is `block-and-report`.** The pair fails closed: a
  new table refuses deletes and names itself until somebody rules on it, rather than
  being silently destroyed by a cascade that never heard of it.

`blocking_references()` and `references_holding()` were not rewritten — they moved from
`trigger_state.rs`, where DD-150 built them, into
`task_repository/references.rs` beside the ruling. The history sweep no longer
recomputes the attribution; it consumes the typed `TaskDeleteBlocked` the delete routine
returns. Two derivations of one fact are two things to keep in agreement.

## Implementation notes

- The refusal is a **precheck**, ahead of any mutation, rather than a rollback. DD-150's
  "skip whole rather than strip" is the reason: a task whose rows are half gone is worse
  than either outcome, and relying on the transaction to undo it makes correctness
  depend on a property the reader has to go and verify.
- `delete_task` in the service layer checks references **before** stopping the task's
  runtime. Previously it stopped first and discovered the refusal afterwards, leaving
  the operator with a task that was neither running nor deleted. The delete re-derives
  the check under its own transaction, so the early check is a courtesy and never the
  authority.
- `cleanup_old_tasks` skips and names rather than aborting the batch. It used to `?` on
  any error, so one undeletable task stopped every task behind it from ever being
  cleaned up — on that run and on every later run, because the sweep reached the same
  row each time.
- `blocking_references` and `references_holding` are `pub(crate)`, not `pub`. They
  take a `Connection`, and FR-141 governs how many public items of this crate demand
  a driver type — the reviewed count is zero, and the first version of this work took
  it to two purely so an integration test could reach the derivation. The assertions
  that need it are unit tests instead. A governed boundary is not a thing to widen for
  a test's convenience; the ledger caught it, which is what the ledger is for.
- The typed error had to be carried across the worker boundary deliberately.
  `tokio_rusqlite::Error::Other` holds a `Box<dyn Error>`, and converting an
  `anyhow::Error` into that box **discards the concrete type**: the message survives and
  `downcast` afterwards fails. Measured, not assumed. `CarriedError` boxes the
  `anyhow::Error` itself so callers can ask *whether* a refusal was a blocking reference
  instead of matching on message text.

## Known limits

- **Frequency is still unmeasured.** Every one of the seven has a production path that
  writes a task reference — derived per column, and three of them acquire it by `UPDATE`
  rather than `INSERT` — so all seven are reachable. How often a real task accumulates
  each of them is not known. The ruling makes this much less pressing than it was under
  DD-150, since all seven are now disposed of rather than refusing.
- **Other parent tables were not audited.** `blocking_references()` is hardcoded to
  `tasks`. Whether project, workspace or agent deletes clear only a subset of *their*
  blocking references is the same question one level over, and nobody has asked it. The
  shape here generalises; the rulings do not.
- **The fail-closed default is currently unreachable in production.** All seven live
  references are ruled on, so `block-and-report` is exercised only by tests that create
  a table. That is the intended steady state, but it does mean the branch operators
  would hit is proven by fixture rather than by use.

## What this cost to get right

The first version of the test suite could not fail. It iterated the shipped disposition
map and asserted the observed behaviour matched the recorded value — so flipping a
ruling flipped the expectation with it. Changing `source_events.routed_task_id` from
null-the-reference to delete-with-task, which silently destroys inbound audit rows,
passed all five tests then present.

The assertion looked derived, and it was: derived from the thing under test. §4.4's rule
is to derive the expected value from the ledger and never restate it — and the trap here
is that **a judgement has no ledger to derive from**. Nothing in the schema or the tree
can be queried for what a row means. Where that is true, the design record *is* the
ledger and the test must restate it, so that changing a ruling takes an edit in two
places and one of them says out loud that a design decision is changing.

The repair was two independent checks rather than one: an echo of the ruling
(`the_recorded_map_is_the_ruling`) and the derived property that justified it
(`each_ruling_matches_the_nullability_that_justified_it`). The mutation now fails both,
with different diagnostics, so the log says which way it broke.

A second instance occurred during fact-gathering and is worth recording because it
produced a confident wrong answer rather than a mystery. The first reproduction harness
reported "delete succeeded" for all seven tables — the opposite of the truth — because
it printed a hardcoded SQL string literal as its result line while the real error went
to stderr and was discarded by `tail -1`.

A third appeared in the test written to prove the *ordering* fix, after both of the
above had been found and written down. `delete_task` checks references before stopping
the runtime, and the first assertion for that was `rendered.contains("left running")` —
a phrase that is a string literal in the error message and would have gone on passing
with the two steps in either order. It now observes `state.running` and the stop flag
directly, and reversing the order fails it with `the refused delete deregistered the
task's runtime, leaving it neither running nor deleted`.

All three share a root: an assertion whose subject is *what actually happened* was
answered by something cheaper that correlates with it. The third is the one worth
dwelling on, because it was written by someone who had just documented the other two.
Knowing the rule is not the same as applying it, and the reliable step is mechanical —
**apply the mutation and watch the test go red**. Each of these survived review and
died to a mutation; none would have been caught by reading the assertion again.
