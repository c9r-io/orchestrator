# Design Doc 66: Incremental Item Progress (FR-054)

## Problem

For long-running tasks (e.g. full-qa with 132 items), `orchestrator task info` shows `Progress: 0/132` throughout the entire execution. The `finalize_items()` batch call only runs after **all** segments complete, meaning item terminal statuses are not written to the database until the very end. Users receive no progress feedback for 100+ minutes.

## Solution

Two complementary layers:

### Data Layer — Per-item incremental finalize

After each item completes its execution in the last item-scope segment, `finalize_item_execution()` is called immediately (both in the sequential and parallel execution paths of `execute_item_segment`). This writes the terminal status (`qa_passed`, `unresolved`, etc.) to the database right away, causing `Progress: X/N` to increment in real-time.

The existing batch `finalize_items()` call at the end of all segments is preserved as a fallback. Since `finalize_item_execution()` is idempotent, the batch re-evaluation produces the same result.

**Key design decision**: The batch finalize is NOT skipped for incrementally-finalized items. This avoids a subtle issue where `acc.terminal` can be set by `is_execution_hard_failure()` (in `apply.rs`) without a corresponding DB write, which would cause the skip to silently drop the finalize for error-path items.

### Display Layer — Step-level progress in CLI

The `task info` table output now shows per-step run statistics below the Progress line:

```
Progress: 12/132 items
    qa_testing:          21 completed, 4 running
    doc_governance:       0 completed
Failed: 3
```

JSON/YAML output includes a `step_progress` array with the same aggregation.

## Modified Files

| File | Change |
|------|--------|
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs` | Incremental `finalize_item_execution()` in sequential and parallel paths |
| `crates/cli/src/output/task_detail.rs` | Step-level progress in table output |
| `crates/cli/src/output/value.rs` | `step_progress` field in JSON/YAML output |

## Verification

- `cargo build` passes
- `cargo test` passes (all integration tests including `workflow_failing_step`)
- Manual: create a multi-item task, check `task info` shows incrementing progress and step-level stats
