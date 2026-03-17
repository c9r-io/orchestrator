# Design Doc 70: Scenario-Level Self-Referential Safety

**FR**: FR-058
**Status**: Implemented
**Date**: 2026-03-17

## Problem

Full-QA regression testing runs self-referentially: the orchestrator's QA agent executes QA docs on its own daemon. To prevent dangerous operations (kill daemon, cargo build), 115 of 138 QA docs were marked `self_referential_safe: false` and skipped entirely. This left only 20 docs (14.5%) executable, rendering full-QA coverage meaningless.

However, most unsafe docs contain a mix of safe (read-only) and unsafe (mutating) scenarios. Skipping an entire doc because one scenario runs `cargo test` discards all the read-only verification scenarios.

## Solution: Scenario-Level Frontmatter Whitelist

### Data Flow

```
QA doc frontmatter:
  self_referential_safe: false
  self_referential_safe_scenarios: [S2, S3, S5]
    → ticket.rs: parse_qa_doc_safe_scenarios()
      → accumulator.rs: populates StepPrehookContext.self_referential_safe_scenarios
        → prehook/context.rs: adds `self_referential_safe_scenarios` CEL list variable
          → workflow prehook: `self_referential_safe || size(self_referential_safe_scenarios) > 0`
            → dispatch.rs: global safety check allows partially-safe docs through
```

### Changes

1. **`core/src/ticket.rs`** — Added `parse_qa_doc_safe_scenarios()` and `get_self_referential_safe_scenarios()` to parse YAML inline list from frontmatter
2. **`crates/orchestrator-config/src/config/execution.rs`** — Added `self_referential_safe_scenarios: Vec<String>` field to `StepPrehookContext`
3. **`crates/orchestrator-scheduler/src/scheduler/item_executor/accumulator.rs`** — Populates the new field from the QA doc frontmatter
4. **`crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch.rs`** — Modified global safety check to allow docs with non-empty `self_referential_safe_scenarios` through; propagated field in adaptive prehook context
5. **`core/src/prehook/context.rs`** — Exposed `self_referential_safe_scenarios` as a CEL list variable
6. **`docs/workflow/full-qa.yaml`** and **`docs/workflow/self-bootstrap.yaml`** — Updated prehook expressions: `self_referential_safe || size(self_referential_safe_scenarios) > 0`
7. **`qa_testing` StepTemplate prompt** — Added instruction for the QA agent to only execute scenarios listed in `self_referential_safe_scenarios` frontmatter, skipping unsafe scenarios

### Design Decisions

- **Frontmatter over inline markers**: Chose frontmatter-level `self_referential_safe_scenarios` (方案 A) over per-scenario inline comments (方案 B), keeping the annotation centralized and machine-parseable.
- **Additive to existing mechanism**: `self_referential_safe: false` remains unchanged for backward compatibility. The new field is purely additive — docs without it behave exactly as before.
- **CEL list variable**: Exposed as `Vec<String>` directly to CEL (same pattern as `regression_target_ids`), enabling `size()` and `in` operators.
- **Global safety check relaxed**: The dispatch-level global skip now checks both `is_self_referential_safe()` AND `get_self_referential_safe_scenarios().is_empty()` before skipping.

### Coverage Recovery Results

| Phase | Action | Docs Recovered | Total Safe |
|-------|--------|---------------|------------|
| Phase 1 | Recover 4 mismarked docs | +4 | 24 |
| Phase 2 | Recover guide-alignment.md + scenario-level infra | +1 | 25 (doc-level) |
| Phase 3 | Annotate 32 docs with safe scenarios | +32 | 56 (24 + 32) |
| **Total** | | | **56/138 (40%)** |

### Frontmatter Format

```yaml
---
self_referential_safe: false
self_referential_safe_scenarios: [S2, S3, S5]
---
```

The QA agent reads this frontmatter and restricts execution to only the listed scenarios when running in self-referential mode.
