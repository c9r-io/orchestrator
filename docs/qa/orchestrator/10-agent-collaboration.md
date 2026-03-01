# Orchestrator - Agent Collaboration Mainline Validation

**Module**: orchestrator
**Scope**: Validate structured AgentOutput handling, MessageBus publication, artifact behavior, template context, and prehook fields
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates collaboration-related behavior after scheduler mainline integration:

- phase output validation and normalization into `AgentOutput`
- event and MessageBus publication for phase execution results
- artifact persistence into run records
- template placeholders in scheduler execution path
- structured prehook context fields availability

Entry point: `./scripts/orchestrator.sh`

### Project Isolation Setup

```bash
./scripts/orchestrator.sh db reset --force --include-config
./scripts/orchestrator.sh init

QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
```

> Note: DB reset is required to clear residual workflows from prior test runs
> that may cause config validation or FOREIGN KEY errors.

---

## Scenario 1: Structured AgentOutput Persistence

### Preconditions
- Runtime initialized.
- At least one task has executed `qa`/`fix`/`retest`/`guard`.

### Goal
Verify scheduler stores structured run payload and validation status.

### Steps
1. Execute a task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "agentoutput-mainline" --goal "structured output" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ```
2. Inspect command run structured fields:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT phase, validation_status, substr(output_json,1,120), substr(artifacts_json,1,120) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 10;"
   ```

### Expected
- `validation_status` is populated per run.
- `output_json` stores serialized `AgentOutput`.
- `artifacts_json` stores artifact payload for parsed artifacts.

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND validation_status IN ('passed','failed')
  AND output_json <> '{}';
-- Expected: count >= 1
```

---

## Scenario 2: Strict Phase Validation Behavior

### Preconditions
- Runtime initialized.

### Goal
Verify non-JSON output is rejected for strict phases.

### Steps
1. Run a task where `qa` phase emits plain text.
2. Query validation failure events:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id='{task_id}' AND event_type='output_validation_failed' ORDER BY id DESC LIMIT 5;"
   ```

### Expected
- `output_validation_failed` event appears for non-JSON strict-phase output.
- Corresponding `command_runs.validation_status` is `failed`.

### Expected Data State
```sql
SELECT phase, validation_status
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC;
-- Expected: strict phase rows with non-JSON output are marked failed
```

---

## Scenario 3: MessageBus Publication Observability

### Preconditions
- Runtime initialized.

### Goal
Verify phase result publication is observable through persisted events.

### Steps
1. Run a task with at least one phase execution.
2. Query publication events:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id='{task_id}' AND event_type IN ('phase_output_published','bus_publish_failed') ORDER BY id DESC LIMIT 10;"
   ```
3. Check debug component output:
   ```bash
   ./scripts/orchestrator.sh debug --component messagebus
   ```

### Expected
- `phase_output_published` appears on successful publish path.
- `bus_publish_failed` appears only on degraded publish path.
- MessageBus debug component reports implementation at `src/collab.rs`.

### Expected Data State
```sql
SELECT COUNT(*)
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'phase_output_published';
-- Expected: count >= 1 for successful phase execution
```

---

## Scenario 4: Scheduler Template Placeholders

### Preconditions
- Agent template uses placeholders.

### Goal
Verify scheduler path renders supported placeholders.

### Steps
1. Configure/verify template containing placeholders:
   - `{rel_path}`
   - `{ticket_paths}`
   - `{phase}`
   - `{cycle}`
2. Run task and inspect command records:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT command FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='{task_id}') ORDER BY started_at DESC LIMIT 5;"
   ```

### Expected
- Stored command text includes rendered values for supported placeholders.
- Guard command path supports `{task_id}` and `{cycle}`.

### Expected Data State
```sql
SELECT command
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC
LIMIT 5;
-- Expected: command contains concrete values (no unresolved supported placeholders)
```

---

## Scenario 5: StepPrehookContext Structured Fields

### Preconditions
- Workflow prehook expressions are enabled.

### Goal
Verify structured prehook fields are available for CEL expressions.

### Steps
1. Check type definition:
   ```bash
   rg -n "struct StepPrehookContext|qa_confidence|qa_quality_score|fix_has_changes|upstream_artifacts" core/src/config.rs
   ```
2. Run workflow with prehook expression that references at least one structured field.

### Expected
- Context definition includes structured fields used by collaboration flow.
- CEL prehook can evaluate without missing-field errors.

### Expected Data State
```sql
SELECT event_type, payload_json
FROM events
WHERE task_id = '{task_id}'
  AND event_type IN ('step_started','step_skipped','step_finished')
ORDER BY id DESC
LIMIT 20;
-- Expected: prehook-driven branch/skip behavior recorded without context-resolution errors
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Structured AgentOutput Persistence | ☐ | | | |
| 2 | Strict Phase Validation Behavior | ☐ | | | |
| 3 | MessageBus Publication Observability | ☐ | | | |
| 4 | Scheduler Template Placeholders | ☐ | | | |
| 5 | StepPrehookContext Structured Fields | ☐ | | | |
