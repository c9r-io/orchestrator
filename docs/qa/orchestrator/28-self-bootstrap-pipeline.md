# Orchestrator - Self-Bootstrap Workflow (Pipeline & Ticket Flow)

**Module**: orchestrator
**Scope**: Full SDLC pipeline, ticket fix chain, pipeline variables (continuation from doc 26)
**Scenarios**: 5
**Priority**: High

See also: `docs/qa/orchestrator/26-self-bootstrap-workflow.md` for Part 1 (basic workflow, build scenarios, checkpoint, manifest).

---

## Background

Self-bootstrap workflows allow the orchestrator to orchestrate AI agents that
develop its own codebase. The simplified AI native SDLC closed-loop:

```
plan → qa_doc_gen → implement → self_test → self_restart → qa_testing → ticket_fix → align_tests → doc_governance → loop_guard
```

Key design decisions:
- **No separate build/test/lint steps** — these are covered by skill internals
- **ticket_fix is prehook-gated**: only runs when `active_ticket_count > 0`
- **Pipeline variables** flow between steps (`{goal}`, `{plan_output}`, `{source_tree}`, `{diff}`)

Fixture: `fixtures/manifests/bundles/self-bootstrap-test.yaml`

### Common Preconditions

```bash
orchestrator init --force

rm -f fixtures/ticket/auto_*.md

QA_PROJECT="qa-bootstrap-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml
orchestrator qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator qa project create "${QA_PROJECT}" --force
```

> Note: Fixture application is additive. Re-apply the expected fixture and
> recreate the isolated project scaffold instead of clearing global config when
> prior runs leave unrelated workflows behind.

---

## Scenario 6: Full Simplified SDLC Pipeline

### Preconditions
- Common Preconditions applied

### Steps
1. Create task with `sdlc_full_pipeline` workflow

### Expected
- Task status: `completed`
- 8 steps execute in order: plan → qa_doc_gen → implement → self_test → self_restart → qa_testing → align_tests → doc_governance
- ticket_fix is skipped (no active tickets, prehook `active_ticket_count > 0` is false)

### Expected Data State
```sql
SELECT json_extract(payload_json, '$.step') AS step
FROM events WHERE task_id = '{task_id}' AND event_type = 'step_started'
ORDER BY created_at;
-- Expected: plan, qa_doc_gen, implement, self_test, self_restart, qa_testing, align_tests, doc_governance

SELECT COUNT(*) FROM events WHERE task_id = '{task_id}'
  AND event_type = 'step_skipped'
  AND json_extract(payload_json, '$.step') = 'ticket_fix';
-- Expected: 1
```

---

## Scenario 7: QA Testing → Ticket Fix Chain

### Preconditions
- Common Preconditions applied, no ticket files

### Steps
1. Create task with `sdlc_qa_ticket_chain` workflow
   - qa_testing creates a ticket file
   - ticket_fix removes the ticket file

### Expected
- Both `qa_testing` and `ticket_fix` execute (`step_started` events)

---

## Scenario 8: Clean QA Testing → Ticket Fix Skipped

### Preconditions
- Common Preconditions applied, no ticket files

### Steps
1. Create task with `sdlc_ticket_skip` workflow
   - qa_testing succeeds cleanly
   - ticket_fix has prehook: `active_ticket_count > 0`

### Expected
- `qa_testing` executes, `ticket_fix` is skipped (`step_skipped`, `reason: prehook_false`)

---

## Scenario 9: Pipeline Variable Propagation

### Preconditions
- Common Preconditions applied

### Steps
1. Create task with `sdlc_pipeline_vars` workflow
   - align_tests command references `{source_tree}`

### Expected
- plan → implement → align_tests all execute
- `{source_tree}` rendered to actual workspace path

---

## Scenario 10: Align Tests as Safety Net After Implement

### Preconditions
- Common Preconditions applied

### Steps
1. Create task with `sdlc_align_after_implement` workflow

### Expected
- Task status: `completed`
- Steps execute: implement → self_test → [self_restart skipped] → qa_testing → align_tests → doc_governance
- align_tests serves as the build+test+lint safety net (no separate builtin steps needed)

---

## Checklist

| # | Scenario | Status | Date | Tester | Notes |
|---|----------|--------|------|--------|-------|
| 6 | Full Simplified SDLC Pipeline | | | | |
| 7 | QA Testing → Ticket Fix Chain | | | | |
| 8 | Clean QA Testing → Ticket Fix Skipped | | | | |
| 9 | Pipeline Variable Propagation | | | | |
| 10 | Align Tests as Safety Net | | | | |
