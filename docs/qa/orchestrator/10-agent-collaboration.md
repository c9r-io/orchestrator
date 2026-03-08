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

Entry point: `orchestrator`

### Environment Isolation Setup

```bash
orchestrator project reset qa-collab --force --include-config
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project qa-collab
```

> **Note**: Use `project reset` + `apply --project` to isolate fixture agents
> from global/bootstrap agents. Project-scoped agent selection ensures only
> fixture-defined agents participate. Auto-ticket files are cleaned during reset.

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
   TASK_ID=$(orchestrator task create \
     --project qa-collab \
     --name "agentoutput-mainline" \
     --goal "structured output" \
     --workspace default \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
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
- Reset previous QA state — `project reset` clears task data, config, and auto-tickets.
- **Important**: This scenario requires the `plain-text-agent.yaml` fixture
  which defines an agent that produces non-JSON output. The base
  `echo-workflow.yaml` fixture must be applied first to provide the Workspace
  resource.

### Goal
Verify non-JSON output is rejected for strict phases.

### Steps
1. Reset and apply into project scope (two fixtures — base workspace + plain-text agent):
   ```bash
   orchestrator project reset qa-strict --force --include-config
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project qa-strict
   orchestrator apply -f fixtures/manifests/bundles/plain-text-agent.yaml --project qa-strict
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-strict \
     --name "strict-validation-test" \
     --goal "Test strict phase validation" \
     --workspace default \
     --workflow plain_text_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Query validation failure events:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events
      WHERE task_id='${TASK_ID}' AND event_type='output_validation_failed'
      ORDER BY id DESC LIMIT 5;"
   ```

4. Query agent selection distribution:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT agent_id, validation_status, COUNT(*)
      FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '${TASK_ID}')
      GROUP BY agent_id, validation_status;"
   ```

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `plain_text_agent` never selected | Fixture not applied with `--project`; global agents participate in selection | Use `apply -f ... --project <name>` to scope agents |
| No `output_validation_failed` events | Only JSON-producing agents were selected | Verify only project agents exist via `describe agent` |
| Workspace not found error | `plain-text-agent.yaml` does not define a Workspace resource | Apply `echo-workflow.yaml` first to provide the Workspace |

### Expected
- `plain_text_agent` appears in `command_runs` with `validation_status = 'failed'`
- `output_validation_failed` event appears for non-JSON strict-phase output
- Other fixture agents (e.g. `mock_echo`) appear with `validation_status = 'passed'`

### Expected Data State
```sql
SELECT phase, validation_status
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC;
-- Expected: plain_text_agent rows are marked failed; other agent rows are marked passed
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
   orchestrator debug --component messagebus
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
