---
self_referential_safe: false
---
# Prehook Self-Referential Safe Filter

**Module**: orchestrator
**Scope**: Verify that QA docs marked `self_referential_safe: false` are skipped during self-referential execution
**Scenarios**: 3


## Scenario 2: Safe QA doc runs normally in self-referential mode

**Precondition**: Workspace is self-referential; QA doc has `self_referential_safe: true` or no frontmatter

### Steps

1. Start a self-bootstrap task with a safe QA doc in the item list
2. Observe the qa_testing step prehook evaluation
3. Verify the QA test executes normally

### Expected

- QA testing agent is spawned and runs the test
- No `step_skipped` event for this item's qa_testing step


## Related Tests

- Existing unit tests for `parse_qa_doc_self_referential_safe()` in `ticket.rs`
- Existing prehook CEL evaluation tests in `prehook/context.rs`
- FR-034 guard tests in `runner::policy::tests`

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | Safe QA doc runs normally in self-referential mode |

