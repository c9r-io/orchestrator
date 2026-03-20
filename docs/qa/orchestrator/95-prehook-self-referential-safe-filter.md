---
self_referential_safe: true
---
# Prehook Self-Referential Safe Filter

**Module**: orchestrator
**Verified**: 2026-03-18
**Scope**: Verify that QA docs marked `self_referential_safe: false` are skipped during self-referential execution
**Scenarios**: 1


## Scenario 2: Safe QA doc runs normally in self-referential mode

**Precondition**: Unit tests pass (`cargo test --workspace --lib`)

### Goal

Verify that the prehook evaluation logic correctly distinguishes safe vs unsafe QA docs during self-referential execution.

### Steps

1. **Code review** — confirm `parse_qa_doc_self_referential_safe()` in `ticket.rs` correctly parses frontmatter:
   - Returns `true` when `self_referential_safe: true`
   - Returns `false` when `self_referential_safe: false`
   - Returns `true` (default) when no frontmatter is present

2. **Code review** — confirm prehook CEL evaluation in `prehook/context.rs` exposes the parsed flag to the CEL expression engine so prehook rules can filter on it.

3. **Code review** — confirm FR-034 guard in `runner::policy::tests` enforces skip behavior for unsafe docs during self-referential execution.

4. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- parse_qa_doc_self_referential_safe
   cargo test --workspace --lib -- prehook
   cargo test --workspace --lib -- policy
   ```

### Expected

- `parse_qa_doc_self_referential_safe()` unit tests pass — frontmatter parsing works correctly
- Prehook CEL evaluation tests pass — flag is available in the expression context
- FR-034 guard tests pass — unsafe docs are skipped, safe docs execute normally
- No `step_skipped` event for safe items; `step_skipped` for unsafe items


## Related Tests

- Existing unit tests for `parse_qa_doc_self_referential_safe()` in `ticket.rs`
- Existing prehook CEL evaluation tests in `prehook/context.rs`
- FR-034 guard tests in `runner::policy::tests`

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ✅ | Safe QA doc runs normally in self-referential mode |
| 2 | `parse_qa_doc_self_referential_safe()` unit tests pass (7 tests) | ✅ | All pass |
| 3 | Prehook CEL evaluation exposes `self_referential_safe` flag | ✅ | All pass (175 prehook tests, 7 dedicated) |
| 4 | Policy tests pass (57 tests) | ✅ | All pass |
