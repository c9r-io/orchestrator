---
lifecycle: active
related_fr: FR-142
---

# DD-150: Trigger History Limit — Cascade Scope And Skip Reporting

**Status**: Implemented (FR-142)
**Related**: [DD-148](148-persistence-crate-extraction.md) (where the defect was found and pinned),
[DD-147](147-persistence-dependency-chokepoint.md) (the ledger this moves)

## The defect

`Trigger.historyLimit` is a documented configuration field. It had never deleted a single task.

`trigger_state::delete_tasks` was a bare statement:

```rust
&format!("DELETE FROM tasks WHERE id IN ({placeholders})"),
```

`task_items` references `tasks(id)` with no `ON DELETE CASCADE`, foreign keys are enforced
(`sqlite.rs:22` sets the pragma, and `async_database.rs:46` asserts it reads back as 1), so the
statement is refused for any task that has one.

Every task has one. This is the part that decides how bad the defect is, and it was worth deriving
rather than assuming: `resolve_task_targets` has five branches and every one yields at least one
item path — a task workspace gets the implicit item, `Explicit` and `QaDirectoryScan` bail rather
than return empty, `ActiveTickets` substitutes `UNASSIGNED_QA_FILE_PATH`, and `SyntheticAnchor`
returns a constant — and `insert_task_with_items` writes them in the same transaction as the task
row. So a task carries a `task_items` row **from the moment it is created**, before it has run
anything.

The limit was therefore not "ineffective for tasks that ran". It was ineffective, full stop, for
every task in every configuration since the field was introduced.

### And there was no symptom

```rust
if trigger.history_limit.is_some()
    && let Err(e) = cleanup_history(...).await
{
    debug!(trigger = trigger_name, error = %e, "history cleanup failed");
}
```

`crates/daemon/src/main.rs` builds its filter with `EnvFilter::new("info")` as the fallback, so
`debug!` does not reach a default deployment. The symptom was not one line in the log per fire; it
was no line, ever, and a table that quietly never shrank. Two lines above, in the same function,
the enqueue failure is an `error!` — the same function chose two severities for two comparable
failures, and the one it demoted is the one that failed silently.

## What the FR got wrong

FR-142 was filed off the DD-148 finding and four of its claims did not survive rebuilding from the
schema and from execution.

### The repository's cascades are all on this chain

> 全库仅有 2 处 `ON DELETE CASCADE`，都不在这条链上。

Both halves are wrong. `grep -c` counts lines, not occurrences: there are **three** cascade
declarations on two lines, and **all three are on the `tasks(id)` chain** —
`task_graph_runs.task_id → tasks(id)`, `task_graph_snapshots.task_id → tasks(id)`, and
`task_graph_snapshots.graph_run_id → task_graph_runs`. Verified by executing the delete against a
database built from the frozen snapshot: a task carrying both graph rows deletes cleanly and takes
them with it.

So of the ten tables referencing `tasks(id)`, **eight** can refuse a delete, not ten. Reading the
count off the schema text rather than off the engine is what produced the wrong number.

### Neither delete path was complete

> 正确的级联已经存在……这不是缺实现，是两条删除路径中只有一条是对的。

The cascade in `task_repository/items.rs` clears `events` — which carries no foreign key at all, so
it never blocked anything — plus `command_runs` and `task_items`. Of the eight tables that can
refuse, it clears exactly **one**.

The remaining seven are unhandled by either path:

| | |
|---|---|
| handoff/resume | `handoff_snapshots`, `resume_plans`, `resume_executions` |
| source ingest | `source_bindings`, `source_events`, `source_routing_attempts`, `source_automation_routes` |

Reproduced directly: `items.rs`'s exact statement sequence, run against a task carrying one
`handoff_snapshots` row, fails with `FOREIGN KEY constraint failed` and leaves the task in place.

This matters twice. It means reusing the existing cascade — which the FR's requirement 2 demands,
correctly — does not by itself make the limit work for every task; and it means **`task_cleanup.rs`
and `delete_task_impl` carry the same defect**, for the same seven tables. Any task that used a
handoff, a resume plan, or source ingest cannot be deleted by retention cleanup or by
`task delete` either. That is a second, independent failure on the second path, found by this FR
and *not* fixed by it — see Known limits.

### The user documentation states a default that does not exist

`docs/guide/02-resource-model.md` described `historyLimit` as "Max completed tasks to keep per
trigger (default: 5)". There is no default at any layer: `history_limit` is
`Option<TriggerHistoryLimitConfig>` under `#[serde(default)]`, both `successful` and `failed` are
`Option<u32>` defaulting to `None`, and `trigger_config_to_spec` maps `None` through unchanged.
An absent `historyLimit` retains everything forever. The line also described one number where the
field takes two independent ones. Neither error is caused by the cascade defect; both were
standing next to it.

The FR's non-goal "不改变 `history_limit` 的默认值" turns out to be free for a reason it did not
know: there is no default to change.

### The FR's own QA note was stale

It advised that a new gate would need a hand-added `OUTCOMES` line because "FR-137 未闭环". FR-137
closed earlier the same day; the aggregation check now derives that coverage. No new gate was added
here regardless.

## The decision

FR-142 asked for a choice between three shapes. Measured, none is implementable as written.

**B — limited cascade, keeping audit and handoff with orphaned foreign keys.** Not available.
Foreign keys are enforced, so an orphan is not a policy choice the design can make; and
`source_bindings.task_id`, `resume_plans.task_id` and `handoff_snapshots.task_id` are `NOT NULL`,
so the reference cannot even be nulled in place.

**C — delete only tasks with no child rows.** A permanent no-op, for the reason established above:
every task has items from birth, so the set C would delete is always empty. C is not a smaller fix,
it is the current behaviour with better documentation.

**A — full cascade.** Requires extending `items.rs`, which is the cascade `task_cleanup` and
`delete_task_impl` also call, so extending it silently changes what `task delete` destroys. That is
FR-142's own non-goal 3. Writing a second cascade instead violates its requirement 2. A also
demands, right now, an answer to "may a retention limit destroy delivery audit and resume plans",
and the evidence needed to answer it well does not exist yet.

**Chosen: A′ — cascade the execution trace, skip and report the rest.**

Each candidate goes through `task_repository`'s existing cascade. A task still referenced by one of
the seven is skipped **whole** and named in the outcome. Schema unchanged, `items.rs` unchanged,
`task_cleanup` behaviour unchanged, no third delete path. For a cron trigger with no source event
and no handoff — the ordinary case, and the one the field was written for — every task is now
deletable.

A′ is also what makes A answerable later. Its skip log records which of the seven actually occur in
practice, so the decision about whether audit and handoff state may go with a task can be made
against a measured distribution instead of an intuition about nine tables. **The skip log is the
requirements document for the next decision**, which is why it reports causes and not a count.

## Design

### The blocking set is read from the schema

```sql
SELECT m.name, f."from"
  FROM sqlite_master m
  JOIN pragma_foreign_key_list(m.name) f
 WHERE m.type = 'table' AND f."table" = 'tasks'
   AND UPPER(COALESCE(f.on_delete, '')) <> 'CASCADE'
   AND m.name <> 'task_items'
```

A hand-written list of the seven would be correct today and silently short by one the next time
somebody adds a table — the enumeration shape this repository keeps finding, and the reason
`task_graph_*` were miscounted in the first place. `task_items` is the single literal, because it
is the one the cascade clears; it is named here rather than discovered because the fact that
justifies excluding it lives in `items.rs`, not in the schema.

A negative fixture pins the `on_delete` filter specifically: the skipped task is seeded with a
`task_graph_runs` row as well, so a query that dropped the filter reports two blockers instead of
one and fails. Without that row the mutation survives, because a cascading table never reaches the
diagnostic on the deletion path.

### Skipped means untouched

The cascade deletes events and command runs first and hits the foreign key on `DELETE FROM tasks`
last. Without a transaction, a refused sweep would leave the task row standing with its execution
history silently emptied underneath it — worse than the original defect, which at least changed
nothing.

`unchecked_transaction` should already make this hold. It is asserted anyway, by removing the
transaction and observing the result, because "it should already be correct" is precisely the
judgement that keeps being falsified on this surface. Under the mutation the task survives with
zero items where it had one, and the test says so.

### A failure that is not a child row is not a retention decision

If the delete fails and nothing references the task, the error propagates instead of being filed as
a skip. The defect being closed here was a real failure wearing a shape nobody looked at; a skip
list that absorbs unrelated failures rebuilds exactly that one level up. The reachable instance is
a task removed by another writer between the retention query and the sweep, and it now surfaces as
an error saying it was not a retention skip.

This was found by mutation, not by design: neutering the branch left every test green until the
assertion was added.

### Visibility

- The swallowed `debug!` becomes `error!`, matching the enqueue failure two lines above rather than
  sitting three levels below it.
- One `warn!` per skipped task, naming `table.column`.
- One `info!` per sweep that had candidates, with selected, deleted and skipped. A mechanism that
  claims to keep the most recent N should be able to say how many it actually keeps, and it should
  say so when it succeeds, not only when it fails.

Deleted and skipped are separate fields rather than one number because a sweep that deletes nothing
for want of candidates and a sweep that deletes nothing because every candidate is pinned are the
same count and opposite situations.

## Decisions and their alternatives

**Skip whole rather than strip.** The alternative — delete what can be deleted and leave the task —
was rejected outright: a task row whose items and events are gone is a worse state than either
outcome, and it is unreachable only because the cascade is transactional.

**Report causes, not counts.** `skipped=3` is a number with no next action. The seven tables have
genuinely different answers to "may retention destroy this", so the name of the one that blocked is
the fact worth carrying.

**Retire `delete_tasks` rather than keep it beside the new call.** DD-148 recorded what a
compatibility shim costs when nothing is compatible with it. Its only production consumer was
`cleanup_history`.

**Keep the bare-`DELETE` assertion as raw SQL.** The API no longer makes that statement, so the
pinned assertion could not stay as an API call, and FR-142 required it not simply be deleted. It is
now asserted directly against the connection: what is pinned is the reason the API had to change.

## Known limits

- **`task_cleanup.rs` and `delete_task_impl` have the same defect, and this FR does not fix it.**
  Both route through `items.rs`, which clears one of the eight blocking references. Any task that
  used a handoff, a resume plan, or source ingest cannot be removed by retention cleanup or by
  `orchestrator task delete`; the delete fails with `FOREIGN KEY constraint failed`. Extending the
  cascade is a separate decision with a wider blast radius — it changes what an operator's explicit
  `task delete` destroys — and FR-142's non-goal 3 excluded it. Recorded here and in the CHANGELOG
  rather than left to be rediscovered, because it is a defect found by this work and not repaired
  by it. The FR's characterisation of that path as "the correct one" is withdrawn.
- **No test asserts which of the seven occur for real trigger tasks.** The skip path is asserted
  with `resume_plans` because it is the cheapest to seed. Whether cron-fired tasks ever accumulate
  handoff or source rows in practice is exactly what the skip log exists to measure, and it has not
  been measured yet.
- **Log file unlinking is best-effort and unasserted.** `cleanup_history` unlinks the returned
  paths and ignores failures, matching `task_cleanup`. The paths are asserted to come back from the
  sweep; that they are then removed from disk is not, and a path that fails to unlink leaves an
  orphan file with no record.
- **`events` has no foreign key.** It is cleared by the cascade for correctness, not because it
  ever blocked anything. A task deleted by any path that forgot `events` would leave orphan rows
  that nothing in the schema would catch. Not introduced here; noted because the eight/ten
  arithmetic above will read as complete otherwise.
- **The composite of "task graph rows cascade" is asserted only through this path.** If a migration
  later dropped `ON DELETE CASCADE` from `task_graph_runs`, the discovery query would pick it up as
  a blocker and the behaviour would silently become "tasks with graphs are skipped" — correct, but
  the assertion that they cascade would then fail loudly, which is the intended direction.
