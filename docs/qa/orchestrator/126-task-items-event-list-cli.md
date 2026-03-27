# QA 126: Task Items & Event List CLI Commands

## FR Reference

FR-078

## Verification Scenarios

### Scenario 1: task items — table output

**Steps:**
1. Create and run a task with multiple items
2. `orchestrator task items <task_id>`

**Expected:** Table with ORDER, LABEL, STATUS, FIXED columns. All items listed.

### Scenario 2: task items — status filter

**Steps:**
1. `orchestrator task items <task_id> --status running`

**Expected:** Only items with status "running" shown.

### Scenario 3: task items — JSON output

**Steps:**
1. `orchestrator task items <task_id> -o json`

**Expected:** JSON array with id, label, status, order_no, fix_required, fixed, last_error, started_at, completed_at fields.

### Scenario 4: event list — basic

**Steps:**
1. `orchestrator event list --task <task_id>`

**Expected:** Table with ID, TYPE, PAYLOAD, CREATED columns. Default 50 events, newest first.

### Scenario 5: event list — type filter

**Steps:**
1. `orchestrator event list --task <task_id> --type step_skipped`

**Expected:** Only events with type starting with "step_skipped".

### Scenario 6: event list — limit and JSON

**Steps:**
1. `orchestrator event list --task <task_id> --limit 5 -o json`

**Expected:** JSON array with at most 5 events, each having id, event_type, task_item_id, payload (parsed), created_at.

### Scenario 7: event list — self_restart filter

**Steps:**
1. `orchestrator event list --task <task_id> --type self_restart`

**Expected:** Only self_restart* events shown (prefix match).

### Scenario 8: showcases free of sqlite workarounds

**Steps:**
1. `grep -rn sqlite3 docs/showcases/ | grep -v manual-testing`

**Expected:** Zero results — all showcase sqlite queries replaced with CLI commands (except orchestrator-usage-manual-testing.md which uses command_runs for framework testing).

### Scenario 9: compilation and tests

**Steps:**
1. `cargo test --workspace`

**Expected:** All tests pass.

## Checklist

- [ ] S1: task items -- table output
- [ ] S2: task items -- status filter
- [ ] S3: task items -- JSON output
- [ ] S4: event list -- basic
- [ ] S5: event list -- type filter
- [ ] S6: event list -- limit and JSON
- [ ] S7: event list -- self_restart filter
- [ ] S8: showcases free of sqlite workarounds
- [ ] S9: compilation and tests
