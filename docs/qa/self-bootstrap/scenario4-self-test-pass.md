# Self-Bootstrap Tests - Scenario 4: Self-Test Step Passes (All Three Phases)

**Module**: self-bootstrap  
**Scenario**: Self-Test Step Passes (All Three Phases)  
**Status**: IN PROGRESS  
**Test Date**: 2026-03-05  
**Tester**: QA Bot

---

## Goal
Verify that the `self_test` builtin step executes all three phases successfully and sets pipeline variables correctly.

---

### Preconditions
- ✅ Common Preconditions applied (qa-survival project, self-bootstrap workflow)
- ✅ Codebase is in a clean, compilable state (`cargo check` and `cargo test --lib` pass)
- ✅ `scripts/orchestrator.sh` exists (for manifest validate phase)

### Steps
1. Create and start a task using the `self-bootstrap` workflow
2. Wait for the `self_test` step to execute (after `implement` step)
3. Query the `step_finished` event for `self_test` in the events table
4. Check pipeline variables are set correctly

---

### Current Progress
1. ✅ Binary snapshot tests completed
2. 🔄 Preparing self-test validation scenario
3. ⏳ Will execute self-bootstrap task
4. ⏳ Will verify all three self-test phases pass

---

### Expected Results
- Three `self_test_phase` in-memory events emitted in order (visible in SSE stream):
  1. `{"phase": "cargo_check", "passed": true}`
  2. `{"phase": "cargo_test_lib", "passed": true}`
  3. `{"phase": "manifest_validate", "passed": true}`
- `step_finished` event persisted to SQLite with `{"step": "self_test", "exit_code": 0, "success": true}`
- Pipeline variable `self_test_passed` is `"true"`
- Pipeline variable `self_test_exit_code` is `"0"`
- Task continues to `qa_testing` step (self_test does not block)

---

### Expected Data State
```sql
-- step_finished is persisted to the events table via insert_event()
SELECT json_extract(payload_json, '$.exit_code') AS exit_code,
       json_extract(payload_json, '$.success') AS success
FROM events 
WHERE task_id = '{task_id}' 
  AND event_type = 'step_finished'
  AND json_extract(payload_json, '$.step') = 'self_test';
-- Expected: exit_code=0, success=true
```

---

## Checklist

- [ ] `self_test` step executes after `implement` step
- [ ] All three phases pass: `cargo_check`, `cargo_test_lib`, `manifest_validate`
- [ ] `step_finished` event persisted with `exit_code=0, success=true`
- [ ] Pipeline variable `self_test_passed` = `"true"`
- [ ] Pipeline variable `self_test_exit_code` = `"0"`
- [ ] Task continues to next step after self_test