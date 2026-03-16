# QA 109b: Parallel Spawn Stagger Delay — Compatibility (FR-055)

Split from doc 109: unknown-field warning compatibility check.

## Scenario 1: No unknown-field warning

**Steps**:
1. Create workflow YAML with `stagger_delay_ms` field
2. Load and validate the workflow

**Expected**: No unknown-field warning (FR-051) for `stagger_delay_ms`.

## Checklist

| # | Check | Status |
|---|-------|--------|
| 1 | All scenarios verified against implementation | ☑ |
