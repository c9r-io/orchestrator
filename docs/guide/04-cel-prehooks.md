# 04 - CEL Prehooks

Prehooks are conditional gates on workflow steps. Before a step runs, its prehook CEL expression is evaluated; if it returns `false`, the step is skipped for that cycle or item.

## Prehook Syntax

```yaml
- id: qa_testing
  prehook:
    engine: cel                # only "cel" is supported
    when: "is_last_cycle"      # CEL expression — must evaluate to bool
    reason: "QA deferred to final cycle"   # human-readable explanation (optional)
```

When `when` evaluates to `true`, the step runs. When `false`, the step is skipped and the `reason` is logged.

## Available Variables (Prehook Context)

These variables are available inside prehook `when` expressions:

### Cycle & Task State

| Variable | Type | Description |
|----------|------|-------------|
| `cycle` | `int` | Current cycle number (1-based) |
| `max_cycles` | `int` | Total configured cycles |
| `is_last_cycle` | `bool` | `true` when `cycle == max_cycles` |
| `task_id` | `string` | Current task ID |
| `task_item_id` | `string` | Current item ID (empty for task-scoped steps) |
| `task_status` | `string` | Current task status |
| `item_status` | `string` | Current item status |
| `step` | `string` | Current step ID |

### QA & Ticket State

| Variable | Type | Description |
|----------|------|-------------|
| `qa_file_path` | `string` | Path to the QA file for this item |
| `qa_exit_code` | `int?` | Exit code of the last QA step (`null` if not run) |
| `qa_failed` | `bool` | Whether the last QA step failed |
| `active_ticket_count` | `int` | Number of active (unresolved) tickets |
| `new_ticket_count` | `int` | Tickets created in the current cycle |

### Fix & Retest State

| Variable | Type | Description |
|----------|------|-------------|
| `fix_exit_code` | `int?` | Exit code of the last fix step |
| `fix_required` | `bool` | Whether a fix is needed |
| `retest_exit_code` | `int?` | Exit code of the last retest step |

### Build & Test State

| Variable | Type | Description |
|----------|------|-------------|
| `build_exit_code` | `int?` | Exit code of the last build step |
| `test_exit_code` | `int?` | Exit code of the last test step |
| `build_errors` | `int` | Number of build errors |
| `test_failures` | `int` | Number of test failures |
| `self_test_exit_code` | `int?` | Exit code of the last self_test step |
| `self_test_passed` | `bool` | Whether the last self_test passed |

### Safety

| Variable | Type | Description |
|----------|------|-------------|
| `self_referential_safe` | `bool` | Whether this item is safe for self-referential execution |

## Common Patterns

### Defer to Last Cycle

Run QA only on the final cycle of a multi-cycle workflow:

```yaml
prehook:
  engine: cel
  when: "is_last_cycle"
  reason: "QA deferred to final cycle"
```

### Conditional Fix

Only run fix when there are active tickets:

```yaml
prehook:
  engine: cel
  when: "active_ticket_count > 0"
  reason: "No tickets to fix"
```

### Combined Conditions

Defer QA to last cycle AND filter by safe files:

```yaml
prehook:
  engine: cel
  when: >-
    is_last_cycle
    && self_referential_safe
    && qa_file_path.startsWith("docs/qa/")
    && qa_file_path.endsWith(".md")
  reason: "QA testing deferred to final cycle; skips unsafe docs"
```

### Confidence-Based Gating

Skip fix if QA confidence is high enough:

```yaml
prehook:
  engine: cel
  when: "qa_confidence != null && qa_confidence < 0.8"
  reason: "QA confidence above threshold — no fix needed"
```

### Build Failure Gate

Only run deployment if build succeeded:

```yaml
prehook:
  engine: cel
  when: "build_exit_code != null && build_exit_code == 0"
  reason: "Build must pass before deployment"
```

## CEL Expression Quick Reference

CEL (Common Expression Language) supports standard operations:

```cel
# Comparison
cycle > 1
active_ticket_count == 0

# Logical operators
is_last_cycle && qa_failed
fix_required || active_ticket_count > 0

# Null checks (important for optional values)
qa_exit_code != null && qa_exit_code == 0

# String operations
qa_file_path.startsWith("docs/qa/")
qa_file_path.endsWith(".md")
step == "qa_testing"

# Negation
!qa_failed
!(is_last_cycle && fix_required)
```

**Important**: Optional integer variables (`qa_exit_code`, `fix_exit_code`, etc.) can be `null`. Always null-check before comparing:

```cel
# Wrong — will error if qa_exit_code is null
qa_exit_code == 0

# Correct
qa_exit_code != null && qa_exit_code == 0
```

## Finalize Rules (CEL Context)

Finalize rules use the same CEL engine but with an extended variable set. In addition to the prehook variables above, finalize rules have access to:

| Variable | Type | Description |
|----------|------|-------------|
| `retest_new_ticket_count` | `int` | Tickets created during retest |
| `qa_configured` | `bool` | QA step exists in workflow |
| `qa_observed` | `bool` | QA step was observed in this cycle |
| `qa_enabled` | `bool` | QA step is enabled |
| `qa_ran` | `bool` | QA step actually executed |
| `qa_skipped` | `bool` | QA step was skipped (prehook returned false) |
| `fix_configured` | `bool` | Fix step exists in workflow |
| `fix_enabled` | `bool` | Fix step is enabled |
| `fix_ran` | `bool` | Fix step executed |
| `fix_skipped` | `bool` | Fix step was skipped |
| `fix_success` | `bool` | Fix completed successfully |
| `retest_enabled` | `bool` | Retest step is enabled |
| `retest_ran` | `bool` | Retest executed |
| `retest_success` | `bool` | Retest passed |
| `is_last_cycle` | `bool` | Whether this is the final cycle |

### Default Finalize Rules

If you don't specify custom finalize rules, the engine applies 12 built-in rules in this order (first match wins):

| # | Rule ID | Condition (simplified) | Status |
|---|---------|----------------------|--------|
| 1 | `skip_without_tickets` | `qa_skipped && active_ticket_count == 0 && is_last_cycle` | skipped |
| 2 | `qa_passed_without_tickets` | `qa_ran && qa_exit_code == 0 && active_ticket_count == 0` | qa_passed |
| 3 | `fix_disabled_with_tickets` | `!fix_enabled && active_ticket_count > 0` | unresolved |
| 4 | `fix_failed` | `fix_ran && !fix_success` | unresolved |
| 5 | `fixed_without_retest` | `fix_success && !retest_enabled` | fixed |
| 6 | `fix_skipped_and_retest_disabled` | `fix_enabled && !fix_ran && !retest_enabled && active_ticket_count > 0` | unresolved |
| 7 | `fixed_retest_skipped_after_fix_success` | `retest_enabled && !retest_ran && fix_success` | fixed |
| 8 | `unresolved_retest_skipped_without_fix` | `retest_enabled && !retest_ran && !fix_success && active_ticket_count > 0` | unresolved |
| 9 | `verified_after_retest` | `retest_ran && retest_success && retest_new_ticket_count == 0` | verified |
| 10 | `unresolved_after_retest` | `retest_ran && (!retest_success \|\| retest_new_ticket_count > 0)` | unresolved |
| 11 | `fallback_unresolved_with_tickets` | `active_ticket_count > 0` | unresolved |
| 12 | `fallback_qa_passed` | `active_ticket_count == 0` | qa_passed |

The last two rules are catch-all fallbacks. Custom rules in your workflow's `finalize.rules` replace these defaults entirely.

### Custom Finalize Rules Example

```yaml
finalize:
  rules:
    # QA passed cleanly
    - id: qa_clean_pass
      engine: cel
      when: "qa_ran && active_ticket_count == 0"
      status: qa_passed
      reason: "QA passed with no active tickets"

    # Fix verified by retest
    - id: fix_verified
      engine: cel
      when: "fix_ran && retest_ran && retest_success"
      status: fix_verified
      reason: "Fix applied and verified"

    # QA skipped in non-final cycle — keep pending
    - id: qa_deferred
      engine: cel
      when: "qa_skipped && !is_last_cycle"
      status: pending
      reason: "QA deferred to next cycle"

    # Fallback
    - id: fallback
      engine: cel
      when: "true"
      status: pending
      reason: "No rule matched — keep pending"
```

## Next Steps

- [05 - Advanced Features](05-advanced-features.md) — CRDs, persistent stores, task spawning
- [03 - Workflow Configuration](03-workflow-configuration.md) — step definitions and loop policies
