# CEL Prehook & Finalize Expressions

## Table of Contents
- [Prehook Syntax](#prehook-syntax)
- [Prehook Variables](#prehook-variables)
- [CEL Syntax](#cel-syntax)
- [Common Patterns](#common-patterns)
- [Finalize Rules](#finalize-rules)
- [Finalize-Only Variables](#finalize-only-variables)
- [Default Finalize Rules](#default-finalize-rules)

## Prehook Syntax

```yaml
prehook:
  engine: cel
  when: "is_last_cycle && active_ticket_count > 0"
  reason: "Only fix tickets on final cycle"
```

`when: true` → step runs. `when: false` → step skipped.

## Prehook Variables

### Cycle & Task
| Variable | Type | Description |
|----------|------|-------------|
| `cycle` | int | Current cycle (1-based) |
| `max_cycles` | int | Total configured cycles |
| `is_last_cycle` | bool | cycle == max_cycles |
| `task_id` | string | Task ID |
| `task_item_id` | string | Item ID (empty for task-scoped) |
| `task_status` | string | Task status |
| `item_status` | string | Item status |
| `step` | string | Step ID |

### QA & Tickets
| Variable | Type | Description |
|----------|------|-------------|
| `qa_file_path` | string | QA file path |
| `qa_exit_code` | int? | Last QA exit code (null if not run) |
| `qa_failed` | bool | Last QA failed |
| `active_ticket_count` | int | Active tickets |
| `new_ticket_count` | int | New tickets this cycle |

### Fix & Retest
| Variable | Type | Description |
|----------|------|-------------|
| `fix_exit_code` | int? | Last fix exit code |
| `fix_required` | bool | Fix needed |
| `retest_exit_code` | int? | Last retest exit code |

### Build & Test
| Variable | Type | Description |
|----------|------|-------------|
| `build_exit_code` | int? | Build exit code |
| `test_exit_code` | int? | Test exit code |
| `build_errors` | int | Build error count |
| `test_failures` | int | Test failure count |
| `self_test_exit_code` | int? | Self-test exit code |
| `self_test_passed` | bool | Self-test passed |

### Safety
| Variable | Type | Description |
|----------|------|-------------|
| `self_referential_safe` | bool | Safe for self-referential exec |

## CEL Syntax

```cel
# Comparison
cycle > 1
active_ticket_count == 0

# Logic
is_last_cycle && qa_failed
fix_required || active_ticket_count > 0

# Null check (REQUIRED for optional ints)
qa_exit_code != null && qa_exit_code == 0

# Strings
qa_file_path.startsWith("docs/qa/")
qa_file_path.endsWith(".md")

# Negation
!qa_failed
```

**Critical**: Always null-check optional variables before comparing: `qa_exit_code != null && qa_exit_code == 0`.

## Common Patterns

```yaml
# Defer to last cycle
when: "is_last_cycle"

# Only when tickets exist
when: "active_ticket_count > 0"

# Combined: last cycle + safe files
when: >-
  is_last_cycle && self_referential_safe
  && qa_file_path.startsWith("docs/qa/")
  && qa_file_path.endsWith(".md")

# Confidence gating
when: "qa_confidence != null && qa_confidence < 0.8"

# Build must pass
when: "build_exit_code != null && build_exit_code == 0"
```

## Finalize Rules

```yaml
finalize:
  rules:
    - id: rule_name
      engine: cel
      when: "qa_ran && active_ticket_count == 0"
      status: qa_passed
      reason: "QA passed"
```

Rules evaluated in order; first match wins.

## Finalize-Only Variables

In addition to all prehook variables:

| Variable | Type | Description |
|----------|------|-------------|
| `retest_new_ticket_count` | int | Retest-created tickets |
| `qa_configured` | bool | QA step exists |
| `qa_observed` | bool | QA observed this cycle |
| `qa_enabled` | bool | QA enabled |
| `qa_ran` | bool | QA executed |
| `qa_skipped` | bool | QA skipped by prehook |
| `fix_configured` | bool | Fix step exists |
| `fix_enabled` | bool | Fix enabled |
| `fix_ran` | bool | Fix executed |
| `fix_skipped` | bool | Fix skipped |
| `fix_success` | bool | Fix succeeded |
| `retest_enabled` | bool | Retest enabled |
| `retest_ran` | bool | Retest executed |
| `retest_success` | bool | Retest passed |
| `is_last_cycle` | bool | Final cycle |

## Default Finalize Rules

Applied when no custom rules specified (first match wins):

| # | Rule | Condition | Status |
|---|------|-----------|--------|
| 1 | skip_without_tickets | qa_skipped && tickets==0 && is_last_cycle | skipped |
| 2 | qa_passed_without_tickets | qa_ran && exit==0 && tickets==0 | qa_passed |
| 3 | fix_disabled_with_tickets | !fix_enabled && tickets>0 | unresolved |
| 4 | fix_failed | fix_ran && !fix_success | unresolved |
| 5 | fixed_without_retest | fix_success && !retest_enabled | fixed |
| 6 | fix_skipped_retest_disabled | fix_enabled && !fix_ran && !retest_enabled && tickets>0 | unresolved |
| 7 | fixed_retest_skipped | retest_enabled && !retest_ran && fix_success | fixed |
| 8 | unresolved_no_fix | retest_enabled && !retest_ran && !fix_success && tickets>0 | unresolved |
| 9 | verified | retest_ran && retest_success && retest_tickets==0 | verified |
| 10 | unresolved_after_retest | retest_ran && (!success || tickets>0) | unresolved |
| 11 | fallback_unresolved | tickets>0 | unresolved |
| 12 | fallback_passed | tickets==0 | qa_passed |
