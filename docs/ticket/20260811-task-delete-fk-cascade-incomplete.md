# Task deletion clears 1 of 8 blocking FK references — delete and retention fail on real tasks

- **Observed during**: 2026-08-11 product analysis (declared-debt sweep); the defect is *self-reported* in DD-150's Known limits and knowingly unfixed
- **Severity**: high (user-visible: `orchestrator task delete` and retention cleanup fail with `FOREIGN KEY constraint failed` on any task that used a handoff, a resume plan, or source ingest)
- **Symptom**: per `docs/design_doc/orchestrator/150-*.md:226` (verbatim): "Any task that used a handoff, a resume plan, or source ingest cannot be removed by retention cleanup or by `orchestrator task delete`; the delete fails with `FOREIGN KEY constraint failed`."
- **Status**: open

## Mechanism (from DD-150, re-verify at fix time)

`task_cleanup.rs` and `delete_task_impl` both route through `items.rs`, which
clears **1 of 8** blocking FK references. Additionally `events` has no FK and
is cleared for correctness only — "a task deleted by any path that forgot
`events` would leave orphan rows that nothing in the schema would catch."

## For ticket-fix

1. Reproduce: create a task, attach a handoff (`orchestrator handoff ...`),
   `task delete` → expect the FK failure.
2. Likely classification: Bug (the delete path deviates from its own intent;
   DD-150 parks it as a known limit, not as design). Fix = enumerate the
   8 references **derived from the schema** (not hand-listed — §4.4 shape 2;
   consider a test that walks sqlite `foreign_key_list` against every table
   referencing `tasks` and asserts the delete path covers the set), plus an
   orphan-events assertion.
3. Retention sweep needs the same coverage; one shared clearing routine.
