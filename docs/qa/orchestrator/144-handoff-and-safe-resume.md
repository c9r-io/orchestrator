---
self_referential_safe: true
---

# Orchestrator - Handoff And Safe Resume

**Module**: Orchestrator  
**Scope**: Immutable handoffs, logical boundaries, stale-safe consequence planning/execution, provider opacity, RBAC, and task-detail entry  
**Scenarios**: 5  
**Priority**: High

> The original resume safety scenarios remain authoritative for backend behavior. Dialog focus lifecycle closure is maintained by [DD-132](../../design_doc/orchestrator/132-handoff-dialog-focus-lifecycle.md) and [QA-170](170-handoff-dialog-focus-lifecycle.md).

---

## Background

Handoff and safe resume provide a deterministic evidence briefing and a two-stage recovery path. Planning is non-mutating; execution requires the reviewed plan ID, exact task state version, operator reason, and idempotency key. Logical boundaries never invoke source-control rollback.

The automated QA uses only this deterministic fixture:

```bash
orchestrator apply -f fixtures/manifests/bundles/handoff-safe-resume.yaml --project qa-handoff-safe-resume
```

`./scripts/qa/test-handoff-safe-resume.sh` starts an isolated daemon on `127.0.0.1:19197` with temporary HOME/data/workspace directories and no real AI provider.

## Database Schema Reference

| Table | Purpose |
|---|---|
| `handoff_snapshots` | Immutable canonical briefing and cursor/hash/version evidence |
| `resume_plans` | Expiring consequence preview and expected state version |
| `resume_executions` | Idempotent execution projection linked to FR-101 canonical audit by `request_id` |
| `tasks` | Source/child correlation, step filter, variables, and opaque command-run reference |
| `events` | Source evidence and post-mutation `resume_executed` audit with promoted `request_id` |

---

## Scenario 1: Visible Handoff Entry And Deterministic Briefing

### Preconditions

- Build `cargo build -p orchestratord -p orchestrator-cli -p orchestrator-gui`.
- Run `cd gui && npm ci && npm run build`.
- Apply `fixtures/manifests/bundles/handoff-safe-resume.yaml` only.
- A failed `handoff_failure` task exists.

### Goal

Verify operators discover the feature through normal Process Workspace navigation and repeated same-cursor generation is immutable, bounded, and redacted.

### Entry Visibility

The feature must be reachable from Processes → Process Workspace; a direct hidden route or Expert-only toggle is not an acceptable substitute.

### Steps

1. Open Processes, select the failed task, and verify the "Handoff & safe resume" panel appears in the Process Workspace contextual rail.
2. Select "Generate handoff" and inspect current state, failure, changed files, recommendations, snapshot digest, and evidence count.
3. Run `./scripts/qa/test-handoff-safe-resume.sh` and confirm both handoff assertions pass.
4. Generate twice with `orchestrator handoff generate {task_id} --cursor {cursor} -o json`.
5. Search the UI response, CLI response, and daemon log for the injected provider token.

### Expected

- The entry is visible without a direct URL, hidden expert mode, or Attention action.
- Same cursor returns the same `id` and `content_hash`.
- Briefing includes goal/current state, failure/test evidence, `src/lib.rs`, and deterministic recommendations.
- Transcript bodies, raw stdout/stderr, and provider token values are absent.

### Expected Data State

```sql
SELECT task_id, source_event_cursor, content_hash, COUNT(*) AS snapshots
FROM handoff_snapshots
WHERE task_id = '{task_id}'
GROUP BY task_id, source_event_cursor, content_hash;
-- Expected: snapshots = 1 for repeated generation at the same cursor/hash
```

---

## Scenario 2: Logical Boundaries And Non-idempotent Default Denial

### Preconditions

- Apply `fixtures/manifests/bundles/handoff-safe-resume.yaml` to the isolated project.
- `handoff_failure` and `handoff_external_unknown` tasks exist.
- Project policy has `mutating_resume_enabled: true` and `elevated_resume_enabled: false`.

### Goal

Verify declared workspace-only work is replay-safe while undeclared agent/external behavior fails closed.

### Steps

1. Run `orchestrator resume boundaries {safe_task_id} -o json`.
2. Verify step `qa` is `workspace_only` and `replay_safe: true`.
3. Run `orchestrator resume boundaries {external_task_id} -o json` and create a `restart_from_boundary` plan.
4. Execute it with `--elevated-confirmation` while elevated project policy remains disabled.
5. Attempt `resume_provider_session` for a boundary without a provider session.

### Expected

- Boundary IDs are stable for the same task/cycle/step/item/state.
- Undeclared external/agent replay is `non_idempotent_external` and not replay-safe.
- Confirmation alone cannot override disabled policy; execution is denied without task/workspace mutation.
- Missing provider session returns an explicit `restart_from_boundary`/new-session fallback.

### Expected Data State

```sql
SELECT status, COUNT(*)
FROM resume_executions
WHERE plan_id = '{unsafe_plan_id}'
GROUP BY status;
-- Expected: no succeeded/executing row for policy-denied execution
```

---

## Scenario 3: Stale And Idempotency Protection Before Mutation

### Preconditions

- A failed safe task and a reviewed `restart_from_boundary` plan exist.
- Record child-task count and workspace digest before execution.

### Goal

Verify task drift and duplicate requests cannot repeat scheduler or workspace effects.

### Steps

1. Change the source task cycle/event watermark after creating the plan.
2. Run:

   ```bash
   orchestrator resume execute {plan_id} \
     --expected-state-version {state_version} \
     --reason "stale plan QA" \
     --idempotency-key {idempotency_key}
   ```

3. Verify failure contains `stale resume plan` and child/workspace state is unchanged.
4. Create and execute a fresh plan; repeat the same execute request/key.
5. Change a tracked workspace file after planning and run `cargo test -p agent-orchestrator handoff::tests --lib`.

### Expected

- Task-state, event-watermark, or git workspace drift and expired plans fail before execution reservation or enqueue.
- Only one caller receives execution ownership for one plan/idempotency key.
- Replaying the same key returns existing status and cannot create another child.
- A key reused with different reviewed input is rejected.

### Expected Data State

```sql
SELECT plan_id, idempotency_key, COUNT(*) AS attempts, MIN(status) AS status
FROM resume_executions
WHERE plan_id = '{fresh_plan_id}'
GROUP BY plan_id, idempotency_key;
-- Expected: attempts = 1; one terminal execution for the idempotency key
```

---

## Scenario 4: Reviewed Restart, Correlated Child, And Audit Ordering

### Preconditions

- Apply the deterministic fixture.
- A current replay-safe boundary exists.
- The caller has `operator` or `admin` role.

### Goal

Verify execution uses existing child creation/enqueue semantics, preserves correlation, and emits audit evidence only after state changes.

### Steps

1. Create a fresh `restart_from_boundary` plan and review `consequence.workspace_rollback == false`, step filter, child creation flag, and expiry.
2. Execute with a non-empty reason and unique idempotency key.
3. Inspect the returned `child_task_id`, parent/spawn reason, pending/running status, and event ordering.
4. Verify no git reset/checkout/stash action occurred and source workspace contents are unchanged by the resume controller.
5. Run `./scripts/qa/test-handoff-safe-resume.sh`.

### Expected

- A child is created with `parent_task_id={source_task_id}` and `spawn_reason=resume_boundary:{boundary_id}`.
- The child uses the selected step and remaining execution plan through normal enqueue.
- `resume_executed` and terminal `resume_executions` state appear only after enqueue succeeds.
- Attention state is not modified by planning/stale rejection; any later Attention change follows durable execution events.

### Expected Data State

```sql
SELECT t.parent_task_id, t.spawn_reason, t.status,
       re.status AS execution_status, e.event_type
FROM tasks t
JOIN resume_executions re ON re.child_task_id = t.id
JOIN resume_plans rp ON rp.id = re.plan_id
LEFT JOIN events e ON e.task_id = rp.task_id AND e.event_type = 'resume_executed'
WHERE t.id = '{child_task_id}';
-- Expected: correlated parent/reason, execution_status='succeeded', event_type='resume_executed'
```

---

## Scenario 5: RBAC And Accessible Consequence Dialog

### Preconditions

- Start the GUI against a daemon containing a paused or failed task.
- Test with both `read_only` and `operator` roles.

### Goal

Verify role boundaries, required review fields, visible safety messaging, and keyboard-complete dialog behavior.

### Steps

1. With `read_only`, open Process Workspace and inspect existing handoff/boundary context; verify "Generate handoff" and mutating "Preview resume" are unavailable.
2. With `operator`, select either the failed-process "Review safe resume" primary action or panel-level "Preview resume" and verify focus moves into the dialog.
3. Choose a boundary/mode, select "Create preview", and inspect side-effect warning, no-rollback statement, expiry, and consequence JSON.
4. Try executing with an empty reason and, when elevated, without checking confirmation.
5. Complete the form by keyboard, cycle focus with `Tab`/`Shift+Tab`, close with `Escape`, and verify focus returns to the actual initiating button.

### Expected

- `HandoffGet`/`ResumeBoundaryList` require `read_only+`; generation/planning/execution require `operator+`.
- Direct task-detail "Resume"/"Retry" controls do not bypass the reviewed flow.
- Required reason and elevated confirmation disable execution until valid.
- Focus is trapped while open, `Escape` closes, focus returns to "Review safe resume" or "Preview resume" according to the initiating control, and visible focus rings use the design token.
- The scrollable dialog remains usable on narrow screens and with long consequence JSON.

### Expected Data State

```sql
SELECT rpc, authz_result, role, rejection_stage
FROM control_plane_audit
WHERE rpc IN ('HandoffGenerate','ResumePlan','ResumeExecute','HandoffGet','ResumeBoundaryList')
ORDER BY id DESC;
-- Expected: read_only is allowed only on read RPCs; operator is allowed on all five
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Visible handoff entry and deterministic briefing | PASS | 2026-07-12 | Codex | Isolated same-cursor/redaction assertions passed; visible entry and production build inspected |
| 2 | Logical boundaries and non-idempotent default denial | PASS | 2026-07-12 | Codex | Workspace-safe classification and disabled elevated replay passed |
| 3 | Stale and idempotency protection before mutation | PASS | 2026-07-12 | Codex | Stale execution created no child; DB and tracked-workspace drift gates passed |
| 4 | Reviewed restart, correlated child, and audit ordering | PASS | 2026-07-12 | Codex | Correlated child, enqueue, execution row, resume event, and post-event Attention resolution passed |
| 5 | RBAC and accessible consequence dialog | PASS | 2026-07-12 | Codex | Role mapping tests, focus trap/restore implementation, Tauri check, and React build passed |
