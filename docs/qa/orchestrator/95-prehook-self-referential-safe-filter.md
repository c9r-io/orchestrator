# Prehook Self-Referential Safe Filter

**Module**: orchestrator
**Scope**: Verify that QA docs marked `self_referential_safe: false` are skipped during self-referential execution
**Scenarios**: 3

---

## Scenario 1: Unsafe QA doc is skipped in self-referential mode

**Precondition**: Workspace is self-referential; QA doc has `self_referential_safe: false` frontmatter

### Steps

1. Start a self-bootstrap task with `53-client-server-architecture.md` in the QA item list
2. Observe the qa_testing step prehook evaluation for this item
3. Verify the item is skipped with a `step_skipped` event

### Expected

- `step_skipped` event recorded for qa_testing on this item
- No agent process spawned for this QA doc
- Daemon process remains alive throughout execution

---

## Scenario 2: Safe QA doc runs normally in self-referential mode

**Precondition**: Workspace is self-referential; QA doc has `self_referential_safe: true` or no frontmatter

### Steps

1. Start a self-bootstrap task with a safe QA doc in the item list
2. Observe the qa_testing step prehook evaluation
3. Verify the QA test executes normally

### Expected

- QA testing agent is spawned and runs the test
- No `step_skipped` event for this item's qa_testing step

---

## Scenario 3: All QA docs run in non-self-referential mode

**Precondition**: Workspace is NOT self-referential (`self_referential: false`)

### Steps

1. Start a task with QA docs including ones marked `self_referential_safe: false`
2. Observe that `is_self_referential_safe()` returns `true` (because workspace is non-self-referential)
3. Verify all QA docs pass the prehook filter

### Expected

- All QA docs execute regardless of `self_referential_safe` frontmatter
- No items incorrectly skipped

---

## Related Tests

- Existing unit tests for `parse_qa_doc_self_referential_safe()` in `ticket.rs`
- Existing prehook CEL evaluation tests in `prehook/context.rs`
- FR-034 guard tests in `runner::policy::tests`

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
